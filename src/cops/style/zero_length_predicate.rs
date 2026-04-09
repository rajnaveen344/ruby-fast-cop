//! Style/ZeroLengthPredicate - Checks for numeric comparisons that can be replaced by a predicate method.
//!
//! Detects `receiver.length == 0`, `receiver.size == 0`, `receiver.length.zero?` etc.
//! and suggests using `empty?` or `!empty?` instead.
//!
//! NOTE: File, Tempfile, and StringIO do not have `empty?`, so `size == 0` and `size.zero?`
//! are allowed for those types.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/zero_length_predicate.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct ZeroLengthPredicate;

impl ZeroLengthPredicate {
    pub fn new() -> Self {
        Self
    }
}

/// Check if a call node is a `.length` or `.size` method call with a receiver and no args
fn is_length_or_size(node: &ruby_prism::CallNode) -> bool {
    let name = node_name!(node);
    (name == "length" || name == "size") && node.receiver().is_some() && node.arguments().is_none()
}

/// Get integer value from an IntegerNode via source text
fn get_int_value(node: &Node, source: &str) -> Option<i64> {
    if let Node::IntegerNode { .. } = node {
        let loc = node.location();
        let text = &source[loc.start_offset()..loc.end_offset()];
        text.parse::<i64>().ok()
    } else {
        None
    }
}

/// Check if a length/size call's receiver chain matches non-polymorphic types
/// (File, Tempfile, StringIO — these don't have `empty?`)
fn is_non_polymorphic(length_call: &ruby_prism::CallNode, source: &str) -> bool {
    let receiver = match length_call.receiver() {
        Some(r) => r,
        None => return false,
    };

    let inner_call = match receiver.as_call_node() {
        Some(c) => c,
        None => return false,
    };

    let inner_name = node_name!(inner_call);

    if inner_name == "stat" {
        if let Some(inner_recv) = inner_call.receiver() {
            return is_constant_named(&inner_recv, "File", source);
        }
    }

    if inner_name == "new" || inner_name == "open" {
        if let Some(inner_recv) = inner_call.receiver() {
            return is_constant_named(&inner_recv, "File", source)
                || is_constant_named(&inner_recv, "Tempfile", source)
                || is_constant_named(&inner_recv, "StringIO", source);
        }
    }

    false
}

/// Check if a node is a constant with the given name (handles both `Foo` and `::Foo`)
fn is_constant_named(node: &Node, name: &str, source: &str) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        let cname = node_name!(c);
        return cname == name;
    }
    if let Node::ConstantPathNode { .. } = node {
        let loc = node.location();
        let text = &source[loc.start_offset()..loc.end_offset()];
        return text == format!("::{}", name);
    }
    false
}

fn is_safe_nav(call: &ruby_prism::CallNode, source: &str) -> bool {
    if let Some(op_loc) = call.call_operator_loc() {
        &source[op_loc.start_offset()..op_loc.end_offset()] == "&."
    } else {
        false
    }
}

impl Cop for ZeroLengthPredicate {
    fn name(&self) -> &'static str {
        "Style/ZeroLengthPredicate"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = node_name!(node);

        match method_name.as_ref() {
            "zero?" => self.check_zero_predicate(node, ctx),
            "==" | "!=" | "<" | ">" => self.check_comparison(node, ctx),
            _ => vec![],
        }
    }
}

impl ZeroLengthPredicate {
    /// Handle `x.length.zero?` and `x.size.zero?`
    fn check_zero_predicate(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let length_call = match receiver.as_call_node() {
            Some(c) if is_length_or_size(&c) => c,
            _ => return vec![],
        };

        if length_call.receiver().is_none() {
            return vec![];
        }

        if is_non_polymorphic(&length_call, ctx.source) {
            return vec![];
        }

        // Offense range: from method name of length/size to end of zero?
        let length_msg_loc = length_call.message_loc().unwrap();
        let offense_start = length_msg_loc.start_offset();
        let offense_end = node.location().end_offset();
        let offense_text = &ctx.source[offense_start..offense_end];

        let message = format!("Use `empty?` instead of `{}`.", offense_text);

        let mut offense = ctx.offense_with_range(
            self.name(),
            &message,
            self.severity(),
            offense_start,
            offense_end,
        );

        let correction = Correction::replace(offense_start, offense_end, "empty?");
        offense = offense.with_correction(correction);

        vec![offense]
    }

    /// Handle comparison patterns
    fn check_comparison(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let op = node_name!(node).to_string();

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };

        let arg_nodes: Vec<_> = args.arguments().iter().collect();
        if arg_nodes.len() != 1 {
            return vec![];
        }
        let rhs = &arg_nodes[0];

        // A) length/size OP int
        if let Some(length_call) = receiver.as_call_node() {
            if is_length_or_size(&length_call) {
                if let Some(int_val) = get_int_value(rhs, ctx.source) {
                    return self.check_length_op_int(node, &length_call, &op, int_val, ctx);
                }
            }
        }

        // B) int OP length/size
        if let Some(int_val) = get_int_value(&receiver, ctx.source) {
            if let Some(length_call) = rhs.as_call_node() {
                if is_length_or_size(&length_call) {
                    return self.check_int_op_length(node, &length_call, &op, int_val, ctx);
                }
            }
        }

        vec![]
    }

    /// `x.length == 0`, `x.length < 1`, `x.length > 0`, `x.length != 0`
    fn check_length_op_int(
        &self,
        comparison: &ruby_prism::CallNode,
        length_call: &ruby_prism::CallNode,
        op: &str,
        int_val: i64,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let length_name = node_name!(length_call);

        let is_zero = match (op, int_val) {
            ("==", 0) => Some(true),
            ("<", 1) => Some(true),
            (">", 0) => Some(false),
            ("!=", 0) => Some(false),
            _ => None,
        };

        let is_zero = match is_zero {
            Some(z) => z,
            None => return vec![],
        };

        // Nonzero + safe navigation = skip (RuboCop only handles nonzero for on_send)
        if !is_zero && is_safe_nav(length_call, ctx.source) {
            return vec![];
        }

        if is_non_polymorphic(length_call, ctx.source) {
            return vec![];
        }

        let msg_current = format!("{} {} {}", length_name, op, int_val);
        let (template, replacement) = if is_zero {
            (
                format!("Use `empty?` instead of `{}`.", msg_current),
                self.build_empty(length_call, ctx.source),
            )
        } else {
            (
                format!("Use `!empty?` instead of `{}`.", msg_current),
                self.build_not_empty(length_call, ctx.source),
            )
        };

        let start = comparison.location().start_offset();
        let end = comparison.location().end_offset();

        let mut offense = ctx.offense_with_range(self.name(), &template, self.severity(), start, end);
        offense = offense.with_correction(Correction::replace(start, end, &replacement));
        vec![offense]
    }

    /// `0 == x.length`, `1 > x.length`, `0 < x.length`, `0 != x.length`
    fn check_int_op_length(
        &self,
        comparison: &ruby_prism::CallNode,
        length_call: &ruby_prism::CallNode,
        op: &str,
        int_val: i64,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let length_name = node_name!(length_call);

        let is_zero = match (op, int_val) {
            ("==", 0) => Some(true),
            (">", 1) => Some(true),
            ("<", 0) => Some(false),
            ("!=", 0) => Some(false),
            _ => None,
        };

        let is_zero = match is_zero {
            Some(z) => z,
            None => return vec![],
        };

        if !is_zero && is_safe_nav(length_call, ctx.source) {
            return vec![];
        }

        if is_non_polymorphic(length_call, ctx.source) {
            return vec![];
        }

        let msg_current = format!("{} {} {}", int_val, op, length_name);
        let (template, replacement) = if is_zero {
            (
                format!("Use `empty?` instead of `{}`.", msg_current),
                self.build_empty(length_call, ctx.source),
            )
        } else {
            (
                format!("Use `!empty?` instead of `{}`.", msg_current),
                self.build_not_empty(length_call, ctx.source),
            )
        };

        let start = comparison.location().start_offset();
        let end = comparison.location().end_offset();

        let mut offense = ctx.offense_with_range(self.name(), &template, self.severity(), start, end);
        offense = offense.with_correction(Correction::replace(start, end, &replacement));
        vec![offense]
    }

    fn build_empty(&self, length_call: &ruby_prism::CallNode, source: &str) -> String {
        let recv = length_call.receiver().unwrap();
        let recv_src = &source[recv.location().start_offset()..recv.location().end_offset()];
        let dot = self.dot_str(length_call, source);
        format!("{}{}empty?", recv_src, dot)
    }

    fn build_not_empty(&self, length_call: &ruby_prism::CallNode, source: &str) -> String {
        let recv = length_call.receiver().unwrap();
        let recv_src = &source[recv.location().start_offset()..recv.location().end_offset()];
        let dot = self.dot_str(length_call, source);
        format!("!{}{}empty?", recv_src, dot)
    }

    fn dot_str<'a>(&self, call: &'a ruby_prism::CallNode, source: &'a str) -> &'a str {
        if let Some(op_loc) = call.call_operator_loc() {
            &source[op_loc.start_offset()..op_loc.end_offset()]
        } else {
            "."
        }
    }
}
