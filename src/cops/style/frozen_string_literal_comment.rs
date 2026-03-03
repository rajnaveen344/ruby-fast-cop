//! Style/FrozenStringLiteralComment - Checks for the presence of a frozen string literal comment.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/frozen_string_literal_comment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Always,
    AlwaysTrue,
    Never,
}

pub struct FrozenStringLiteralComment {
    enforced_style: EnforcedStyle,
}

impl FrozenStringLiteralComment {
    pub fn new(style: EnforcedStyle) -> Self {
        Self {
            enforced_style: style,
        }
    }

    /// Parse the value of a frozen_string_literal magic comment.
    /// Handles case-insensitive matching, dash/underscore variants, and emacs-style.
    /// Returns Some("true"), Some("false"), Some("token"), etc., or None if not a frozen comment.
    fn parse_frozen_comment_value(line: &str) -> Option<String> {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            return None;
        }

        let content = trimmed[1..].trim();

        // Check emacs-style: # -*- frozen_string_literal: value -*-
        if content.starts_with("-*-") && content.ends_with("-*-") {
            let inner = content[3..content.len() - 3].trim();
            // May have multiple key: value pairs separated by ;
            for part in inner.split(';') {
                let part = part.trim();
                if let Some((key, val)) = part.split_once(':') {
                    let key_normalized = key.trim().to_lowercase().replace(['-', '_'], "");
                    if key_normalized == "frozenstringliteral" {
                        return Some(val.trim().to_string());
                    }
                }
            }
            return None;
        }

        // Standard format: # frozen_string_literal: value
        // Match any case, dash/underscore variant
        if let Some((key, val)) = content.split_once(':') {
            let key_normalized = key.trim().to_lowercase().replace(['-', '_'], "");
            if key_normalized == "frozenstringliteral" {
                return Some(val.trim().to_string());
            }
        }

        None
    }

    /// Check if a line is a shebang
    fn is_shebang(line: &str) -> bool {
        line.starts_with("#!")
    }

    /// Check if a line is an encoding comment
    fn is_encoding_comment(line: &str) -> bool {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            return false;
        }
        let content = trimmed[1..].trim();

        // Emacs-style encoding: # -*- encoding: utf-8 -*-
        if content.starts_with("-*-") && content.ends_with("-*-") {
            let inner = content[3..content.len() - 3].trim();
            for part in inner.split(';') {
                let part = part.trim();
                if let Some((key, _)) = part.split_once(':') {
                    let key_lower = key.trim().to_lowercase();
                    if key_lower == "encoding" || key_lower == "coding" {
                        return true;
                    }
                }
            }
            return false;
        }

        // Ruby encoding comments: # encoding: utf-8, # coding: utf-8
        content.contains("encoding:") || content.contains("coding:")
    }

    /// Check if source has only whitespace/blank tokens (essentially empty)
    fn is_effectively_empty(source: &str) -> bool {
        source.trim().is_empty()
    }

    /// Determine the byte offset where a frozen string literal comment should be inserted.
    /// It goes after shebang and encoding lines but before code.
    /// Returns (insert_offset, replace_end_offset, had_blank_lines) — if there are blank lines
    /// at the insertion point, we consume them and flag it so the caller can re-add one.
    fn insertion_point(source: &str) -> (usize, usize, bool) {
        let mut offset = 0;
        let mut last_magic_line_end = 0;
        let mut has_any_magic = false;

        for (i, line) in source.lines().enumerate() {
            let line_end = offset + line.len();
            let next_offset = if line_end < source.len() {
                line_end + 1 // skip \n
            } else {
                line_end
            };

            let trimmed = line.trim();
            if i == 0 && Self::is_shebang(line) {
                has_any_magic = true;
                last_magic_line_end = next_offset;
                offset = next_offset;
                continue;
            }
            if Self::is_encoding_comment(line) {
                has_any_magic = true;
                last_magic_line_end = next_offset;
                offset = next_offset;
                continue;
            }
            if trimmed.is_empty() {
                // Blank line in magic area — skip it
                offset = next_offset;
                continue;
            }
            if trimmed.starts_with('#') {
                // Other comment before code
                offset = next_offset;
                continue;
            }
            // Hit code
            break;
        }

        let insert_at = if has_any_magic {
            last_magic_line_end
        } else {
            0
        };

        // Skip over blank lines at the insertion point so we don't create duplicates
        let mut replace_end = insert_at;
        while replace_end < source.len() && source.as_bytes()[replace_end] == b'\n' {
            replace_end += 1;
        }
        let had_blank_lines = replace_end > insert_at;

        (insert_at, replace_end, had_blank_lines)
    }

    /// Get the byte range of a frozen comment line (including its newline and
    /// any blank line that follows it if it's the only content between other magic comments and code).
    fn frozen_comment_byte_range(source: &str, line_idx: usize) -> (usize, usize) {
        let mut offset = 0;
        for (i, line) in source.lines().enumerate() {
            let line_end = offset + line.len();
            let next_offset = if line_end < source.len() {
                line_end + 1
            } else {
                line_end
            };

            if i == line_idx {
                // Include the trailing newline
                let mut end = next_offset;
                // If the next line is blank, include it too (remove the blank line separator)
                if end < source.len() {
                    let rest = &source[end..];
                    if rest.starts_with('\n') || rest.starts_with("\r\n") {
                        let blank_end = if rest.starts_with("\r\n") {
                            end + 2
                        } else {
                            end + 1
                        };
                        end = blank_end;
                    }
                }
                return (offset, end);
            }
            offset = next_offset;
        }
        (offset, offset)
    }

    /// Find the frozen_string_literal comment in the source.
    /// Only looks in the "magic comment" area (before any Ruby code).
    /// Returns (line_index, line_text, parsed_value) if found.
    fn find_frozen_comment_in_magic_area(source: &str) -> Option<(usize, String, String)> {
        for (i, line) in source.lines().enumerate() {
            let trimmed = line.trim();

            // Check if this line is a frozen_string_literal comment
            if let Some(value) = Self::parse_frozen_comment_value(line) {
                return Some((i, line.to_string(), value));
            }

            // Skip shebangs, encoding comments, empty lines, and other comments
            if trimmed.is_empty() || Self::is_shebang(line) || Self::is_encoding_comment(line) {
                continue;
            }
            if trimmed.starts_with('#') {
                // Other comment - could be a regular comment before code
                // In RuboCop, the frozen_string_literal comment can appear after
                // other comments (e.g., after shebang, encoding, and other comments)
                continue;
            }

            // Hit actual Ruby code - stop searching
            break;
        }
        None
    }

}

impl Cop for FrozenStringLiteralComment {
    fn name(&self) -> &'static str {
        "Style/FrozenStringLiteralComment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Empty source is always accepted
        if Self::is_effectively_empty(ctx.source) {
            return vec![];
        }

        // Frozen string literal magic comment was introduced in Ruby 2.3
        // For Ruby < 2.3, the comment has no effect, so don't enforce
        if !ctx.ruby_version_at_least(2, 3) {
            return vec![];
        }

        match self.enforced_style {
            EnforcedStyle::Always => {
                // Check if there's a valid frozen_string_literal comment in the magic area
                // The value must be "true" or "false" (case-insensitive)
                if let Some((_, _, value)) = Self::find_frozen_comment_in_magic_area(ctx.source) {
                    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
                        return vec![];
                    }
                }

                // No valid frozen_string_literal comment in the magic area
                let (insert_offset, replace_end, had_blank_lines) = Self::insertion_point(ctx.source);
                let insert_text = if had_blank_lines {
                    "# frozen_string_literal: true\n\n"
                } else {
                    "# frozen_string_literal: true\n"
                };
                let correction = Correction::replace(insert_offset, replace_end, insert_text);
                vec![Offense::new(
                    self.name(),
                    "Missing frozen string literal comment.",
                    self.severity(),
                    Location::new(1, 0, 1, 1),
                    ctx.filename,
                ).with_correction(correction)]
            }
            EnforcedStyle::AlwaysTrue => {
                // Look for the comment in the magic area first
                if let Some((line_idx, line_text, value)) =
                    Self::find_frozen_comment_in_magic_area(ctx.source)
                {
                    // Comment exists - check if it's set to "true" (case-insensitive)
                    if value.eq_ignore_ascii_case("true") {
                        return vec![]; // All good
                    }
                    // Comment exists but is not "true" (could be "false", "token", etc.)
                    // Replace just the comment line (preserve emacs style if applicable)
                    let (start, _end) = Self::frozen_comment_byte_range(ctx.source, line_idx);
                    let line_end = start + line_text.len();
                    let line_end_with_newline = if line_end < ctx.source.len() { line_end + 1 } else { line_end };

                    // Check if it's emacs-style and preserve that format
                    let replacement = if line_text.trim().contains("-*-") {
                        // Emacs style: replace value within the emacs comment
                        // Rebuild the emacs comment with frozen_string_literal: true
                        let trimmed = line_text.trim();
                        let inner = &trimmed[trimmed.find("-*-").unwrap() + 3..trimmed.rfind("-*-").unwrap()].trim();
                        // Replace frozen_string_literal value
                        let mut parts: Vec<String> = Vec::new();
                        for part in inner.split(';') {
                            let part_trimmed = part.trim();
                            if let Some((key, _)) = part_trimmed.split_once(':') {
                                let key_normalized = key.trim().to_lowercase().replace(['-', '_'], "");
                                if key_normalized == "frozenstringliteral" {
                                    parts.push(format!("{}: true", key.trim()));
                                    continue;
                                }
                            }
                            if !part_trimmed.is_empty() {
                                parts.push(part_trimmed.to_string());
                            }
                        }
                        format!("# -*- {} -*-\n", parts.join("; "))
                    } else {
                        "# frozen_string_literal: true\n".to_string()
                    };
                    let correction = Correction::replace(start, line_end_with_newline, replacement);
                    let line_num = (line_idx + 1) as u32;
                    let line_len = line_text.chars().count() as u32;
                    return vec![Offense::new(
                        self.name(),
                        "Frozen string literal comment must be set to `true`.",
                        self.severity(),
                        Location::new(line_num, 0, line_num, line_len),
                        ctx.filename,
                    ).with_correction(correction)];
                }

                // No comment found - report missing, insert it
                let (insert_offset, replace_end, had_blank_lines) = Self::insertion_point(ctx.source);
                let insert_text = if had_blank_lines {
                    "# frozen_string_literal: true\n\n"
                } else {
                    "# frozen_string_literal: true\n"
                };
                let correction = Correction::replace(insert_offset, replace_end, insert_text);
                vec![Offense::new(
                    self.name(),
                    "Missing magic comment `# frozen_string_literal: true`.",
                    self.severity(),
                    Location::new(1, 0, 1, 1),
                    ctx.filename,
                ).with_correction(correction)]
            }
            EnforcedStyle::Never => {
                // Check if there IS a frozen_string_literal comment in the magic area
                if let Some((line_idx, _line_text, _)) =
                    Self::find_frozen_comment_in_magic_area(ctx.source)
                {
                    // Delete the frozen comment line and its trailing blank line
                    let (start, end) = Self::frozen_comment_byte_range(ctx.source, line_idx);
                    let correction = Correction::delete(start, end);
                    let line_num = (line_idx + 1) as u32;
                    let line_len = _line_text.chars().count() as u32;

                    return vec![Offense::new(
                        self.name(),
                        "Unnecessary frozen string literal comment.",
                        self.severity(),
                        Location::new(line_num, 0, line_num, line_len),
                        ctx.filename,
                    ).with_correction(correction)];
                }
                // No magic comment found (even if one exists below code, it doesn't count)
                vec![]
            }
        }
    }
}
