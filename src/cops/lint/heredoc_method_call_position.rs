//! Lint/HeredocMethodCallPosition - method call against heredoc placement.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/lint/heredoc_method_call_position.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

const COP: &str = "Lint/HeredocMethodCallPosition";
const MSG: &str = "Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.";

#[derive(Default)]
pub struct HeredocMethodCallPosition;

impl HeredocMethodCallPosition {
    pub fn new() -> Self { Self }
}

impl Cop for HeredocMethodCallPosition {
    fn name(&self) -> &'static str { COP }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Receiver must itself be a heredoc string node (direct, not transitive).
        let receiver = match node.receiver() { Some(r) => r, None => return vec![] };
        let heredoc = match heredoc_bounds(&receiver) { Some(h) => h, None => return vec![] };

        let dot = match node.call_operator_loc() { Some(d) => d, None => return vec![] };
        let dot_line = ctx.line_of(dot.start_offset());
        let opener_line = ctx.line_of(heredoc.opener_start);
        if dot_line == opener_line {
            return vec![]; // call on opener line is fine
        }

        // Build correction: insert "<dot>...<message>(<args>)" right after opener line's heredoc tag,
        // and delete the original chained method call (from dot start through end of selector
        // and any trailing comma whitespace handling). For simplicity match RuboCop's autocorrect
        // shape: cut the call piece, paste it after opener.

        let dot_start = dot.start_offset();
        let dot_end = dot.end_offset();

        let call_end = node.location().end_offset();
        let mut move_end = end_of_call_selector_chain(node, ctx);
        // Extend through any chained calls on the same line.
        let bytes = ctx.source.as_bytes();
        loop {
            // Skip whitespace
            let mut i = move_end;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
            // Same line check: no newline before the next dot
            let line_at_i = ctx.line_of(i.saturating_sub(0));
            if line_at_i != dot_line { break; }
            // Check for `.method` or `&.method`
            if i < bytes.len() && bytes[i] == b'.' {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'?' || bytes[j] == b'!') { j += 1; }
                // Possibly arguments `(...)` — match parens balance.
                if j < bytes.len() && bytes[j] == b'(' {
                    let mut depth = 1usize;
                    j += 1;
                    while j < bytes.len() && depth > 0 {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                }
                if ctx.line_of(j.saturating_sub(1)) != dot_line { break; }
                move_end = j;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i] == b'&' && bytes[i+1] == b'.' {
                // similar but skip the &.
                let mut j = i + 2;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'?' || bytes[j] == b'!') { j += 1; }
                if j < bytes.len() && bytes[j] == b'(' {
                    let mut depth = 1usize;
                    j += 1;
                    while j < bytes.len() && depth > 0 {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                }
                if ctx.line_of(j.saturating_sub(1)) != dot_line { break; }
                move_end = j;
                continue;
            }
            break;
        }

        let inserted = ctx.source[dot_start..move_end].to_string();

        // Insert immediately after heredoc opener line content (right after the opener tag like `<<-SQL`).
        // RuboCop inserts after the opener identifier itself.
        let insert_at = heredoc.opener_end;

        // Delete leading newline before the dot if present (so we don't leave a blank line).
        let bytes = ctx.source.as_bytes();
        let mut delete_start = dot_start;
        if delete_start > 0 && bytes[delete_start - 1] == b'\n' {
            delete_start -= 1;
        }
        // Trailing comma handling: if char after move_end is `,`, include the newline before dot
        // (already done) and keep the comma in the moved piece by extending move_end.
        let mut effective_move_end = move_end;
        if effective_move_end < bytes.len() && bytes[effective_move_end] == b',' {
            effective_move_end += 1;
        }
        let inserted2 = ctx.source[dot_start..effective_move_end].to_string();

        let correction = Correction {
            edits: vec![
                crate::offense::Edit {
                    start_offset: insert_at,
                    end_offset: insert_at,
                    replacement: inserted2.clone(),
                },
                crate::offense::Edit {
                    start_offset: delete_start,
                    end_offset: effective_move_end,
                    replacement: String::new(),
                },
            ],
        };
        let _ = inserted;
        let _ = call_end;
        let _ = dot_end;

        // Offense range: just the dot character (col 0 width 1 on its line).
        let mut off = ctx.offense_with_range(COP, MSG, Severity::Warning, dot_start, dot_start + 1);
        off.correction = Some(correction);
        vec![off]
    }
}

struct HeredocBounds {
    opener_start: usize,
    opener_end: usize,
}

fn heredoc_bounds(node: &ruby_prism::Node) -> Option<HeredocBounds> {
    if let Some(s) = node.as_string_node() {
        let opener = s.opening_loc()?;
        if !is_heredoc_opener(opener.as_slice()) { return None; }
        return Some(HeredocBounds {
            opener_start: opener.start_offset(),
            opener_end: opener.end_offset(),
        });
    }
    if let Some(s) = node.as_interpolated_string_node() {
        let opener = s.opening_loc()?;
        if !is_heredoc_opener(opener.as_slice()) { return None; }
        return Some(HeredocBounds {
            opener_start: opener.start_offset(),
            opener_end: opener.end_offset(),
        });
    }
    if let Some(s) = node.as_x_string_node() {
        let opener = s.opening_loc();
        if !is_heredoc_opener(opener.as_slice()) { return None; }
        return Some(HeredocBounds {
            opener_start: opener.start_offset(),
            opener_end: opener.end_offset(),
        });
    }
    if let Some(s) = node.as_interpolated_x_string_node() {
        let opener = s.opening_loc();
        if !is_heredoc_opener(opener.as_slice()) { return None; }
        return Some(HeredocBounds {
            opener_start: opener.start_offset(),
            opener_end: opener.end_offset(),
        });
    }
    None
}

fn is_heredoc_opener(opener: &[u8]) -> bool {
    opener.starts_with(b"<<")
}

/// End offset of "this call's selector + arguments", excluding trailing chained calls.
fn end_of_call_selector_chain(node: &ruby_prism::CallNode, _ctx: &CheckContext) -> usize {
    let msg_end = node.message_loc().map(|l| l.end_offset()).unwrap_or(0);
    let args_end = node.arguments().map(|a| a.location().end_offset()).unwrap_or(0);
    let closing_end = node.closing_loc().map(|l| l.end_offset()).unwrap_or(0);
    msg_end.max(args_end).max(closing_end)
}

crate::register_cop!("Lint/HeredocMethodCallPosition", |_cfg| Some(Box::new(
    HeredocMethodCallPosition::new()
)));
