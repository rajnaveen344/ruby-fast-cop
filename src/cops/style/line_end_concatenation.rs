//! Style/LineEndConcatenation cop
//!
//! Checks for string literal concatenation at the end of a line using + or <<.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str = "Use `\\` instead of `%op%` to concatenate multiline strings.";

#[derive(Default)]
pub struct LineEndConcatenation;

impl LineEndConcatenation {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for LineEndConcatenation {
    fn name(&self) -> &'static str {
        "Style/LineEndConcatenation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        check_source(ctx)
    }
}

fn check_source(ctx: &CheckContext) -> Vec<Offense> {
    let source = ctx.source;
    let lines: Vec<&str> = source.split('\n').collect();
    let mut offenses = Vec::new();

    // Compute byte offset of each line start
    let mut line_offsets = Vec::with_capacity(lines.len());
    let mut off = 0usize;
    for line in &lines {
        line_offsets.push(off);
        off += line.len() + 1; // +1 for '\n'
    }

    for i in 0..lines.len() {
        let line = lines[i];
        // Find the effective end of content (before trailing whitespace)
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        // Check if line ends with + or << (after trimming), optionally followed by backslash
        // Handle: `"a" + \` (backslash continuation after operator)
        let effective = if trimmed.ends_with('\\') {
            trimmed[..trimmed.len() - 1].trim_end()
        } else {
            trimmed
        };
        let (op_str, op_col_end, op_col_start) = if effective.ends_with("<<") {
            let start = effective.len() - 2;
            ("<<", start + 2, start)
        } else if effective.ends_with('+') {
            let start = effective.len() - 1;
            ("+", start + 1, start)
        } else {
            continue;
        };

        // Before the operator: must end with a string literal closing quote or `\`
        let before_op = trimmed[..op_col_start].trim_end();
        if before_op.is_empty() {
            continue;
        }
        let last_char = before_op.chars().last().unwrap();
        // Must end with ' or " (string literal close), or be after a string ending with backslash-continuation
        if last_char != '\'' && last_char != '"' {
            continue;
        }

        // Check for comment after operator (on same line)
        // Use the original trimmed line content after the operator position
        let after_op = &trimmed[op_col_end..].trim_start();
        if after_op.starts_with('#') {
            continue; // comment after operator → skip
        }
        // After the operator, only allow: whitespace, optional \, nothing else
        let after_stripped = after_op.trim();
        if !after_stripped.is_empty() && after_stripped != "\\" {
            continue; // Something other than backslash after operator
        }

        // Find next non-empty, non-comment line
        let mut next_line_idx = None;
        for j in (i + 1)..lines.len() {
            let next = lines[j].trim_start();
            if next.is_empty() {
                continue;
            }
            if next.starts_with('#') {
                // Comment line — skip it (don't flag if the next content line has a comment before it)
                // Actually: RuboCop accepts if next non-blank line is a comment line
                next_line_idx = None;
                break;
            }
            next_line_idx = Some(j);
            break;
        }

        let next_j = match next_line_idx {
            Some(j) => j,
            None => continue,
        };

        let next_trimmed = lines[next_j].trim_start();
        // Next line must start with a string literal (' or ")
        let first_char = match next_trimmed.chars().next() {
            Some(c) => c,
            None => continue,
        };
        if first_char != '\'' && first_char != '"' {
            continue;
        }

        // Also check: next line string must not be followed by ., [, *, %
        // (indicating a method call on the string, not concatenation)
        if is_followed_by_high_precedence(next_trimmed) {
            continue;
        }

        // Report offense at op position on line i
        let line_start = line_offsets[i];
        let op_start_offset = line_start + op_col_start;
        let op_end_offset = line_start + op_col_end;

        let msg = MSG.replace("%op%", op_str);
        offenses.push(ctx.offense_with_range(
            "Style/LineEndConcatenation",
            &msg,
            crate::offense::Severity::Convention,
            op_start_offset,
            op_end_offset,
        ));
    }

    offenses
}

/// Check if a string literal on the next line is followed by a high-precedence operator
/// like ., [, *, % (which would mean it's not a simple concatenation target).
fn is_followed_by_high_precedence(line: &str) -> bool {
    // Skip leading whitespace (already done by caller via trim_start)
    let rest = line.trim_start();
    if rest.is_empty() { return false; }

    let quote = rest.chars().next().unwrap();
    if quote != '\'' && quote != '"' { return false; }

    // Find end of the string literal
    let str_end = find_string_end(rest, 0);
    let after = &rest[str_end..].trim_start();
    if after.is_empty() { return false; }

    let next = after.chars().next().unwrap();
    matches!(next, '.' | '[' | '*' | '%')
}

fn find_string_end(s: &str, start: usize) -> usize {
    let bytes = s.as_bytes();
    if start >= bytes.len() { return start; }
    let quote = bytes[start];
    if quote != b'\'' && quote != b'"' { return start; }
    let mut pos = start + 1;
    while pos < bytes.len() {
        if bytes[pos] == b'\\' {
            pos += 2;
            continue;
        }
        if bytes[pos] == b'#' && quote == b'"' && pos + 1 < bytes.len() && bytes[pos + 1] == b'{' {
            // Skip interpolation
            let mut depth = 1usize;
            pos += 2;
            while pos < bytes.len() && depth > 0 {
                if bytes[pos] == b'{' { depth += 1; }
                else if bytes[pos] == b'}' { depth -= 1; }
                pos += 1;
            }
            continue;
        }
        if bytes[pos] == quote {
            return pos + 1;
        }
        pos += 1;
    }
    pos
}

crate::register_cop!("Style/LineEndConcatenation", |_cfg| {
    Some(Box::new(LineEndConcatenation::new()))
});
