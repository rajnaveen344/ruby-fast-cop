//! Lint/OrderedMagicComments - Encoding comment must precede other magic comments.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/ordered_magic_comments.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::ProgramNode;
#[allow(unused_imports)]
use crate::offense::Edit;

#[derive(Default)]
pub struct OrderedMagicComments;

impl OrderedMagicComments {
    pub fn new() -> Self {
        Self
    }

    /// Returns true if the line is a shebang.
    fn is_shebang(line: &str) -> bool {
        line.starts_with("#!")
    }

    /// Returns true if this is an encoding magic comment.
    /// Matches: `# encoding:`, `# coding:`, `# -*- ... encoding ... -*-`
    fn is_encoding_comment(line: &str) -> bool {
        let trimmed = line.trim_start_matches(|c: char| c == '#' || c.is_whitespace());
        let lower = line.to_lowercase();
        // Emacs style: -*- ... encoding: xxx -*-
        if lower.contains("-*-") && lower.contains("encoding") {
            return true;
        }
        // Standard: `# encoding:` or `# coding:`
        let body = line.trim_start_matches('#').trim_start();
        let body_lower = body.to_lowercase();
        body_lower.starts_with("encoding:") || body_lower.starts_with("coding:")
    }

    /// Returns true if this is a frozen_string_literal magic comment.
    fn is_frozen_string_literal(line: &str) -> bool {
        let body = line.trim_start_matches('#').trim_start();
        let body_lower = body.to_lowercase();
        body_lower.starts_with("frozen_string_literal:")
    }

    /// Check: is line a magic comment (encoding or frozen_string_literal)?
    fn is_magic_comment(line: &str) -> bool {
        Self::is_encoding_comment(line) || Self::is_frozen_string_literal(line)
    }
}

impl Cop for OrderedMagicComments {
    fn name(&self) -> &'static str {
        "Lint/OrderedMagicComments"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, _node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let src = ctx.source;
        let lines: Vec<&str> = src.lines().collect();

        // Find the magic comment lines (only look at leading comments, stop at blank/non-comment)
        let mut frozen_line: Option<usize> = None; // index into lines
        let mut encoding_line: Option<usize> = None;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                // Stop scanning when we hit blank line (but only after first non-shebang)
                if i > 0 {
                    break;
                }
                continue;
            }
            if !trimmed.starts_with('#') {
                break;
            }
            if Self::is_shebang(trimmed) {
                continue;
            }
            if Self::is_encoding_comment(line) {
                encoding_line = Some(i);
            } else if Self::is_frozen_string_literal(line) {
                frozen_line = Some(i);
            } else {
                // non-magic comment — stop
                break;
            }
        }

        // Offense: encoding comment exists but appears AFTER frozen_string_literal
        if let (Some(enc_idx), Some(frz_idx)) = (encoding_line, frozen_line) {
            if enc_idx > frz_idx {
                // Compute byte offsets for encoding comment line
                let mut offset = 0usize;
                for (i, line) in lines.iter().enumerate() {
                    if i == enc_idx {
                        let line_len = line.len();
                        let start = offset;
                        let end = offset + line_len;

                        // Correction: swap the two lines
                        let frz_offset = {
                            let mut o = 0usize;
                            for (j, l) in lines.iter().enumerate() {
                                if j == frz_idx { break; }
                                o += l.len() + 1; // +1 for \n
                            }
                            o
                        };
                        let frz_line = lines[frz_idx];
                        let frz_len = frz_line.len();

                        // Build correction: replace frozen line with encoding line, encoding line with frozen line
                        use crate::offense::Edit;
                        let correction = crate::offense::Correction {
                            edits: vec![
                                Edit { start_offset: frz_offset, end_offset: frz_offset + frz_len, replacement: line.to_string() },
                                Edit { start_offset: start, end_offset: end, replacement: frz_line.to_string() },
                            ],
                        };

                        let offense = ctx.offense_with_range(
                            "Lint/OrderedMagicComments",
                            "The encoding magic comment should precede all other magic comments.",
                            Severity::Warning,
                            start,
                            end,
                        );
                        return vec![offense.with_correction(correction)];
                    }
                    offset += line.len() + 1; // +1 for \n
                }
            }
        }

        vec![]
    }
}

crate::register_cop!("Lint/OrderedMagicComments", |_cfg| {
    Some(Box::new(OrderedMagicComments::new()))
});
