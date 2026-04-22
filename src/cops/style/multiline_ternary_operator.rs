//! Style/MultilineTernaryOperator cop
//!
//! Checks for multi-line ternary operator expressions.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::IfNode;

const MSG_IF: &str = "Avoid multi-line ternary operators, use `if` or `unless` instead.";
const MSG_SINGLE_LINE: &str = "Avoid multi-line ternary operators, use single-line instead.";

#[derive(Default)]
pub struct MultilineTernaryOperator;

impl MultilineTernaryOperator {
    pub fn new() -> Self {
        Self
    }

    fn is_multiline(node: &IfNode, source: &str) -> bool {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        if !source[start..end].contains('\n') {
            return false;
        }
        // Exclude: method-chain multiline condition where `?` and branches are all on one line
        // e.g., `arg\n.foo ? bar : baz` — the newline is only in the condition (before `?`)
        // Check: if the predicate ends before the `?`'s line, AND both branches are on
        // the same line as `?`, skip (this is the only exclusion RuboCop makes via source!=replacement)
        let q_loc = match node.then_keyword_loc() {
            Some(l) => l,
            None => return false,
        };
        let q_line_start = source[..q_loc.start_offset()].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let q_line_end = source[q_loc.end_offset()..].find('\n')
            .map(|p| q_loc.end_offset() + p)
            .unwrap_or(source.len());

        // predicate starts before the q_line (condition is on an earlier line than ?)
        let pred_start = node.predicate().location().start_offset();
        let pred_end = node.predicate().location().end_offset();
        let pred_starts_before_q_line = pred_start < q_line_start;

        // predicate ends on q_line (the last part of condition is on same line as ?)
        let pred_ends_on_q_line = pred_end >= q_line_start && pred_end <= q_line_end;

        let then_on_q_line = node.statements()
            .map(|s| {
                let ts = s.location().start_offset();
                let te = s.location().end_offset();
                ts >= q_line_start && te <= q_line_end
            })
            .unwrap_or(false);

        let else_on_q_line = node.subsequent()
            .map(|e| {
                if let Some(else_node) = e.as_else_node() {
                    if let Some(stmts) = else_node.statements() {
                        let es = stmts.location().start_offset();
                        return es >= q_line_start && es <= q_line_end;
                    }
                }
                let ee = e.location().end_offset();
                ee <= q_line_end
            })
            .unwrap_or(false);

        // Exclude ONLY when: the newline is purely in the predicate (method chain),
        // the predicate's last token connects to `?` on same line,
        // and both branches are on that same line.
        // This matches `arg\n.foo ? bar : baz` but NOT `b ==\n    c ? d : e`
        // For `b ==\n    c ? d : e`: pred_starts_before_q_line=true, pred_ends_on_q_line=true
        // (c is on q_line), then/else on q_line → would exclude but shouldn't.
        // For `arg\n.foo ? bar : baz`: same pattern — can't distinguish structurally.
        //
        // RuboCop distinguishes via: `node.source != replacement(node)`.
        // For `arg\n.foo ? bar : baz`, replacement = `arg\n.foo ? bar : baz` (same) → no offense
        // For `b ==\n    c ? d : e`, replacement = `if b ==\n    c\n  d\nelse\n  e\nend` ≠ source
        //
        // We can approximate: if the predicate has a newline but the `? branch : branch` part
        // is entirely on one line, AND the predicate's multiline is due to a chained call
        // (the line break is right before a `.` or `&.`), then skip.
        if pred_starts_before_q_line && pred_ends_on_q_line && then_on_q_line && else_on_q_line {
            // Check if the newline in the predicate is a method-chain break
            // i.e., the text after the newline starts with `.` or `&.` (with possible indent)
            let nl_in_pred = source[pred_start..pred_end].find('\n');
            if let Some(nl_pos) = nl_in_pred {
                let after_nl = source[pred_start + nl_pos + 1..pred_end].trim_start();
                if after_nl.starts_with('.') || after_nl.starts_with("&.") {
                    return false;
                }
            }
        }

        true
    }

    fn is_ternary(node: &IfNode) -> bool {
        // Ternary has no `if` keyword loc
        node.if_keyword_loc().is_none()
    }

    /// Detect if parent context forces single-line (return/break/next/method call)
    fn enforce_single_line(node: &IfNode, source: &str) -> bool {
        // Look at what comes before the ternary condition on its line
        let pred = node.predicate();
        let start = pred.location().start_offset();
        // Find start of line containing `start`
        let line_start = source[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let prefix = source[line_start..start].trim();
        // If prefix is return/break/next or a non-assignment method call
        prefix == "return"
            || prefix == "break"
            || prefix == "next"
            || (!prefix.is_empty() && !prefix.ends_with('=') && !prefix.ends_with('[') && !is_assignment_context(prefix))
    }
}

fn is_assignment_context(prefix: &str) -> bool {
    prefix.ends_with('=') || prefix.ends_with(',')
}

impl Cop for MultilineTernaryOperator {
    fn name(&self) -> &'static str {
        "Style/MultilineTernaryOperator"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &IfNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_ternary(node) {
            return vec![];
        }
        if !Self::is_multiline(node, ctx.source) {
            return vec![];
        }

        let enforce_single = Self::enforce_single_line(node, ctx.source);
        let msg = if enforce_single { MSG_SINGLE_LINE } else { MSG_IF };

        // Offense range: predicate start to the end of the first "line" of the ternary
        // i.e., from condition start to just before the first newline in the ternary
        let pred = node.predicate();
        let start = pred.location().start_offset();
        let node_start = node.location().start_offset();
        let node_src = &ctx.source[node_start..node.location().end_offset()];
        let end = if let Some(nl_pos) = node_src.find('\n') {
            // End is the position of the newline (exclusive, so the char before \n)
            node_start + nl_pos
        } else {
            node.location().end_offset()
        };

        vec![ctx.offense_with_range(self.name(), msg, self.severity(), start, end)]
    }
}

crate::register_cop!("Style/MultilineTernaryOperator", |_cfg| {
    Some(Box::new(MultilineTernaryOperator::new()))
});
