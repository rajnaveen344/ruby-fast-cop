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
            // No final newline
            let last_line_num = effective_source.lines().count() as u32;
            let last_line = effective_source.lines().last().unwrap_or("");
            let last_line_len = last_line.chars().count() as u32;

            // Both styles report "Final newline missing" when there's no newline at all.
            // RuboCop uses iterative correction — first adds \n, then on second pass
            // would add the blank line for FinalBlankLine style.
            let message = "Final newline missing.";
            let correction = Correction::insert(source.len(), "\n");
            let loc = Location::new(last_line_num, last_line_len, last_line_num, last_line_len + 1);

            return vec![Offense::new(
                self.name(),
                message,
                self.severity(),
                loc,
                ctx.filename,
            )
            .with_correction(correction)];
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

        // trailing_newlines counts raw \n chars. Blank lines = trailing_newlines - 1
        // (one \n is the final newline, the rest are blank lines).
        let blank_lines = if trailing_newlines > 0 { trailing_newlines - 1 } else { 0 };

        match self.enforced_style {
            EnforcedStyle::FinalNewline => {
                // Should have exactly 1 trailing newline (no blank lines)
                if blank_lines > 0 {
                    let content_lines: Vec<&str> = effective_source.lines().collect();
                    let last_content_line_idx = content_lines
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .unwrap_or(0);
                    let offense_line = (last_content_line_idx + 2) as u32;

                    let stripped_len = source.trim_end().len();
                    let correction = Correction::replace(stripped_len, source.len(), "\n");

                    return vec![Offense::new(
                        self.name(),
                        &format!("{} trailing blank lines detected.", blank_lines),
                        self.severity(),
                        Location::new(offense_line, 0, offense_line, 1),
                        ctx.filename,
                    )
                    .with_correction(correction)];
                }
                vec![]
            }
            EnforcedStyle::FinalBlankLine => {
                if blank_lines == 0 {
                    // Need a blank line but only have a final newline
                    let total_lines = effective_source.lines().count();
                    let offense_line = (total_lines + 1) as u32;

                    let correction = Correction::insert(source.len(), "\n");

                    return vec![Offense::new(
                        self.name(),
                        "Trailing blank line missing.",
                        self.severity(),
                        Location::new(offense_line, 0, offense_line, 1),
                        ctx.filename,
                    )
                    .with_correction(correction)];
                } else if blank_lines > 1 {
                    // Too many blank lines (should be exactly 1 blank line = 2 newlines)
                    let content_lines: Vec<&str> = effective_source.lines().collect();
                    let last_content_line_idx = content_lines
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .unwrap_or(0);
                    let offense_line = (last_content_line_idx + 2) as u32;

                    let stripped_len = source.trim_end().len();
                    let correction = Correction::replace(stripped_len, source.len(), "\n\n");

                    return vec![Offense::new(
                        self.name(),
                        &format!(
                            "{} trailing blank lines instead of 1 detected.",
                            blank_lines
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
