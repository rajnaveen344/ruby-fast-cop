//! Style/FrozenStringLiteralComment - Checks for the presence of a frozen string literal comment.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/frozen_string_literal_comment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

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
                // Always report on line 1 (RuboCop behavior)
                vec![Offense::new(
                    self.name(),
                    "Missing frozen string literal comment.",
                    self.severity(),
                    Location::new(1, 0, 1, 1),
                    ctx.filename,
                )]
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
                    let line_num = (line_idx + 1) as u32;
                    let line_len = line_text.chars().count() as u32;
                    return vec![Offense::new(
                        self.name(),
                        "Frozen string literal comment must be set to `true`.",
                        self.severity(),
                        Location::new(line_num, 0, line_num, line_len),
                        ctx.filename,
                    )];
                }

                // No comment found - report missing
                // Always report on line 1
                vec![Offense::new(
                    self.name(),
                    "Missing magic comment `# frozen_string_literal: true`.",
                    self.severity(),
                    Location::new(1, 0, 1, 1),
                    ctx.filename,
                )]
            }
            EnforcedStyle::Never => {
                // Check if there IS a frozen_string_literal comment in the magic area
                if let Some((line_idx, line_text, _)) =
                    Self::find_frozen_comment_in_magic_area(ctx.source)
                {
                    let line_num = (line_idx + 1) as u32;
                    let line_len = line_text.chars().count() as u32;

                    return vec![Offense::new(
                        self.name(),
                        "Unnecessary frozen string literal comment.",
                        self.severity(),
                        Location::new(line_num, 0, line_num, line_len),
                        ctx.filename,
                    )];
                }
                // No magic comment found (even if one exists below code, it doesn't count)
                vec![]
            }
        }
    }
}
