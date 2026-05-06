//! Style/MultilineTernaryOperator cop
//!
//! Checks for multi-line ternary operator expressions.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
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

        let correction = build_correction(node, ctx.source, enforce_single);

        let mut off = ctx.offense_with_range(self.name(), msg, self.severity(), start, end);
        if let Some(c) = correction {
            off = off.with_correction(c);
        }
        vec![off]
    }
}

/// Build correction for a multiline ternary operator.
fn build_correction(node: &IfNode, source: &str, enforce_single: bool) -> Option<Correction> {
    // Get source text for predicate, then-branch, else-branch
    let pred = node.predicate();
    let pred_src = source[pred.location().start_offset()..pred.location().end_offset()].trim().to_string();

    // Then branch: node.statements() for ternary
    let then_src = if let Some(stmts) = node.statements() {
        source[stmts.location().start_offset()..stmts.location().end_offset()].trim().to_string()
    } else {
        return None;
    };

    // Else branch: node.subsequent() -> ElseNode -> statements
    let else_src = if let Some(subseq) = node.subsequent() {
        if let Some(else_node) = subseq.as_else_node() {
            if let Some(stmts) = else_node.statements() {
                source[stmts.location().start_offset()..stmts.location().end_offset()].trim().to_string()
            } else {
                return None;
            }
        } else {
            // Could be another IfNode (elsif) — get its source
            source[subseq.location().start_offset()..subseq.location().end_offset()].trim().to_string()
        }
    } else {
        return None;
    };

    let node_start = node.location().start_offset();
    let node_end = node.location().end_offset();

    // Collect comments from condition area (between `?` keyword and then-branch start)
    // and also from any position between pred end and then-branch start
    let condition_comments = collect_condition_comments(node, source);

    if enforce_single {
        // Collapse to single line: "cond ? then : else"
        let replacement = format!("{} ? {} : {}", pred_src, then_src, else_src);
        let mut edits = vec![Edit { start_offset: node_start, end_offset: node_end, replacement }];
        // If there were condition comments, insert them before the parent line
        if !condition_comments.is_empty() {
            let parent_line_start = find_parent_line_start(node_start, source);
            let insert = format!("{}\n", condition_comments.join("\n"));
            edits.push(Edit { start_offset: parent_line_start, end_offset: parent_line_start, replacement: insert });
        }
        Some(Correction { edits })
    } else {
        // Convert to if/else form
        let replacement = format!("if {}\n  {}\nelse\n  {}\nend", pred_src, then_src, else_src);
        let mut edits = vec![Edit { start_offset: node_start, end_offset: node_end, replacement }];
        // Move condition comments before the parent statement's line
        if !condition_comments.is_empty() {
            let parent_line_start = find_parent_line_start(node_start, source);
            let insert = format!("{}\n", condition_comments.join("\n"));
            edits.push(Edit { start_offset: parent_line_start, end_offset: parent_line_start, replacement: insert });
        }
        Some(Correction { edits })
    }
}

/// Find the start of the line that contains `offset`.
fn find_parent_line_start(node_start: usize, source: &str) -> usize {
    // node_start is where the ternary begins; find the line start of the parent statement
    // which is the line that contains the assignment/call before the ternary.
    // We look back from node_start for the line start.
    source[..node_start].rfind('\n').map(|p| p + 1).unwrap_or(0)
}

/// Collect comments that are in the "condition area" — between pred end and then-branch start,
/// OR between the `:` colon and the else-branch start (for ternaries with comments after `:`)
/// Returns vec of comment text strings.
fn collect_condition_comments(node: &IfNode, source: &str) -> Vec<String> {
    let mut comments = Vec::new();

    let pred_end = node.predicate().location().end_offset();
    let then_start = if let Some(stmts) = node.statements() {
        stmts.location().start_offset()
    } else {
        return vec![];
    };

    // Comments between predicate and then-branch
    if pred_end < then_start {
        collect_region_comments(source, pred_end, then_start, &mut comments);
    }

    // Comments between `:` and else-branch
    if let Some(subseq) = node.subsequent() {
        if let Some(else_node) = subseq.as_else_node() {
            let else_node_start = else_node.location().start_offset();
            let else_stmts_start = if let Some(stmts) = else_node.statements() {
                stmts.location().start_offset()
            } else {
                return comments;
            };
            if else_node_start < else_stmts_start {
                collect_region_comments(source, else_node_start, else_stmts_start, &mut comments);
            }
        }
    }

    comments
}

fn collect_region_comments(source: &str, from: usize, to: usize, out: &mut Vec<String>) {
    let region = &source[from..to];
    for line in region.split('\n') {
        if let Some(hash_pos) = find_comment_hash(line) {
            let comment_text = line[hash_pos..].trim().to_string();
            if !comment_text.is_empty() {
                out.push(comment_text);
            }
        }
    }
}

/// Find position of `#` comment start in a line, skipping string literals.
fn find_comment_hash(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        let indent = line.len() - trimmed.len();
        return Some(indent);
    }
    // Look for ` #` or similar after code
    if let Some(pos) = line.find(" #") {
        return Some(pos + 1);
    }
    None
}

crate::register_cop!("Style/MultilineTernaryOperator", |_cfg| {
    Some(Box::new(MultilineTernaryOperator::new()))
});
