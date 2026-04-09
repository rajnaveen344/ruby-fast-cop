//! Style/Next - Use `next` to skip iteration instead of a condition at the end.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/next.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/Next";
const MSG: &str = "Use `next` to skip iteration.";

/// RuboCop's enumerator_method? check
const ENUMERATOR_METHODS: &[&str] = &[
    "collect", "collect!", "detect", "downto", "each", "each_cons", "each_key",
    "each_object", "each_pair", "each_slice", "each_value", "each_with_index",
    "each_with_object", "find", "find_all", "find_index", "flat_map", "grep",
    "grep_v", "inject", "loop", "map", "map!", "max_by", "min_by", "minmax_by",
    "reduce", "reject", "reject!", "reverse_each", "select", "select!", "sort_by",
    "sum", "times", "upto",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    SkipModifierIfs,
    Always,
}

pub struct Next {
    style: EnforcedStyle,
    min_body_length: i64,
    allow_consecutive_conditionals: bool,
}

impl Next {
    pub fn new() -> Self {
        Self {
            style: EnforcedStyle::SkipModifierIfs,
            min_body_length: 1,
            allow_consecutive_conditionals: false,
        }
    }

    pub fn with_config(
        style: EnforcedStyle,
        min_body_length: i64,
        allow_consecutive_conditionals: bool,
    ) -> Self {
        Self {
            style,
            min_body_length,
            allow_consecutive_conditionals,
        }
    }
}

impl Default for Next {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for Next {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = NextVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct NextVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a Next,
    offenses: Vec<Offense>,
}

impl<'a> NextVisitor<'a> {
    fn check_body(&mut self, body: Option<Node>) {
        let body = match body {
            Some(b) => b,
            None => return,
        };

        if !self.ends_with_condition(&body) {
            return;
        }

        // Find the offending node (the last if/unless without else)
        let (off_start, off_cond_end, off_node_start, off_node_end) =
            match self.find_offense_location(&body) {
                Some(loc) => loc,
                None => return,
            };

        // AllowConsecutiveConditionals
        if self.cop.allow_consecutive_conditionals {
            if self.is_consecutive_conditional(&body, off_node_start, off_node_end) {
                return;
            }
        }

        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention, off_start, off_cond_end,
        ));
    }

    fn ends_with_condition(&self, body: &Node) -> bool {
        if self.simple_if_without_break(body) {
            return true;
        }

        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            if let Some(last) = children.last() {
                return self.simple_if_without_break(last);
            }
        }

        false
    }

    fn simple_if_without_break(&self, node: &Node) -> bool {
        if !self.if_without_else(node) {
            return false;
        }
        if self.if_else_children(node) {
            return false;
        }
        if self.allowed_modifier_if(node) {
            return false;
        }
        !self.exit_body_type(node)
    }

    fn if_without_else(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                // Not ternary
                if let Some(kw_loc) = n.if_keyword_loc() {
                    let kw = self.ctx.src(kw_loc.start_offset(), kw_loc.end_offset());
                    if kw == "?" {
                        return false;
                    }
                } else {
                    return false;
                }
                n.subsequent().is_none()
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                n.else_clause().is_none()
            }
            _ => false,
        }
    }

    fn if_else_children(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                if let Some(stmts) = n.statements() {
                    for child in stmts.body().iter() {
                        if self.has_else(&child) {
                            return true;
                        }
                    }
                }
                false
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                if let Some(stmts) = n.statements() {
                    for child in stmts.body().iter() {
                        if self.has_else(&child) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn has_else(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().subsequent().is_some(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().else_clause().is_some(),
            _ => false,
        }
    }

    fn allowed_modifier_if(&self, node: &Node) -> bool {
        let is_modifier = self.is_modifier_form(node);
        if is_modifier {
            self.cop.style == EnforcedStyle::SkipModifierIfs
        } else {
            !self.min_body_length_met(node)
        }
    }

    fn is_modifier_form(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().end_keyword_loc().is_none(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().end_keyword_loc().is_none(),
            _ => false,
        }
    }

    fn min_body_length_met(&self, node: &Node) -> bool {
        if self.cop.min_body_length < 0 {
            return false;
        }
        let body_length = self.body_line_count(node);
        body_length >= self.cop.min_body_length as usize
    }

    fn body_line_count(&self, node: &Node) -> usize {
        let stmts = match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().statements(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().statements(),
            _ => return 0,
        };
        match stmts {
            Some(s) => {
                let first = s.body().iter().next();
                let last = s.body().iter().last();
                match (first, last) {
                    (Some(f), Some(l)) => {
                        let start_line = self.ctx.line_of(f.location().start_offset());
                        let end_line = self.ctx.line_of(l.location().end_offset());
                        end_line - start_line + 1
                    }
                    _ => 0,
                }
            }
            None => 0,
        }
    }

    fn exit_body_type(&self, node: &Node) -> bool {
        let stmts = match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().statements(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().statements(),
            _ => return false,
        };
        match stmts {
            Some(s) => {
                // Check first child (the if_branch)
                if let Some(first) = s.body().iter().next() {
                    matches!(first, Node::BreakNode { .. } | Node::ReturnNode { .. })
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// Find the offense location (start, cond_end, node_start, node_end) for the offending if/unless.
    fn find_offense_location(&self, body: &Node) -> Option<(usize, usize, usize, usize)> {
        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            if let Some(last) = children.last() {
                if self.simple_if_without_break(last) {
                    return self.offense_loc_for_node(last);
                }
            }
        }

        // Body itself is if/unless
        if self.simple_if_without_break(body) {
            return self.offense_loc_for_node(body);
        }

        None
    }

    fn offense_loc_for_node(&self, node: &Node) -> Option<(usize, usize, usize, usize)> {
        let start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let cond_end = match node {
            Node::IfNode { .. } => {
                node.as_if_node().unwrap().predicate().location().end_offset()
            }
            Node::UnlessNode { .. } => {
                node.as_unless_node().unwrap().predicate().location().end_offset()
            }
            _ => return None,
        };
        Some((start, cond_end, start, node_end))
    }

    fn is_consecutive_conditional(&self, body: &Node, node_start: usize, node_end: usize) -> bool {
        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            for i in 1..children.len() {
                let child_start = children[i].location().start_offset();
                let child_end = children[i].location().end_offset();
                if child_start == node_start && child_end == node_end {
                    if matches!(&children[i - 1], Node::IfNode { .. } | Node::UnlessNode { .. }) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn is_enumerator_method(name: &str) -> bool {
        ENUMERATOR_METHODS.contains(&name)
    }
}

impl Visit<'_> for NextVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        if Self::is_enumerator_method(&method_name) {
            if let Some(block) = node.block() {
                if let Some(block_node) = block.as_block_node() {
                    self.check_body(block_node.body());
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if let Some(stmts) = node.statements() {
            self.check_body(Some(stmts.as_node()));
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if let Some(stmts) = node.statements() {
            self.check_body(Some(stmts.as_node()));
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        if let Some(stmts) = node.statements() {
            self.check_body(Some(stmts.as_node()));
        }
        ruby_prism::visit_for_node(self, node);
    }
}
