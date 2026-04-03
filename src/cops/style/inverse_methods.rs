//! Style/InverseMethods - Checks for usages of `not` or `!` on a method when an inverse
//! method can be used instead, and for blocks with inverted conditions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/inverse_methods.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

fn is_camel_case(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase()) && s.chars().any(|c| c.is_ascii_lowercase())
}

fn is_camel_case_constant(node: &Node, source: &str) -> bool {
    if !matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }) {
        return false;
    }
    let loc = node.location();
    source
        .get(loc.start_offset()..loc.end_offset())
        .map_or(false, |s| is_camel_case(s))
}

const CLASS_COMPARISON_METHODS: &[&str] = &["<=", ">=", "<", ">"];
const SAFE_NAV_INCOMPATIBLE: &[&str] = &["<=", ">=", "<", ">", "any?", "none?"];
const NEGATED_EQUALITY_METHODS: &[&str] = &["!=", "!~"];

/// Visitor to check if a subtree contains any NextNode
struct NextFinder {
    found: bool,
}

impl Visit<'_> for NextFinder {
    fn visit_next_node(&mut self, _node: &ruby_prism::NextNode) {
        self.found = true;
    }
}

pub struct InverseMethods {
    inverse_methods: HashMap<String, String>,
    inverse_blocks: HashMap<String, String>,
}

impl InverseMethods {
    pub fn new() -> Self {
        let mut inverse_methods = HashMap::new();
        for (a, b) in &[
            ("any?", "none?"),
            ("even?", "odd?"),
            ("present?", "blank?"),
            ("include?", "exclude?"),
            ("==", "!="),
            ("=~", "!~"),
            ("<", ">="),
            (">", "<="),
        ] {
            inverse_methods.insert(a.to_string(), b.to_string());
            inverse_methods.insert(b.to_string(), a.to_string());
        }
        let mut inverse_blocks = HashMap::new();
        for (a, b) in &[("select", "reject"), ("select!", "reject!")] {
            inverse_blocks.insert(a.to_string(), b.to_string());
            inverse_blocks.insert(b.to_string(), a.to_string());
        }
        Self {
            inverse_methods,
            inverse_blocks,
        }
    }

    pub fn with_config(
        inverse_methods_cfg: HashMap<String, String>,
        inverse_blocks_cfg: HashMap<String, String>,
    ) -> Self {
        let mut inverse_methods = HashMap::new();
        for (k, v) in &inverse_methods_cfg {
            inverse_methods.insert(k.clone(), v.clone());
            inverse_methods.insert(v.clone(), k.clone());
        }
        let mut inverse_blocks = HashMap::new();
        for (k, v) in &inverse_blocks_cfg {
            inverse_blocks.insert(k.clone(), v.clone());
            inverse_blocks.insert(v.clone(), k.clone());
        }
        Self {
            inverse_methods,
            inverse_blocks,
        }
    }

    fn is_csend(call: &ruby_prism::CallNode, source: &str) -> bool {
        if let Some(op) = call.call_operator_loc() {
            source.get(op.start_offset()..op.end_offset()) == Some("&.")
        } else {
            false
        }
    }

    fn safe_navigation_incompatible(call: &ruby_prism::CallNode, source: &str) -> bool {
        if !Self::is_csend(call, source) {
            return false;
        }
        let method = String::from_utf8_lossy(call.name().as_slice());
        SAFE_NAV_INCOMPATIBLE.contains(&method.as_ref())
    }

    fn possible_class_hierarchy_check(
        lhs: &Node,
        rhs_args: Option<ruby_prism::ArgumentsNode>,
        method: &str,
        source: &str,
    ) -> bool {
        if !CLASS_COMPARISON_METHODS.contains(&method) {
            return false;
        }
        if is_camel_case_constant(lhs, source) {
            return true;
        }
        if let Some(args) = rhs_args {
            let args_vec: Vec<_> = args.arguments().iter().collect();
            if args_vec.len() == 1 && is_camel_case_constant(&args_vec[0], source) {
                return true;
            }
        }
        false
    }

    /// Extract the inner method call from the receiver of a `!` call.
    fn extract_method_call<'a>(receiver: &Node<'a>) -> Option<ruby_prism::CallNode<'a>> {
        match receiver {
            Node::CallNode { .. } => receiver.as_call_node(),
            Node::ParenthesesNode { .. } => {
                let paren = receiver.as_parentheses_node()?;
                let body = paren.body()?;
                let stmts = body.as_statements_node()?;
                let stmts_vec: Vec<_> = stmts.body().iter().collect();
                if stmts_vec.len() != 1 {
                    return None;
                }
                stmts_vec[0].as_call_node()
            }
            _ => None,
        }
    }

    /// Check if an expression is "inverted" (negated)
    fn is_inverted_expr(node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = String::from_utf8_lossy(call.name().as_slice());
                method == "!" || NEGATED_EQUALITY_METHODS.contains(&method.as_ref())
            }
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if let Some(stmts) = body.as_statements_node() {
                        if let Some(last) = stmts.body().iter().last() {
                            return Self::is_inverted_expr(&last);
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }
}

impl Default for InverseMethods {
    fn default() -> Self {
        Self::new()
    }
}

/// Visitor that collects offenses, tracking ignored ranges for nested inverse blocks
struct InverseMethodsVisitor<'a> {
    cop: &'a InverseMethods,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Byte ranges of nodes inside inverse blocks whose inner inversions should be ignored
    ignored_ranges: Vec<(usize, usize)>,
}

impl<'a> InverseMethodsVisitor<'a> {
    fn new(cop: &'a InverseMethods, ctx: &'a CheckContext<'a>) -> Self {
        Self {
            cop,
            ctx,
            offenses: Vec::new(),
            ignored_ranges: Vec::new(),
        }
    }

    fn is_in_ignored_range(&self, start: usize, end: usize) -> bool {
        self.ignored_ranges
            .iter()
            .any(|&(ig_start, ig_end)| start >= ig_start && end <= ig_end)
    }

    /// Check `!` calls for inverse method pattern
    fn check_inverse_method(&mut self, not_call: &ruby_prism::CallNode) {
        let source = self.ctx.source;
        let receiver = match not_call.receiver() {
            Some(r) => r,
            None => return,
        };

        let method_call = match InverseMethods::extract_method_call(&receiver) {
            Some(c) => c,
            None => return,
        };

        let method_name = String::from_utf8_lossy(method_call.name().as_slice()).to_string();

        if !self.cop.inverse_methods.contains_key(&method_name) {
            return;
        }

        let not_start = not_call.location().start_offset();
        let not_end = not_call.location().end_offset();

        // Check if this node is in an ignored range (inside an inverse block offense)
        if self.is_in_ignored_range(not_start, not_end) {
            return;
        }

        // Check for double negation: if the byte before this `!` is also `!`
        // Prism parses `!!x` as nested `!` calls. The outer `!` has the inner as receiver.
        // When we process the inner `!`, check if preceded by another `!`.
        if not_start > 0 && source.as_bytes()[not_start - 1] == b'!' {
            return;
        }
        // Also check for `not` keyword double negation
        if not_start >= 4 && &source[not_start - 4..not_start] == "not " {
            // Check if this `not` itself is preceded by `not `
            let before = &source[..not_start - 4];
            if before.trim_end().ends_with("not") {
                return;
            }
        }

        // Check safe navigation incompatibility
        if InverseMethods::safe_navigation_incompatible(&method_call, source) {
            return;
        }

        // Check class hierarchy comparison
        let lhs = match method_call.receiver() {
            Some(l) => l,
            None => return,
        };
        if InverseMethods::possible_class_hierarchy_check(
            &lhs,
            method_call.arguments(),
            &method_name,
            source,
        ) {
            return;
        }

        let inverse = &self.cop.inverse_methods[&method_name];
        let message = format!("Use `{}` instead of inverting `{}`.", inverse, method_name);

        self.offenses.push(self.ctx.offense_with_range(
            self.cop.name(),
            &message,
            self.cop.severity(),
            not_start,
            not_end,
        ));
    }

    /// Check calls with blocks for inverse block pattern
    fn check_inverse_block(&mut self, call_node: &ruby_prism::CallNode) {
        let source = self.ctx.source;
        let method_name = String::from_utf8_lossy(call_node.name().as_slice()).to_string();

        if !self.cop.inverse_blocks.contains_key(&method_name) {
            return;
        }

        let block = match call_node.block() {
            Some(b) => b,
            None => return,
        };
        let block_node = match block.as_block_node() {
            Some(b) => b,
            None => return,
        };

        // Check for `next` statements
        if let Some(body) = block_node.body() {
            let mut finder = NextFinder { found: false };
            finder.visit(&body);
            if finder.found {
                return;
            }
        }

        // Get the last expression in the block body
        let last_expr = match block_node.body() {
            Some(body) => match body.as_statements_node() {
                Some(stmts) => match stmts.body().iter().last() {
                    Some(e) => e,
                    None => return,
                },
                None => return,
            },
            None => return,
        };

        if !InverseMethods::is_inverted_expr(&last_expr) {
            return;
        }

        // Check for double negation on block parent
        let recv_start = if let Some(recv) = call_node.receiver() {
            recv.location().start_offset()
        } else {
            call_node.location().start_offset()
        };
        if recv_start >= 2 {
            let b0 = source.as_bytes()[recv_start - 2];
            let b1 = source.as_bytes()[recv_start - 1];
            if b0 == b'!' && b1 == b'!' {
                return;
            }
        }

        let inverse = &self.cop.inverse_blocks[&method_name];
        let message = format!("Use `{}` instead of inverting `{}`.", inverse, method_name);

        let offense_start = if let Some(recv) = call_node.receiver() {
            recv.location().start_offset()
        } else {
            call_node.location().start_offset()
        };

        let open_loc = block_node.opening_loc();
        let is_do = source.get(open_loc.start_offset()..open_loc.end_offset()) == Some("do");
        let offense_end = if is_do {
            if let Some(params) = block_node.parameters() {
                params.location().end_offset()
            } else {
                open_loc.end_offset()
            }
        } else {
            block_node.location().end_offset()
        };

        self.offenses.push(self.ctx.offense_with_range(
            self.cop.name(),
            &message,
            self.cop.severity(),
            offense_start,
            offense_end,
        ));

        // Add the last expression as an ignored range to suppress inner inverse method offenses
        // (mirrors RuboCop's `ignore_node(block)` in on_block)
        let last_loc = last_expr.location();
        self.ignored_ranges
            .push((last_loc.start_offset(), last_loc.end_offset()));
    }
}

impl Visit<'_> for InverseMethodsVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice());

        if method_name == "!" {
            self.check_inverse_method(node);
        }

        // Check for inverse block (call with block whose last expr is inverted)
        // Do this BEFORE descending so we can add ignored ranges
        if self.cop.inverse_blocks.contains_key(method_name.as_ref()) {
            self.check_inverse_block(node);
        }

        // Continue traversal
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for InverseMethods {
    fn name(&self) -> &'static str {
        "Style/InverseMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = InverseMethodsVisitor::new(self, ctx);
        ruby_prism::visit_program_node(&mut visitor, node);
        visitor.offenses
    }
}
