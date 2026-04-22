//! Layout/ClosingHeredocIndentation - Checks indentation of closing heredoc delimiters.
//!
//! The closing delimiter must align with the start of the heredoc opening (`<<-TAG` or `<<~TAG`).
//! Plain `<<TAG` heredocs are exempt (must be at column 0).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/closing_heredoc_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use regex::Regex;

pub struct ClosingHeredocIndentation;

impl Default for ClosingHeredocIndentation {
    fn default() -> Self {
        Self
    }
}

impl ClosingHeredocIndentation {
    /// Compute indentation (spaces) of the line at `offset`.
    fn indent_at(source: &str, offset: usize) -> usize {
        let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
        let bytes = source.as_bytes();
        let mut i = line_start;
        while i < source.len() && bytes[i] == b' ' {
            i += 1;
        }
        i - line_start
    }
}

impl Cop for ClosingHeredocIndentation {
    fn name(&self) -> &'static str {
        "Layout/ClosingHeredocIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;

        // Match heredoc openings: <<-TAG or <<~TAG (not plain <<TAG)
        let heredoc_re = Regex::new(r#"<<([~-])(['"`]?)([A-Za-z_]\w*)(['"`]?)"#).unwrap();

        let mut search_start = 0;
        while search_start < source.len() {
            let caps = match heredoc_re.captures(&source[search_start..]) {
                Some(c) => c,
                None => break,
            };

            let mat = caps.get(0).unwrap();
            let opening_start = search_start + mat.start();
            let opening_end = search_start + mat.end();

            // Validate matching quotes
            let open_q = caps.get(2).map_or("", |m| m.as_str());
            let close_q = caps.get(4).map_or("", |m| m.as_str());
            if open_q != close_q {
                search_start = opening_end;
                continue;
            }

            let delimiter = caps.get(3).unwrap().as_str();

            // Compute expected indent: indent of the line containing the opening
            let expected_indent = Self::indent_at(source, opening_start);

            // Body starts after the opening line's newline
            let body_start = match source[opening_end..].find('\n') {
                Some(pos) => opening_end + pos + 1,
                None => {
                    search_start = opening_end;
                    continue;
                }
            };

            // Find closing delimiter line: must be on its own line, possibly indented
            let closing_re = Regex::new(
                &format!(r"(?m)^([ \t]*)({})[ \t]*$", regex::escape(delimiter))
            ).unwrap();

            let search_body = &source[body_start..];
            let closing_caps = match closing_re.captures(search_body) {
                Some(c) => c,
                None => {
                    search_start = opening_end;
                    continue;
                }
            };

            let closing_mat = closing_caps.get(0).unwrap();
            let closing_abs_start = body_start + closing_mat.start();
            let closing_abs_end = body_start + closing_mat.end();
            let closing_indent_str = closing_caps.get(1).unwrap().as_str();
            let closing_indent = closing_indent_str.len();
            let closing_delim_abs = closing_abs_start + closing_indent;

            // Also compute: column of start of the statement containing the heredoc.
            // If the heredoc is an argument (code before << on same line),
            // the closing may be aligned with the outermost statement start.
            let line_containing_opening_start = source[..opening_start].rfind('\n').map_or(0, |p| p + 1);
            let code_before_heredoc = source[line_containing_opening_start..opening_start].trim();
            // is_argument: heredoc is an arg if there's code before it on the same line,
            // OR if the previous line ends with a continuation char (comma/open-paren/backslash)
            // meaning the heredoc is on a continuation line of a method call.
            let prev_line_is_continuation = if line_containing_opening_start > 0 {
                let prev_line_end = line_containing_opening_start - 1;
                let prev_line_start = source[..prev_line_end].rfind('\n').map_or(0, |p| p + 1);
                let prev_line = source[prev_line_start..prev_line_end].trim_end();
                prev_line.ends_with(',') || prev_line.ends_with('(') || prev_line.ends_with('\\')
            } else {
                false
            };
            let is_argument = !code_before_heredoc.is_empty() || prev_line_is_continuation;

            // Find the column of the statement start (leftmost indent on the first line of the call)
            let stmt_indent = if is_argument {
                // Scan backwards through previous lines to find start of expression
                // Use the indent of the first line with less indentation than this line
                // (i.e., the method call start). Simpler: use indent of the line that begins
                // the outermost expression by checking continuation lines.
                // For now: use the minimum indent found in lines from the start of the method
                // up to the opening heredoc line.
                let mut min_indent = expected_indent;
                let mut scan = line_containing_opening_start;
                // Walk backwards finding lines that are part of the same expression
                // Stop when we find a line that's not a continuation (no comma/operator at end)
                loop {
                    let indent = Self::indent_at(source, scan);
                    if indent < min_indent {
                        min_indent = indent;
                    }
                    // Check if previous line ends with comma (continuation)
                    if scan == 0 { break; }
                    let prev_line_end = scan - 1; // the '\n'
                    let prev_line_start = source[..prev_line_end].rfind('\n').map_or(0, |p| p + 1);
                    let prev_line = source[prev_line_start..prev_line_end].trim_end();
                    let prev_line_trimmed = prev_line.trim_end_matches(|c: char| c.is_whitespace());
                    if prev_line_trimmed.ends_with(',') || prev_line_trimmed.ends_with('(') || prev_line_trimmed.ends_with('\\') {
                        scan = prev_line_start;
                    } else {
                        break;
                    }
                }
                min_indent
            } else {
                expected_indent
            };

            // Offense: closing doesn't match expected, AND (if argument) doesn't match stmt start
            let closing_ok = closing_indent == expected_indent
                || (is_argument && closing_indent == stmt_indent);
            if !closing_ok {
                let msg = format!("`{}` is not aligned with `<<-{}`.", delimiter, delimiter)
                    .replace("<<-", if source[opening_start..opening_end].contains("<<~") { "<<~" } else { "<<-" });
                // Rebuild with correct prefix
                let op_indicator = &source[opening_start..opening_start+3];
                let msg = format!("`{}` is not aligned with `{}{}`.", delimiter,
                    if op_indicator.starts_with("<<~") { "<<~" } else { "<<-" },
                    delimiter);

                // Correction: replace leading whitespace of closing line
                let new_indent = " ".repeat(expected_indent);
                let correction = Correction {
                    edits: vec![Edit {
                        start_offset: closing_abs_start,
                        end_offset: closing_delim_abs,
                        replacement: new_indent,
                    }],
                };

                offenses.push(
                    Offense::new(
                        self.name(),
                        &msg,
                        Severity::Convention,
                        Location::from_offsets(source, closing_abs_start, closing_abs_end),
                        ctx.filename,
                    ).with_correction(correction)
                );
            }

            // Advance past the closing delimiter for next heredoc search
            search_start = closing_abs_end;
        }

        offenses
    }
}

crate::register_cop!("Layout/ClosingHeredocIndentation", |_cfg| {
    Some(Box::new(ClosingHeredocIndentation))
});
