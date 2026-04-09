//! Layout/HeredocIndentation - Checks the indentation of heredoc bodies.
//!
//! Heredoc bodies should be indented one step using `<<~` (squiggly heredoc).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/heredoc_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;

pub struct HeredocIndentation {
    indentation_width: usize,
    active_support_extensions_enabled: bool,
    /// Max line length from Layout/LineLength (None = unlimited)
    max_line_length: Option<usize>,
    /// Whether Layout/LineLength allows heredocs (default true = no limit check)
    allow_heredoc: bool,
}

impl HeredocIndentation {
    pub fn new() -> Self {
        Self {
            indentation_width: 2,
            active_support_extensions_enabled: false,
            max_line_length: None,
            allow_heredoc: true,
        }
    }

    pub fn with_config(
        indentation_width: usize,
        active_support_extensions_enabled: bool,
        max_line_length: Option<usize>,
        allow_heredoc: bool,
    ) -> Self {
        Self {
            indentation_width,
            active_support_extensions_enabled,
            max_line_length,
            allow_heredoc,
        }
    }

    /// Compute the minimum indentation level of non-empty lines.
    fn indent_level(text: &str) -> usize {
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0)
    }

    /// Get the indentation level of the line containing the given offset.
    fn base_indent_level(source: &str, offset: usize) -> usize {
        let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
        let bytes = source.as_bytes();
        let mut i = line_start;
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        i - line_start
    }

    /// Check if `.squish` or `.squish!` follows the heredoc opening.
    fn is_squish_after(source: &str, opening_end: usize) -> bool {
        let after = &source[opening_end..];
        if after.starts_with(".squish!") {
            return true;
        }
        if after.starts_with(".squish") {
            // Make sure it's not part of a longer word
            let rest = &after[7..];
            return rest.is_empty()
                || !rest.as_bytes()[0].is_ascii_alphanumeric() && rest.as_bytes()[0] != b'_';
        }
        false
    }

    /// Check if adding indentation would make the longest line too long.
    fn line_too_long(&self, body: &str, expected_indent: usize, actual_indent: usize) -> bool {
        if self.allow_heredoc {
            return false;
        }
        let max = match self.max_line_length {
            Some(m) => m,
            None => return false,
        };
        if expected_indent <= actual_indent {
            return false;
        }
        let increase = expected_indent - actual_indent;
        let longest = body.lines().map(|l| l.len()).max().unwrap_or(0);
        longest + increase >= max
    }

    /// Build the offense message.
    fn message(&self, indent_type: Option<char>) -> String {
        match indent_type {
            Some('~') => format!(
                "Use {} spaces for indentation in a heredoc.",
                self.indentation_width
            ),
            Some(c) => format!(
                "Use {} spaces for indentation in a heredoc by using `<<~` instead of `<<{}`.",
                self.indentation_width, c
            ),
            None => format!(
                "Use {} spaces for indentation in a heredoc by using `<<~` instead of `<<`.",
                self.indentation_width
            ),
        }
    }

    /// Find the byte offset of the end of the first non-empty line in the body.
    fn first_content_line_end(source: &str, body_start: usize, body_end: usize) -> usize {
        let body = &source[body_start..body_end];
        for line in body.lines() {
            if !line.trim().is_empty() {
                let offset_in_body = line.as_ptr() as usize - body.as_ptr() as usize;
                return body_start + offset_in_body + line.len();
            }
        }
        body_end
    }

    /// Find all heredoc openings and check indentation.
    fn check_heredocs(&self, source: &str, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let heredoc_re = Regex::new(r#"<<([~-])?(['"`]?)(\w+)(['"`]?)"#).unwrap();

        for mat in heredoc_re.find_iter(source) {
            let opening_str = mat.as_str();
            let opening_start = mat.start();
            let opening_end = mat.end();

            // Validate matching quotes
            let caps = heredoc_re.captures(opening_str).unwrap();
            let open_quote = caps.get(2).map_or("", |m| m.as_str());
            let close_quote = caps.get(4).map_or("", |m| m.as_str());
            if open_quote != close_quote {
                continue;
            }
            let indent_type = caps.get(1).and_then(|m| m.as_str().chars().next());
            let delimiter = caps.get(3).unwrap().as_str();

            if !ctx.ruby_version_at_least(2, 3) {
                continue;
            }

            // Body starts on the next line after the opening line
            let body_start = match source[opening_end..].find('\n') {
                Some(pos) => opening_end + pos + 1,
                None => continue,
            };
            if body_start >= source.len() {
                continue;
            }

            // Find closing delimiter (alone on its line, possibly indented)
            let closing_re = Regex::new(
                &format!(r"(?m)^[ \t]*{}[ \t]*$", regex::escape(delimiter)),
            )
            .unwrap();
            let body_end_match = match closing_re.find(&source[body_start..]) {
                Some(m) => m,
                None => continue,
            };

            let body_end = body_start + body_end_match.start();
            let body = &source[body_start..body_end];

            // Skip empty bodies
            if body.trim().is_empty() {
                continue;
            }

            let body_indent_level = Self::indent_level(body);
            let base_indent = Self::base_indent_level(source, opening_start);
            let is_squish = self.active_support_extensions_enabled
                && Self::is_squish_after(source, opening_end);

            if indent_type == Some('~') {
                // Squiggly heredoc: check body indentation matches expected
                let expected = base_indent + self.indentation_width;
                if expected == body_indent_level {
                    continue;
                }
                if self.line_too_long(body, expected, body_indent_level) {
                    continue;
                }
            } else {
                // Non-squiggly: only flag if body at column 0 or squish used
                if body_indent_level != 0 && !is_squish {
                    continue;
                }
                let expected = base_indent + self.indentation_width;
                if self.line_too_long(body, expected, body_indent_level) {
                    continue;
                }
            }

            let msg = self.message(indent_type);
            let offense_end = Self::first_content_line_end(source, body_start, body_end);
            offenses.push(ctx.offense_with_range(
                self.name(),
                &msg,
                Severity::Convention,
                body_start,
                offense_end,
            ));
        }

        offenses
    }
}

impl Cop for HeredocIndentation {
    fn name(&self) -> &'static str {
        "Layout/HeredocIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        _node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        self.check_heredocs(ctx.source, ctx)
    }
}
