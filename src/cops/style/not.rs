//! Style/Not cop
//!
//! Checks for `not` keyword usage — use `!` instead.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
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
    fn requires_parens(recv: &Node, source: &str) -> bool {
        // operator keywords: and/or/not
        if recv.as_and_node().is_some() || recv.as_or_node().is_some() {
            return true;
        }
        // binary send operations
        if let Some(call) = recv.as_call_node() {
            let m = node_name!(call);
            if matches!(m.as_ref(),
                "+" | "-" | "*" | "/" | "%" | "**" | ">>" | "<<" | "&" | "|" | "^"
                | "==" | "===" | "!=" | "<=" | ">=" | "<" | ">"
                | "<=>" | "=~" | "!~"
                | "&&" | "||"
            ) && call.receiver().is_some() {
                return true;
            }
        }
        // ternary `cond ? a : b` — IfNode whose source doesn't start with `if`/`unless`
        if recv.as_if_node().is_some() {
            let s = recv.location().start_offset();
            let src_at = &source[s..];
            if !src_at.starts_with("if") && !src_at.starts_with("unless") {
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
        let node_end = node.location().end_offset();
        let recv = node.receiver().unwrap();
        let recv_start = recv.location().start_offset();
        let recv_end = recv.location().end_offset();

        // Comparison-flip rewrite: `not x < y` → `x >= y`
        let flip = recv.as_call_node().and_then(|c| {
            let m = node_name!(c);
            let opp = Self::opposite_method(m.as_ref())?;
            let r_recv = c.receiver()?;
            let args = c.arguments()?;
            let args_v: Vec<_> = args.arguments().iter().collect();
            if args_v.len() != 1 { return None; }
            let lhs_loc = r_recv.location();
            let rhs_loc = args_v[0].location();
            let lhs_src = &ctx.source[lhs_loc.start_offset()..lhs_loc.end_offset()];
            let rhs_src = &ctx.source[rhs_loc.start_offset()..rhs_loc.end_offset()];
            Some(format!("{} {} {}", lhs_src, opp, rhs_src))
        });

        let correction = if let Some(flipped) = flip {
            Correction::replace(start, node_end, flipped)
        } else if Self::requires_parens(&recv, ctx.source) {
            let recv_src = &ctx.source[recv_start..recv_end];
            Correction::replace(start, node_end, format!("!({})", recv_src))
        } else if ctx.source.as_bytes().get(start + 3) == Some(&b'(') {
            // `not(arg)` — keep parens, just swap keyword
            Correction::replace(start, start + 3, "!")
        } else {
            // simple: replace `not` + any whitespace before receiver with `!`
            Correction::replace(start, recv_start, "!")
        };

        let msg = "Use `!` instead of `not`.";
        vec![ctx
            .offense_with_range(self.name(), msg, self.severity(), start, end)
            .with_correction(correction)]
    }
}

crate::register_cop!("Style/Not", |_cfg| Some(Box::new(Not::new())));
