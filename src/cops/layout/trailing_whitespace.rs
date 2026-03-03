//! Layout/TrailingWhitespace - Checks for trailing whitespace in the source code.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/trailing_whitespace.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use regex::Regex;
use std::collections::VecDeque;

pub struct TrailingWhitespace {
    allow_in_heredoc: bool,
}

impl TrailingWhitespace {
    pub fn new() -> Self {
        Self {
            allow_in_heredoc: false,
        }
    }

    pub fn with_config(allow_in_heredoc: bool) -> Self {
        Self { allow_in_heredoc }
    }

    /// Check if a character is trailing whitespace (space, tab, or fullwidth space U+3000)
    fn is_trailing_ws(c: char) -> bool {
        c == ' ' || c == '\t' || c == '\u{3000}'
    }

    /// Find the start position of trailing whitespace in a line.
    /// Returns None if there's no trailing whitespace.
    fn trailing_ws_start(line: &str) -> Option<usize> {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            return None;
        }

        // Find the rightmost non-whitespace character
        let mut end = chars.len();
        while end > 0 && Self::is_trailing_ws(chars[end - 1]) {
            end -= 1;
        }

        if end < chars.len() {
            Some(end)
        } else {
            None
        }
    }

    /// Detect heredoc body line ranges.
    /// Returns a set of 0-indexed line numbers that are heredoc body lines.
    fn find_heredoc_body_lines(source: &str) -> Vec<bool> {
        let lines: Vec<&str> = source.lines().collect();
        let mut is_heredoc_body = vec![false; lines.len()];
        let heredoc_re = Regex::new(r#"<<[-~]?['"]?(\w+)['"]?"#).unwrap();
        let mut queue: VecDeque<String> = VecDeque::new();

        for (i, line) in lines.iter().enumerate() {
            if let Some(current_delim) = queue.front().cloned() {
                let trimmed = line.trim();
                if trimmed == current_delim {
                    // Closing delimiter line - not a body line
                    queue.pop_front();
                } else {
                    // Body line
                    is_heredoc_body[i] = true;

                    // Check for nested heredoc openings
                    for cap in heredoc_re.captures_iter(line) {
                        queue.push_back(cap[1].to_string());
                    }
                }
            } else {
                // Not inside heredoc - check for openings
                for cap in heredoc_re.captures_iter(line) {
                    queue.push_back(cap[1].to_string());
                }
            }
        }

        is_heredoc_body
    }
}

impl Cop for TrailingWhitespace {
    fn name(&self) -> &'static str {
        "Layout/TrailingWhitespace"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let mut past_end = false;
        let mut in_doc_comment = false;
        let mut byte_offset: usize = 0;

        // Pre-compute heredoc body lines
        let heredoc_body_lines = Self::find_heredoc_body_lines(ctx.source);

        for (line_index, line) in ctx.source.lines().enumerate() {
            let line_byte_offset = byte_offset;
            // Advance byte_offset past this line (line.len() for content + 1 for '\n')
            byte_offset += line.len();
            if byte_offset < ctx.source.len() {
                byte_offset += 1; // skip the '\n'
            }

            let is_in_heredoc = heredoc_body_lines
                .get(line_index)
                .copied()
                .unwrap_or(false);

            // Track =begin/=end doc comments (only at column 0, not inside heredocs)
            if !is_in_heredoc {
                if !in_doc_comment && line.starts_with("=begin") {
                    in_doc_comment = true;
                    continue;
                }
                if in_doc_comment && line.starts_with("=end") {
                    in_doc_comment = false;
                    continue;
                }
            }

            // Skip lines inside =begin/=end when not inside a heredoc
            if in_doc_comment && !is_in_heredoc {
                continue;
            }

            // Check for __END__ (only at top level, not in heredoc or doc comment)
            if !is_in_heredoc && !in_doc_comment && line == "__END__" {
                past_end = true;
                continue;
            }
            if past_end {
                continue;
            }

            // Skip heredoc body lines if AllowInHeredoc is true
            if self.allow_in_heredoc && is_in_heredoc {
                continue;
            }

            // Check for trailing whitespace
            if let Some(ws_start) = Self::trailing_ws_start(line) {
                let line_char_len = line.chars().count();
                let line_num = (line_index + 1) as u32;

                let mut offense = Offense::new(
                    self.name(),
                    "Trailing whitespace detected.",
                    self.severity(),
                    Location::new(line_num, ws_start as u32, line_num, line_char_len as u32),
                    ctx.filename,
                );

                // Add correction for non-heredoc lines (heredoc corrections are complex)
                if !is_in_heredoc {
                    // Find byte position of ws_start within the line
                    let ws_byte_start = line
                        .char_indices()
                        .nth(ws_start)
                        .map(|(pos, _)| pos)
                        .unwrap_or(line.len());
                    offense = offense.with_correction(Correction::delete(
                        line_byte_offset + ws_byte_start,
                        line_byte_offset + line.len(),
                    ));
                }

                offenses.push(offense);
            }
        }

        offenses
    }
}
