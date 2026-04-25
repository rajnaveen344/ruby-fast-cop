//! Style/MagicCommentFormat cop
//!
//! Enforces consistent style (separators, capitalization) for magic comments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Style/MagicCommentFormat";

#[derive(Debug, Clone, PartialEq)]
pub enum SeparatorStyle {
    SnakeCase,
    KebabCase,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapStyle {
    None,
    Lowercase,
    Uppercase,
}

pub struct MagicCommentFormat {
    style: SeparatorStyle,
    directive_cap: CapStyle,
    value_cap: CapStyle,
}

impl MagicCommentFormat {
    pub fn new(style: SeparatorStyle, directive_cap: CapStyle, value_cap: CapStyle) -> Self {
        Self { style, directive_cap, value_cap }
    }
}

/// Recognized magic-comment directive keywords (matches RuboCop's MagicComment::KEYWORDS).
/// We match these case-insensitively, allowing `_` or `-` between words.
fn is_magic_directive(text: &str) -> bool {
    let lower: String = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let normalized: String = lower.chars().map(|c| if c == '-' { '_' } else { c }).collect();
    matches!(
        normalized.as_str(),
        "encoding"
            | "coding"
            | "frozen_string_literal"
            | "rbs_inline"
            | "shareable_constant_value"
            | "typed"
    )
}

/// Find the first byte offset of a Ruby (non-comment, non-blank) token.
/// Anything strictly before this byte is a "leading comment area".
fn first_non_comment_offset(source: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            offset += line.len();
            continue;
        }
        // Found a code line; the first code byte is at:
        let leading_ws = line.len() - trimmed.len();
        return Some(offset + leading_ws);
    }
    None
}

impl Cop for MagicCommentFormat {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let limit = first_non_comment_offset(source).unwrap_or(source.len());

        let mut offenses = Vec::new();
        let mut offset = 0usize;
        for line in source.split_inclusive('\n') {
            if offset >= limit {
                break;
            }
            let line_end = offset + line.len();
            // skip past trailing \n
            let line_no_nl = line.trim_end_matches('\n');

            // Find `#`
            if let Some(hash_rel) = line_no_nl.find('#') {
                let after_hash_off = offset + hash_rel + 1;
                let comment_text = &line_no_nl[hash_rel + 1..];

                // Detect emacs-style: surrounded by `-*-` ... `-*-`
                let trimmed = comment_text.trim();
                let is_emacs = trimmed.starts_with("-*-") && trimmed.ends_with("-*-") && trimmed.len() >= 6;

                if is_emacs {
                    // Inside `-*-` ... `-*-`, split by `;`
                    // We need to find the byte offset of the inner content within `comment_text`.
                    let inner_rel = comment_text.find("-*-").unwrap() + 3;
                    let inner_end_rel = comment_text.rfind("-*-").unwrap();
                    if inner_end_rel <= inner_rel {
                        offset = line_end;
                        continue;
                    }
                    let inner = &comment_text[inner_rel..inner_end_rel];
                    let inner_abs_start = after_hash_off + inner_rel;

                    // Validate: at least one part is a valid directive
                    let has_any_directive = inner.split(';').any(|part| {
                        part.split_once(':')
                            .is_some_and(|(k, _)| is_magic_directive(k.trim()))
                    });
                    if !has_any_directive {
                        offset = line_end;
                        continue;
                    }

                    // Walk inner by ';' segments, tracking absolute position.
                    let mut seg_start_rel = 0usize;
                    for part in inner.split(';') {
                        let seg_len = part.len();
                        // process this part
                        let part_abs_start = inner_abs_start + seg_start_rel;
                        self.process_directive_part(ctx, part, part_abs_start, &mut offenses);
                        seg_start_rel += seg_len + 1; // +1 for ';' separator (last is one over)
                    }
                } else {
                    // Simple comment style: parse single `key: value`
                    self.process_directive_part(ctx, comment_text, after_hash_off, &mut offenses);
                }
            }

            offset = line_end;
        }

        offenses
    }
}

impl MagicCommentFormat {
    /// `part_text` may include leading/trailing whitespace. `part_abs_start` is the byte offset
    /// in `ctx.source` where `part_text[0]` lives.
    fn process_directive_part(
        &self,
        ctx: &CheckContext,
        part_text: &str,
        part_abs_start: usize,
        offenses: &mut Vec<Offense>,
    ) {
        // Find `:` separator
        let colon_rel = match part_text.find(':') {
            Some(c) => c,
            None => return,
        };
        let key_raw = &part_text[..colon_rel];
        let after_colon_rel = colon_rel + 1;
        let value_raw = &part_text[after_colon_rel..];

        // Trim left of key to find directive start; trim right to find directive end.
        let key_left_ws = key_raw.len() - key_raw.trim_start().len();
        let key_trimmed = key_raw.trim();
        if key_trimmed.is_empty() {
            return;
        }
        // We don't strip trailing ws in our slice; recompute end.
        let key_start_rel = key_left_ws;
        let key_end_rel = key_left_ws + key_trimmed.len();

        // Only proceed if the key is a recognized magic-comment directive
        if !is_magic_directive(key_trimmed) {
            return;
        }

        // Directive offense: incorrect separator OR wrong capitalization
        let bad_separator = self.has_wrong_separator(key_trimmed);
        let bad_dir_case = self.is_wrong_case(key_trimmed, &self.directive_cap);

        if bad_separator || bad_dir_case {
            let msg = self.directive_message();
            let off_start = part_abs_start + key_start_rel;
            let off_end = part_abs_start + key_end_rel;
            offenses.push(ctx.offense_with_range(
                COP_NAME,
                &msg,
                Severity::Convention,
                off_start,
                off_end,
            ));
        }

        // Value offense: only if value_cap is set and value mismatches.
        if !matches!(self.value_cap, CapStyle::None) {
            // Find value bounds within value_raw (strip leading/trailing ws)
            let val_left_ws = value_raw.len() - value_raw.trim_start().len();
            let val_trimmed_full = value_raw.trim();
            if !val_trimmed_full.is_empty() {
                let val_start_rel = after_colon_rel + val_left_ws;
                let val_end_rel = val_start_rel + val_trimmed_full.len();
                if self.is_wrong_case(val_trimmed_full, &self.value_cap) {
                    let msg = self.value_message();
                    let off_start = part_abs_start + val_start_rel;
                    let off_end = part_abs_start + val_end_rel;
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        &msg,
                        Severity::Convention,
                        off_start,
                        off_end,
                    ));
                }
            }
        }
    }

    fn has_wrong_separator(&self, text: &str) -> bool {
        match self.style {
            SeparatorStyle::SnakeCase => text.contains('-'),
            SeparatorStyle::KebabCase => text.contains('_'),
        }
    }

    fn is_wrong_case(&self, text: &str, cap: &CapStyle) -> bool {
        match cap {
            CapStyle::None => false,
            CapStyle::Lowercase => text != text.to_lowercase(),
            CapStyle::Uppercase => text != text.to_uppercase(),
        }
    }

    fn directive_message(&self) -> String {
        // Mirrors RuboCop expected_style: "<dir_cap> <style>" with "_case" stripped.
        let style_word = match self.style {
            SeparatorStyle::SnakeCase => "snake",
            SeparatorStyle::KebabCase => "kebab",
        };
        match self.directive_cap {
            CapStyle::None => format!("Prefer {} case for magic comments.", style_word),
            CapStyle::Lowercase => {
                format!("Prefer lower {} case for magic comments.", style_word)
            }
            CapStyle::Uppercase => {
                format!("Prefer upper {} case for magic comments.", style_word)
            }
        }
    }

    fn value_message(&self) -> String {
        let cap = match self.value_cap {
            CapStyle::Lowercase => "lowercase",
            CapStyle::Uppercase => "uppercase",
            CapStyle::None => "",
        };
        format!("Prefer {} for magic comment values.", cap)
    }
}

fn parse_cap(s: Option<&str>) -> CapStyle {
    match s {
        Some("lowercase") => CapStyle::Lowercase,
        Some("uppercase") => CapStyle::Uppercase,
        _ => CapStyle::None,
    }
}

crate::register_cop!("Style/MagicCommentFormat", |cfg| {
    let cc = cfg.get_cop_config("Style/MagicCommentFormat");
    let style_str = cc
        .and_then(|c| c.enforced_style.clone())
        .unwrap_or_else(|| "snake_case".to_string());
    let style = match style_str.as_str() {
        "kebab_case" => SeparatorStyle::KebabCase,
        _ => SeparatorStyle::SnakeCase,
    };
    let dir_cap = parse_cap(
        cc.and_then(|c| c.raw.get("DirectiveCapitalization"))
            .and_then(|v| v.as_str()),
    );
    let val_cap = parse_cap(
        cc.and_then(|c| c.raw.get("ValueCapitalization"))
            .and_then(|v| v.as_str()),
    );
    Some(Box::new(MagicCommentFormat::new(style, dir_cap, val_cap)))
});
