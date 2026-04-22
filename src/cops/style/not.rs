//! Style/Not cop
//!
//! Checks for `not` keyword usage — use `!` instead.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Node};

#[derive(Default)]
pub struct Not;

impl Not {
    pub fn new() -> Self {
        Self
    }

    /// Detect if this call is `not expr` (keyword form), not `!expr`.
    fn is_prefix_not(node: &CallNode, source: &str) -> bool {
        let method = node_name!(node);
        if method != "!" {
            return false;
        }
        // No call operator (no `.` or `&.`)
        if node.call_operator_loc().is_some() {
            return false;
        }
        // Must have a receiver
        if node.receiver().is_none() {
            return false;
        }
        // Source at call start must be `not` (keyword), not `!`
        let start = node.location().start_offset();
        let src = &source[start..];
        src.starts_with("not") && src.as_bytes().get(3).map_or(true, |&b| !b.is_ascii_alphanumeric() && b != b'_')
    }

    /// Check if receiver is a comparison operator that has an opposite
    fn opposite_method(method: &str) -> Option<&'static str> {
        match method {
            "==" => Some("!="),
            "!=" => Some("=="),
            "<=" => Some(">"),
            ">" => Some("<="),
            "<" => Some(">="),
            ">=" => Some("<"),
            _ => None,
        }
    }

    /// Check if receiver requires parentheses when negated with `!`
    fn requires_parens(recv: &Node) -> bool {
        // operator keywords: and/or/not
        if recv.as_and_node().is_some() || recv.as_or_node().is_some() {
            return true;
        }
        // binary send operations
        if let Some(call) = recv.as_call_node() {
            let m = node_name!(call);
            // binary operations that have lower precedence
            if matches!(m.as_ref(),
                "+" | "-" | "*" | "/" | "%" | "**" | ">>" | "<<" | "&" | "|" | "^"
                | "==" | "===" | "!=" | "<=" | ">=" | "<" | ">"
                | "<=>" | "=~" | "!~"
                | "&&" | "||"
            ) && call.receiver().is_some() {
                return true;
            }
        }
        // ternary if
        if let Some(if_node) = recv.as_if_node() {
            // ternary has a predicate/then/else on same line
            if if_node.then_keyword_loc().is_none() && if_node.end_keyword_loc().is_none() {
                return true;
            }
        }
        false
    }
}

impl Cop for Not {
    fn name(&self) -> &'static str {
        "Style/Not"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_prefix_not(node, ctx.source) {
            return vec![];
        }

        // Offense is on the `not` selector: start to start+3
        let start = node.location().start_offset();
        let end = start + 3; // "not"

        let msg = "Use `!` instead of `not`.";
        vec![ctx.offense_with_range(self.name(), msg, self.severity(), start, end)]
    }
}

crate::register_cop!("Style/Not", |_cfg| Some(Box::new(Not::new())));
