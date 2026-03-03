//! Layout/SpaceAfterComma - Checks for missing space after comma.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_after_comma.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

pub struct SpaceAfterComma {
    /// When true, a comma followed by `}` should still be flagged
    /// (because SpaceInsideBlockBraces expects a space before `}`)
    space_inside_braces_is_space: bool,
}

impl SpaceAfterComma {
    pub fn new() -> Self {
        // RuboCop defaults SpaceInsideHashLiteralBraces.EnforcedStyle to 'space',
        // meaning comma-before-} should be flagged by default.
        Self {
            space_inside_braces_is_space: true,
        }
    }

    pub fn with_config(space_inside_braces_is_space: bool) -> Self {
        Self {
            space_inside_braces_is_space,
        }
    }
}

impl Cop for SpaceAfterComma {
    fn name(&self) -> &'static str {
        "Layout/SpaceAfterComma"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();

        for (line_index, line) in ctx.source.lines().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let mut i = 0;
            // Track string/comment context to avoid false positives
            let mut in_single_quote = false;
            let mut in_double_quote = false;
            let mut in_comment = false;

            while i < chars.len() {
                if in_comment {
                    break;
                }

                match chars[i] {
                    '#' if !in_single_quote && !in_double_quote => {
                        in_comment = true;
                    }
                    '\'' if !in_double_quote && !in_comment => {
                        if !in_single_quote {
                            in_single_quote = true;
                        } else {
                            in_single_quote = false;
                        }
                    }
                    '"' if !in_single_quote && !in_comment => {
                        if !in_double_quote {
                            in_double_quote = true;
                        } else {
                            in_double_quote = false;
                        }
                    }
                    '\\' if (in_single_quote || in_double_quote) => {
                        i += 1; // skip escaped character
                    }
                    ',' if !in_single_quote && !in_double_quote && !in_comment => {
                        // Check the next character
                        if i + 1 < chars.len() {
                            let next = chars[i + 1];
                            // Trailing comma (followed by ), ], }, |, newline) is OK
                            // Space after comma is OK
                            // Newline after comma is OK (handled by being at end of line)
                            if next != ' '
                                && next != '\t'
                                && next != ')'
                                && next != ']'
                                && (next != '}' || self.space_inside_braces_is_space)
                                && next != '|'
                                && next != '\n'
                            {
                                let line_num = (line_index + 1) as u32;
                                offenses.push(Offense::new(
                                    self.name(),
                                    "Space missing after comma.",
                                    self.severity(),
                                    Location::new(line_num, i as u32, line_num, (i + 1) as u32),
                                    ctx.filename,
                                ));
                            }
                        }
                        // Comma at end of line is OK (trailing comma)
                    }
                    _ => {}
                }
                i += 1;
            }
        }

        offenses
    }
}
