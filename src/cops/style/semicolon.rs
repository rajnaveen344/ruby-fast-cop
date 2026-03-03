//! Style/Semicolon - Checks for use of semicolons instead of newlines.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/semicolon.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

pub struct Semicolon {
    allow_as_expression_separator: bool,
}

impl Semicolon {
    pub fn new(allow_as_expression_separator: bool) -> Self {
        Self {
            allow_as_expression_separator,
        }
    }

    /// Get positions of semicolons in a line that are not inside strings/comments,
    /// but ARE detected inside string interpolation.
    fn find_semicolons(line: &str) -> Vec<usize> {
        let chars: Vec<char> = line.chars().collect();
        let mut positions = Vec::new();
        let mut i = 0;
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut interpolation_depth = 0;
        let mut in_comment = false;

        while i < chars.len() {
            if in_comment {
                break;
            }

            match chars[i] {
                '#' if !in_single_quote && !in_double_quote && interpolation_depth == 0 => {
                    in_comment = true;
                }
                '#' if in_double_quote && i + 1 < chars.len() && chars[i + 1] == '{' => {
                    interpolation_depth += 1;
                    i += 1; // skip {
                }
                '{' if interpolation_depth > 0 => {
                    interpolation_depth += 1;
                }
                '}' if interpolation_depth > 0 => {
                    interpolation_depth -= 1;
                }
                '\'' if !in_double_quote && interpolation_depth == 0 => {
                    in_single_quote = !in_single_quote;
                }
                '"' if !in_single_quote && interpolation_depth == 0 => {
                    in_double_quote = !in_double_quote;
                }
                '\\' if in_single_quote || in_double_quote => {
                    i += 1; // skip escaped char
                }
                ';' if (!in_single_quote && !in_double_quote && !in_comment)
                    || interpolation_depth > 0 =>
                {
                    positions.push(i);
                }
                _ => {}
            }
            i += 1;
        }

        positions
    }

    /// Check if a line contains a one-line def/class/module definition.
    /// Returns the number of semicolons that are part of the one-liner structure
    /// (which should be allowed).
    /// Returns 0 if not a one-liner or if the one-liner has too many statements.
    fn allowed_semicolons_in_line(line: &str, semi_positions: &[usize]) -> Vec<usize> {
        let trimmed = line.trim();

        // Strip trailing semicolons to check the base pattern
        let base = trimmed.trim_end_matches(';').trim_end();

        let is_def_oneliner = base.starts_with("def ") && (base.ends_with(" end") || base.ends_with(";end"));
        let is_class_oneliner = (base.starts_with("class ") || base.starts_with("module "))
            && (base.ends_with(" end") || base.ends_with(";end"));

        if !is_def_oneliner && !is_class_oneliner {
            return vec![];
        }

        // For class/module one-liners: `class Foo; end` or `module Foo; end`
        // Allow the single semicolon between name and `end`
        if is_class_oneliner {
            // Count semicolons in the base (without trailing `;`)
            let base_semis: Vec<usize> = semi_positions
                .iter()
                .copied()
                .filter(|&pos| {
                    // Check if this semicolon is within the base text (not trailing)
                    let leading_spaces = line.len() - line.trim_start().len();
                    let base_end_in_line = leading_spaces + base.len();
                    let byte_pos = line.char_indices().nth(pos).map(|(p, _)| p).unwrap_or(0);
                    byte_pos < base_end_in_line
                })
                .collect();

            // One-liner class/module should have exactly 1 semicolon: `class Foo; end`
            if base_semis.len() == 1 {
                return base_semis;
            }
            // Too many semicolons in the body - none allowed
            return vec![];
        }

        // For def one-liners: count semicolons in the base
        let base_semis: Vec<usize> = semi_positions
            .iter()
            .copied()
            .filter(|&pos| {
                let byte_pos = line.char_indices().nth(pos).map(|(p, _)| p).unwrap_or(0);
                let leading_spaces = line.len() - line.trim_start().len();
                let base_end_in_line = leading_spaces + base.len();
                byte_pos < base_end_in_line
            })
            .collect();

        // def foo; end → 1 semi (OK)
        // def foo; x(3); end → 2 semis (OK, single body statement)
        // def foo; x; y; end → 3 semis (NOT OK, multiple body statements)
        if base_semis.len() <= 2 {
            return base_semis;
        }

        // Too many semicolons → all are offenses
        vec![]
    }
}

impl Cop for Semicolon {
    fn name(&self) -> &'static str {
        "Style/Semicolon"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();

        if self.allow_as_expression_separator {
            return vec![];
        }

        for (line_index, line) in ctx.source.lines().enumerate() {
            let semi_positions = Self::find_semicolons(line);
            if semi_positions.is_empty() {
                continue;
            }

            let allowed = Self::allowed_semicolons_in_line(line, &semi_positions);

            for &pos in &semi_positions {
                if allowed.contains(&pos) {
                    continue;
                }

                let line_num = (line_index + 1) as u32;

                offenses.push(Offense::new(
                    self.name(),
                    "Do not use semicolons to terminate expressions.",
                    self.severity(),
                    Location::new(line_num, pos as u32, line_num, (pos + 1) as u32),
                    ctx.filename,
                ));
            }
        }

        offenses
    }
}
