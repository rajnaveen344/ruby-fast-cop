//! Layout/EmptyLineAfterMagicComment
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/empty_line_after_magic_comment.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Location, Offense, Severity};

const COP_NAME: &str = "Layout/EmptyLineAfterMagicComment";
const MSG: &str = "Add an empty line after magic comments.";

// RuboCop's MagicComment recognizes these patterns:
// - frozen_string_literal
// - encoding / coding
// - warn_indent / warn_past_scope
// - shareable_constant_value
// - typed (Sorbet)
// - rbs_inline (rbs-inline gem — only enabled/disabled)

#[derive(Default)]
pub struct EmptyLineAfterMagicComment;

impl EmptyLineAfterMagicComment {
    pub fn new() -> Self {
        Self
    }

    fn is_magic_comment(line: &str) -> bool {
        let content = match line.trim().strip_prefix('#') {
            Some(c) => c.trim(),
            None => return false,
        };
        // Emacs-style: -*-...-*-
        let inner = if content.starts_with("-*-") && content.ends_with("-*-") {
            &content[3..content.len() - 3]
        } else {
            content
        };
        // Check for known magic comment keys
        for part in inner.split(';') {
            if let Some((key, _)) = part.trim().split_once(':') {
                let key = key.trim().to_lowercase();
                let key = key.replace(['-', '_', ' '], "");
                match key.as_str() {
                    "frozenstringliteral" | "encoding" | "coding" | "warningindent"
                    | "warningindentedafter" | "shareableconstantvalue" => return true,
                    _ => {}
                }
            }
        }
        // typed: (Sorbet)
        if inner.starts_with("typed:") {
            return true;
        }
        // rbs_inline: enabled or disabled
        if let Some(val) = inner.strip_prefix("rbs_inline:") {
            let v = val.trim().to_lowercase();
            if v == "enabled" || v == "disabled" {
                return true;
            }
        }
        false
    }
}

impl Cop for EmptyLineAfterMagicComment {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let lines: Vec<&str> = source.lines().collect();
        if lines.is_empty() {
            return vec![];
        }

        // Find the last magic comment at top of file (only whitespace/comments before code)
        // Collect initial magic comments (may include shebang lines at top)
        let mut last_magic_idx: Option<usize> = None;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                // blank line after magic comments — stops the preamble
                break;
            }
            if trimmed.starts_with('#') {
                if Self::is_magic_comment(line) {
                    last_magic_idx = Some(i);
                }
                // regular comment: continue looking (part of preamble)
            } else {
                // non-comment code: stop
                break;
            }
        }

        let magic_idx = match last_magic_idx {
            Some(i) => i,
            None => return vec![],
        };

        // Check the line immediately after the magic comment
        let next_idx = magic_idx + 1;
        if next_idx >= lines.len() {
            // magic comment is the last line — no offense
            return vec![];
        }
        let next_line = lines[next_idx].trim();
        if next_line.is_empty() {
            // blank line follows — ok
            return vec![];
        }

        // Offense: at start of next_line (the line after the magic comment)
        let next_line_offset = line_byte_offset(source, next_idx + 1); // 1-indexed
        let offense_end = next_line_offset + 1;
        let loc = Location::from_offsets(source, next_line_offset, offense_end.min(source.len()));
        // Correction: insert "\n" before the next line
        let correction = Correction::insert(next_line_offset, "\n");

        vec![Offense::new(COP_NAME, MSG, Severity::Convention, loc, ctx.filename)
            .with_correction(correction)]
    }
}

crate::register_cop!("Layout/EmptyLineAfterMagicComment", |_cfg| {
    Some(Box::new(EmptyLineAfterMagicComment::new()))
});
