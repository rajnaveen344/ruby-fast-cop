//! Style/NumericPredicate - Prefer `zero?`/`positive?`/`negative?` over comparisons to 0.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/numeric_predicate.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/NumericPredicate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle { Predicate, Comparison }

pub struct NumericPredicate {
    style: EnforcedStyle,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl Default for NumericPredicate {
    fn default() -> Self {
        Self { style: EnforcedStyle::Predicate, allowed_methods: Vec::new(), allowed_patterns: Vec::new() }
    }
}

impl NumericPredicate {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(
        style: EnforcedStyle,
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self { style, allowed_methods, allowed_patterns }
    }
}

impl Cop for NumericPredicate {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            cop: self,
            ctx,
            ancestor_names: Vec::new(),
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a, 'b> {
    cop: &'a NumericPredicate,
    ctx: &'a CheckContext<'b>,
    // Stack of enclosing call/block method names
    ancestor_names: Vec<String>,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visitor<'a, 'b> {
    fn is_allowed(&self, method_name: &str) -> bool {
        if is_method_allowed(&self.cop.allowed_methods, &self.cop.allowed_patterns, method_name, None) {
            return true;
        }
        self.ancestor_names.iter().any(|n| {
            is_method_allowed(&self.cop.allowed_methods, &self.cop.allowed_patterns, n, None)
        })
    }

    fn src(&self, start: usize, end: usize) -> String {
        self.ctx.source[start..end].to_string()
    }

    fn process_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        let method_str = method.as_ref();

        match self.cop.style {
            EnforcedStyle::Predicate => {
                // Match `recv OP 0` or `0 OP recv` where OP in {==, >, <}, recv not gvar, not nil
                if !matches!(method_str, "==" | ">" | "<") { return; }
                let Some(recv) = node.receiver() else { return; };
                let Some(args_node) = node.arguments() else { return; };
                let args: Vec<_> = args_node.arguments().iter().collect();
                if args.len() != 1 { return; }
                let arg = &args[0];

                // Determine numeric expression location + operator (inverted for yoda)
                let (num_start, num_end, is_gvar, is_binary_op_unparen, op_owned): (usize, usize, bool, bool, String) = if is_zero_with_source(&recv, self.ctx.source) {
                    let inv = invert_op(method_str);
                    (
                        arg.location().start_offset(),
                        arg.location().end_offset(),
                        matches!(arg, Node::GlobalVariableReadNode { .. }),
                        require_parens(arg),
                        inv.to_string(),
                    )
                } else if is_zero_with_source(arg, self.ctx.source) {
                    (
                        recv.location().start_offset(),
                        recv.location().end_offset(),
                        matches!(recv, Node::GlobalVariableReadNode { .. }),
                        require_parens(&recv),
                        method_str.to_string(),
                    )
                } else {
                    return;
                };

                if is_gvar { return; }

                // Ruby version check for > and <
                let op = op_owned.as_str();
                if matches!(op, ">" | "<") {
                    if self.ctx.target_ruby_version < 2.3 { return; }
                }

                if self.is_allowed(method_str) { return; }

                // Replacement construction
                let predicate_method = match op {
                    "==" => "zero?",
                    ">" => "positive?",
                    "<" => "negative?",
                    _ => return,
                };
                let numeric_src = self.src(num_start, num_end);
                let num_piece = if is_binary_op_unparen {
                    format!("({})", numeric_src)
                } else {
                    numeric_src.clone()
                };
                let replacement = format!("{}.{}", num_piece, predicate_method);

                let current = self.src(node.location().start_offset(), node.location().end_offset());
                let msg = format!("Use `{}` instead of `{}`.", replacement, current);
                let correction = Correction::replace(
                    node.location().start_offset(),
                    node.location().end_offset(),
                    replacement,
                );
                self.offenses.push(
                    self.ctx.offense(COP_NAME, &msg, Severity::Convention, &node.location())
                        .with_correction(correction),
                );
            }
            EnforcedStyle::Comparison => {
                if !matches!(method_str, "zero?" | "positive?" | "negative?") { return; }
                let Some(recv) = node.receiver() else { return; };
                if node.arguments().is_some() { return; }
                if self.is_allowed(method_str) { return; }

                let op = match method_str {
                    "zero?" => "==",
                    "positive?" => ">",
                    "negative?" => "<",
                    _ => return,
                };
                let recv_src = self.src(recv.location().start_offset(), recv.location().end_offset());
                let is_negated = self.parent_is_bang_call(node);
                let base = if is_negated {
                    format!("({} {} 0)", recv_src, op)
                } else {
                    format!("{} {} 0", recv_src, op)
                };
                let current = self.src(node.location().start_offset(), node.location().end_offset());
                let msg = format!("Use `{}` instead of `{}`.", base, current);
                let correction = Correction::replace(
                    node.location().start_offset(),
                    node.location().end_offset(),
                    base,
                );
                self.offenses.push(
                    self.ctx.offense(COP_NAME, &msg, Severity::Convention, &node.location())
                        .with_correction(correction),
                );
            }
        }
    }

    fn parent_is_bang_call(&self, _node: &ruby_prism::CallNode) -> bool {
        // Without explicit parent tracking: check our ancestor_names stack last
        matches!(self.ancestor_names.last().map(String::as_str), Some("!"))
    }
}

fn is_zero_with_source(node: &Node, source: &str) -> bool {
    if let Node::IntegerNode { .. } = node {
        let s = &source[node.location().start_offset()..node.location().end_offset()];
        return s == "0";
    }
    false
}

fn invert_op(op: &str) -> &str {
    match op { ">" => "<", "<" => ">", o => o }
}

/// require_parentheses? per RuboCop: send_type + binary_operation + !parenthesized
fn require_parens(node: &Node) -> bool {
    let call = match node.as_call_node() { Some(c) => c, None => return false };
    let name = node_name!(call);
    let name_s = name.as_ref();
    let is_binary = matches!(name_s,
        "+" | "-" | "*" | "/" | "%" | "**" | "<<" | ">>" | "&" | "|" | "^"
        | "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>"
    );
    if !is_binary { return false; }
    if call.receiver().is_none() { return false; }
    // Check args count
    let arg_count = call.arguments().map(|a| a.arguments().iter().count()).unwrap_or(0);
    if arg_count != 1 { return false; }
    // parenthesized? — look for opening_loc
    call.opening_loc().is_none()
}

impl<'a, 'b> Visit<'_> for Visitor<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.process_call(node);
        // Track method-name ancestry for AllowedMethods-in-ancestor check
        let name = node_name!(node).to_string();
        self.ancestor_names.push(name);
        ruby_prism::visit_call_node(self, node);
        self.ancestor_names.pop();
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // The block's method name is provided by the enclosing call node which is already on the stack.
        ruby_prism::visit_block_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String, allowed_methods: Vec<String>, allowed_patterns: Vec<String> }

crate::register_cop!("Style/NumericPredicate", |cfg| {
    let c: Cfg = cfg.typed("Style/NumericPredicate");
    let style = match c.enforced_style.as_str() {
        "comparison" => EnforcedStyle::Comparison,
        _ => EnforcedStyle::Predicate,
    };
    Some(Box::new(NumericPredicate::with_config(style, c.allowed_methods, c.allowed_patterns)))
});
