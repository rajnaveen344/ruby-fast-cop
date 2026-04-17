//! Style/Semicolon cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

pub struct Semicolon {
    allow_as_expression_separator: bool,
}

impl Semicolon {
    pub fn new(allow_as_expression_separator: bool) -> Self {
        Self { allow_as_expression_separator }
    }

    fn find_semicolons(line: &str) -> Vec<(usize, bool)> {
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
                    positions.push((i, interpolation_depth > 0));
                }
                _ => {}
            }
            i += 1;
        }

        positions
    }

    fn allowed_semicolons_in_line(line: &str, semi_positions: &[(usize, bool)]) -> Vec<usize> {
        let trimmed = line.trim();

        let base = trimmed.trim_end_matches(';').trim_end();

        let is_def_oneliner = base.starts_with("def ") && (base.ends_with(" end") || base.ends_with(";end"));
        let is_class_oneliner = (base.starts_with("class ") || base.starts_with("module "))
            && (base.ends_with(" end") || base.ends_with(";end"));

        if !is_def_oneliner && !is_class_oneliner {
            return vec![];
        }

        if is_class_oneliner {
            let base_semis: Vec<usize> = semi_positions
                .iter()
                .filter(|&&(pos, _)| {
                    // Check if this semicolon is within the base text (not trailing)
                    let leading_spaces = line.len() - line.trim_start().len();
                    let base_end_in_line = leading_spaces + base.len();
                    let byte_pos = line.char_indices().nth(pos).map(|(p, _)| p).unwrap_or(0);
                    byte_pos < base_end_in_line
                })
                .map(|&(pos, _)| pos)
                .collect();

            if base_semis.len() == 1 { return base_semis; }
            return vec![];
        }

        let base_semis: Vec<usize> = semi_positions
            .iter()
            .filter(|&&(pos, _)| {
                let byte_pos = line.char_indices().nth(pos).map(|(p, _)| p).unwrap_or(0);
                let leading_spaces = line.len() - line.trim_start().len();
                let base_end_in_line = leading_spaces + base.len();
                byte_pos < base_end_in_line
            })
            .map(|&(pos, _)| pos)
            .collect();

        if base_semis.len() <= 2 { return base_semis; }
        vec![]
    }

    fn is_endless_range_before(before: &str) -> bool {
        let trimmed = before.trim_end();
        trimmed.ends_with("..") || trimmed.ends_with("...")
    }

    /// Check if a line represents a method call with hash value omission and no parens,
    /// ending with a trailing semicolon. Pattern: `m key:;` or `obj&.m key:;`
    /// Returns Some((method_start, args_end)) if it matches, where we need to add parens.
    fn hash_value_omission_method_call(line: &str) -> Option<(usize, usize)> {
        let trimmed = line.trim_end();
        // Must end with `:;` (hash value omission followed by semicolon)
        if !trimmed.ends_with(":;") && !trimmed.ends_with(": ;") {
            // Check if it ends with :; possibly with trailing whitespace
            let without_semi = trimmed.trim_end_matches(';').trim_end();
            if !without_semi.ends_with(':') {
                return None;
            }
        }

        let without_semi = trimmed.trim_end_matches(';').trim_end();
        // Must contain a method call pattern: `word key:` (with space, no parens)
        // Look for the first space that separates method name from args
        if without_semi.contains('(') {
            return None; // Already has parens
        }

        // Find the boundary between method name and arguments
        // Could be `m key:` or `obj&.m key:, other:` or `obj.m key:`
        // Find first space after the method name
        let bytes = without_semi.as_bytes();
        let mut method_end = None;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b' ' && i > 0 {
                // Check if what comes before looks like a method name (word chars, dots, &.)
                let before = &without_semi[..i];
                let after = &without_semi[i + 1..];
                // The args part should contain `key:` patterns
                if after.contains(':') && !before.is_empty() {
                    method_end = Some(i);
                    break;
                }
            }
            i += 1;
        }

        if let Some(me) = method_end {
            // Account for leading whitespace
            let leading_ws = line.len() - line.trim_start().len();
            let abs_method_end = leading_ws + me;
            let abs_args_end = leading_ws + without_semi.len();
            Some((abs_method_end, abs_args_end))
        } else {
            None
        }
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

        let mut byte_offset: usize = 0;
        for (line_index, line) in ctx.source.lines().enumerate() {
            let line_byte_offset = byte_offset;
            byte_offset += line.len();
            if byte_offset < ctx.source.len() {
                byte_offset += 1; // skip '\n'
            }

            let semi_positions = Self::find_semicolons(line);
            if semi_positions.is_empty() {
                continue;
            }

            let allowed = Self::allowed_semicolons_in_line(line, &semi_positions);

            for &(pos, in_interpolation) in &semi_positions {
                if allowed.contains(&pos) {
                    continue;
                }

                let line_num = (line_index + 1) as u32;

                // Compute byte offset of the semicolon
                let semi_byte_pos = line.char_indices().nth(pos).map(|(p, _)| p).unwrap_or(pos);
                let abs_semi = line_byte_offset + semi_byte_pos;

                let before_semi = &line[..semi_byte_pos];
                let after_semi = &line[semi_byte_pos + 1..];
                let trimmed_before = before_semi.trim_end();
                let trimmed_after = after_semi.trim_start();

                // Context-aware correction
                let correction = if in_interpolation {
                    // Inside string interpolation: just delete the semicolon
                    Correction::delete(abs_semi, abs_semi + 1)
                } else if trimmed_after.is_empty() {
                    // Trailing semicolon
                    if Self::is_endless_range_before(before_semi) {
                        // Endless range: `42..;` → `(42..)`
                        // Find the start of the range expression
                        let range_content = before_semi.trim();
                        let leading_ws = &line[..line.len() - line.trim_start().len()];
                        let replacement = format!("{}({})", leading_ws, range_content);
                        Correction::replace(line_byte_offset, abs_semi + 1, replacement)
                    } else if let Some((method_end, args_end)) = Self::hash_value_omission_method_call(line) {
                        // Method call with hash value omission: `m key:;` → `m(key:)`
                        let abs_method_end = line_byte_offset + method_end;
                        let abs_args_end = line_byte_offset + args_end;
                        let args_text = &ctx.source[abs_method_end + 1..abs_args_end];
                        let replacement = format!("({})", args_text);
                        Correction::replace(abs_method_end, abs_semi + 1, replacement)
                    } else {
                        // Regular trailing semicolon: delete
                        Correction::delete(abs_semi, abs_semi + 1)
                    }
                } else if trimmed_before.is_empty()
                    || trimmed_before.ends_with('{')
                    || trimmed_after.starts_with('}')
                {
                    // At start of line, after opening brace, or before closing brace:
                    // just delete the semicolon
                    Correction::delete(abs_semi, abs_semi + 1)
                } else {
                    // Expression separator: replace semicolon + whitespace with newline + indent
                    let ws_after = after_semi.len() - trimmed_after.len();
                    Correction::replace(abs_semi, abs_semi + 1 + ws_after, "\n ")
                };

                offenses.push(Offense::new(
                    self.name(),
                    "Do not use semicolons to terminate expressions.",
                    self.severity(),
                    Location::new(line_num, pos as u32, line_num, (pos + 1) as u32),
                    ctx.filename,
                ).with_correction(correction));
            }
        }

        offenses
    }
}

crate::register_cop!("Style/Semicolon", |cfg| {
    let cop_config = cfg.get_cop_config("Style/Semicolon");
    let allow = cop_config
        .and_then(|c| c.raw.get("AllowAsExpressionSeparator"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(Box::new(Semicolon::new(allow)))
});
