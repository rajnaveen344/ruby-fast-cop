//! Layout/TrailingEmptyLines - Checks trailing blank lines at the end of a file.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/trailing_empty_lines.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    FinalNewline,
    FinalBlankLine,
}

pub struct TrailingEmptyLines {
    enforced_style: EnforcedStyle,
}

impl TrailingEmptyLines {
    pub fn new(style: EnforcedStyle) -> Self {
        Self {
            enforced_style: style,
        }
    }

    /// Find the position of __END__ in the source (0-indexed line number).
    /// Only matches __END__ at the start of a line (not indented).
    fn find_end_marker(source: &str) -> Option<usize> {
        for (i, line) in source.lines().enumerate() {
            if line == "__END__" {
                return Some(i);
            }
        }
        None
    }
}

impl Cop for TrailingEmptyLines {
    fn name(&self) -> &'static str {
        "Layout/TrailingEmptyLines"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;

        // Empty source is always accepted
        if source.is_empty() {
            return vec![];
        }

        // If __END__ marker is present, don't check trailing empty lines at all.
        // Content after __END__ is the data section, not code.
        if Self::find_end_marker(source).is_some() {
            return vec![];
        }

        let effective_source = source.to_string();

        // Check if source ends with newline
        let ends_with_newline = effective_source.ends_with('\n');

        if !ends_with_newline {
            // No final newline - offense for both styles
            let last_line_num = effective_source.lines().count() as u32;
            let last_line = effective_source.lines().last().unwrap_or("");
            let last_line_len = last_line.chars().count() as u32;

            return vec![Offense::new(
                self.name(),
                "Final newline missing.",
                self.severity(),
                Location::new(last_line_num, last_line_len, last_line_num, last_line_len + 1),
                ctx.filename,
            )
            .with_correction(Correction::insert(source.len(), "\n"))];
        }

        // Count trailing blank lines (lines that are empty after the last non-empty line)
        let mut trailing_newlines = 0u32;
        let bytes = effective_source.as_bytes();
        let len = bytes.len();

        // Count newlines from the end
        let mut pos = len;
        while pos > 0 && bytes[pos - 1] == b'\n' {
            trailing_newlines += 1;
            pos -= 1;
            // Skip any whitespace-only content on the line before this newline
            while pos > 0 && bytes[pos - 1] != b'\n' {
                let c = bytes[pos - 1];
                if c == b' ' || c == b'\t' {
                    pos -= 1;
                } else {
                    break;
                }
            }
            // If we hit another newline or start of string, continue counting
            if pos > 0 && bytes[pos - 1] != b'\n' {
                // We hit actual content - put back and stop
                break;
            }
        }

        match self.enforced_style {
            EnforcedStyle::FinalNewline => {
                // Should have exactly 1 trailing newline (no blank lines)
                if trailing_newlines > 1 {
                    // Find the line number where extra blank lines start
                    let content_lines: Vec<&str> = effective_source.lines().collect();
                    let last_content_line_idx = content_lines
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .unwrap_or(0);
                    let offense_line = (last_content_line_idx + 2) as u32; // 1-indexed, +1 for next line

                    // Correction: strip all trailing whitespace, add single \n
                    let stripped_len = source.trim_end().len();
                    let correction = Correction::replace(stripped_len, source.len(), "\n");

                    return vec![Offense::new(
                        self.name(),
                        &format!("{} trailing blank lines detected.", trailing_newlines),
                        self.severity(),
                        Location::new(offense_line, 0, offense_line, 1),
                        ctx.filename,
                    )
                    .with_correction(correction)];
                }
                vec![]
            }
            EnforcedStyle::FinalBlankLine => {
                if trailing_newlines == 1 {
                    // Need a blank line but only have a newline
                    let total_lines = effective_source.lines().count();
                    let offense_line = (total_lines + 1) as u32;

                    // Correction: insert an extra \n to create the blank line
                    let correction = Correction::insert(source.len(), "\n");

                    return vec![Offense::new(
                        self.name(),
                        "Trailing blank line missing.",
                        self.severity(),
                        Location::new(offense_line, 0, offense_line, 1),
                        ctx.filename,
                    )
                    .with_correction(correction)];
                } else if trailing_newlines > 2 {
                    // Too many blank lines (should be exactly 1 blank line = 2 newlines)
                    let content_lines: Vec<&str> = effective_source.lines().collect();
                    let last_content_line_idx = content_lines
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .unwrap_or(0);
                    let offense_line = (last_content_line_idx + 2) as u32;

                    // Correction: strip all trailing whitespace, add single \n
                    // (RuboCop corrects "too many" to \n in one pass)
                    let stripped_len = source.trim_end().len();
                    let correction = Correction::replace(stripped_len, source.len(), "\n");

                    return vec![Offense::new(
                        self.name(),
                        &format!(
                            "{} trailing blank lines instead of 1 detected.",
                            trailing_newlines
                        ),
                        self.severity(),
                        Location::new(offense_line, 0, offense_line, 1),
                        ctx.filename,
                    )
                    .with_correction(correction)];
                }
                vec![]
            }
        }
    }
}
