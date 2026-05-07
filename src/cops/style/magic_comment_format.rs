//! Style/MagicCommentFormat cop
//!
//! Ensures magic comments are written consistently (snake_case vs kebab-case,
//! directive/value capitalization).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

/// Known magic comment directive keywords (in snake_case form)
static MAGIC_DIRECTIVES: &[&str] = &[
    "frozen_string_literal",
    "encoding",
    "shareable_constant_value",
    "typed",
    "warn_indent",
    "warn_past_scope",
];

#[derive(Clone, PartialEq)]
enum MagicCommentStyle {
    SnakeCase,
    KebabCase,
}

#[derive(Clone, PartialEq)]
enum CapStyle {
    Lowercase,
    Uppercase,
}

pub struct MagicCommentFormat {
    style: MagicCommentStyle,
    directive_cap: Option<CapStyle>,
    value_cap: Option<CapStyle>,
}

impl Default for MagicCommentFormat {
    fn default() -> Self {
        Self {
            style: MagicCommentStyle::SnakeCase,
            directive_cap: None,
            value_cap: None,
        }
    }
}

impl MagicCommentFormat {
    pub fn new(
        style: MagicCommentStyle,
        directive_cap: Option<CapStyle>,
        value_cap: Option<CapStyle>,
    ) -> Self {
        Self { style, directive_cap, value_cap }
    }
}

impl Cop for MagicCommentFormat {
    fn name(&self) -> &'static str {
        "Style/MagicCommentFormat"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        check_source(ctx, &self.style, &self.directive_cap, &self.value_cap)
    }
}

fn check_source(
    ctx: &CheckContext,
    style: &MagicCommentStyle,
    directive_cap: &Option<CapStyle>,
    value_cap: &Option<CapStyle>,
) -> Vec<Offense> {
    let source = ctx.source;
    let mut offenses = Vec::new();

    // Find magic comments in leading comment section
    // (before first non-comment, non-blank token)
    let lines: Vec<&str> = source.split('\n').collect();
    let mut line_offset = 0usize;

    for line in &lines {
        let trimmed = line.trim();

        // Blank line: continue
        if trimmed.is_empty() {
            line_offset += line.len() + 1;
            continue;
        }

        // Non-comment: stop
        if !trimmed.starts_with('#') {
            break;
        }

        // Check if this is a magic comment
        // Parse both normal style (`# directive: value`) and emacs style (`# -*- ... -*-`)
        let comment_content = trimmed.strip_prefix('#').unwrap_or("").trim();
        let is_emacs = comment_content.starts_with("-*-") && comment_content.ends_with("-*-");

        let search_in = if is_emacs {
            &comment_content[3..comment_content.len() - 3]
        } else {
            comment_content
        };

        // Find directives in this comment
        let comment_start_offset = source[..line_offset + line.len().min(source[line_offset..].find('\n').unwrap_or(line.len()))].len().saturating_sub(line.len());
        let _ = comment_start_offset;
        let actual_line_start = line_offset;

        // Find the '#' position in the source line
        let hash_pos = line.find('#').unwrap_or(0);
        let content_start = actual_line_start + hash_pos + 1; // after '#'

        offenses.extend(find_directive_offenses(
            search_in,
            content_start,
            actual_line_start,
            line,
            source,
            style,
            directive_cap,
            value_cap,
            ctx,
            is_emacs,
        ));

        line_offset += line.len() + 1;
    }

    offenses
}

fn find_directive_offenses(
    search_in: &str,
    content_start: usize, // byte offset of content start (after '#') in full source
    line_start: usize,
    line: &str,
    source: &str,
    style: &MagicCommentStyle,
    directive_cap: &Option<CapStyle>,
    value_cap: &Option<CapStyle>,
    ctx: &CheckContext,
    is_emacs: bool,
) -> Vec<Offense> {
    let mut offenses = Vec::new();
    let _ = line;
    let _ = source;

    // Scan search_in for directive: value pairs
    // Directive = word chars (alphanumeric + _ + -)
    let bytes = search_in.as_bytes();
    let mut pos = 0usize;

    while pos < bytes.len() {
        // Skip whitespace and delimiters
        while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t' || bytes[pos] == b';') {
            pos += 1;
        }
        if pos >= bytes.len() { break; }

        // Try to match a directive name: letters, digits, _, -
        let dir_start = pos;
        while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_' || bytes[pos] == b'-') {
            pos += 1;
        }
        let dir_end = pos;
        if dir_start == dir_end {
            pos += 1;
            continue;
        }

        let directive_text = &search_in[dir_start..dir_end];

        // Skip whitespace before ':'
        let mut after_dir = pos;
        while after_dir < bytes.len() && bytes[after_dir] == b' ' {
            after_dir += 1;
        }

        if after_dir >= bytes.len() || bytes[after_dir] != b':' {
            // Not a directive (no colon follows)
            pos = after_dir + 1;
            continue;
        }

        // Check if this is a known magic directive (case-insensitive)
        let dir_lower = directive_text.replace('-', "_").to_lowercase();
        let is_magic = MAGIC_DIRECTIVES.iter().any(|&k| k == dir_lower.as_str());
        if !is_magic {
            pos = after_dir + 1;
            continue;
        }

        // Compute absolute offset of directive in source
        // content_start points to char after '#' (and space for emacs offset)
        // For emacs style, adjust for the '-*- ' prefix
        let emacs_offset = if is_emacs {
            // '-*- ' = 4 chars
            let hash_pos_in_line = line.find('#').unwrap_or(0);
            let after_hash = &line[hash_pos_in_line + 1..].trim_start();
            if after_hash.starts_with("-*-") {
                let content_before = line[hash_pos_in_line + 1..].find("-*-").map(|p| p + 3).unwrap_or(0);
                // leading spaces after -*-
                let after_dash = &line[hash_pos_in_line + 1 + content_before..];
                let spaces = after_dash.len() - after_dash.trim_start().len();
                content_before + spaces
            } else { 0 }
        } else {
            // Normal comment: '# ' prefix (1 char '#' already counted in content_start, plus space)
            let line_hash = line.find('#').unwrap_or(0);
            let after_hash_str = &line[line_hash + 1..];
            let leading_spaces = after_hash_str.len() - after_hash_str.trim_start().len();
            leading_spaces
        };
        let _ = emacs_offset;

        // Recompute absolute offset using line_start + position of directive in the line
        // We need to find where `directive_text` starts in the original `line`
        let dir_abs_start = find_in_line(line, line_start, dir_start, search_in);
        let dir_abs_end = dir_abs_start + dir_end - dir_start;

        // Check directive for issues
        let wrong_sep = if *style == MagicCommentStyle::SnakeCase { '-' } else { '_' };
        let correct_sep = if *style == MagicCommentStyle::SnakeCase { '_' } else { '-' };

        let has_wrong_sep = directive_text.contains(wrong_sep);
        let wrong_cap = directive_cap.as_ref().map(|cap| {
            match cap {
                CapStyle::Lowercase => directive_text != directive_text.to_lowercase(),
                CapStyle::Uppercase => directive_text != directive_text.to_uppercase(),
            }
        }).unwrap_or(false);

        if has_wrong_sep || wrong_cap {
            let (msg, corrected) = build_directive_correction(
                directive_text,
                style,
                directive_cap,
                wrong_sep,
                correct_sep,
                has_wrong_sep,
                wrong_cap,
            );

            let mut offense = ctx.offense_with_range(
                "Style/MagicCommentFormat",
                &msg,
                Severity::Convention,
                dir_abs_start,
                dir_abs_end,
            );
            let correction = Correction::replace(dir_abs_start, dir_abs_end, corrected);
            offense = offense.with_correction(correction);
            offenses.push(offense);
        }

        // Parse value (after ':' and whitespace)
        let colon_pos = after_dir;
        let mut val_start = colon_pos + 1;
        while val_start < bytes.len() && bytes[val_start] == b' ' {
            val_start += 1;
        }
        // Value ends at ';', '*' (emacs end), or end of string
        let mut val_end = val_start;
        while val_end < bytes.len() && bytes[val_end] != b';' && bytes[val_end] != b'*' {
            val_end += 1;
        }
        // Trim trailing whitespace from value
        while val_end > val_start && bytes[val_end - 1] == b' ' {
            val_end -= 1;
        }

        if val_start < val_end {
            let value_text = &search_in[val_start..val_end];
            let val_wrong_cap = value_cap.as_ref().map(|cap| {
                match cap {
                    CapStyle::Lowercase => value_text != value_text.to_lowercase(),
                    CapStyle::Uppercase => value_text != value_text.to_uppercase(),
                }
            }).unwrap_or(false);

            if val_wrong_cap {
                let cap = value_cap.as_ref().unwrap();
                let corrected_val = match cap {
                    CapStyle::Lowercase => value_text.to_lowercase(),
                    CapStyle::Uppercase => value_text.to_uppercase(),
                };
                let cap_name = match cap {
                    CapStyle::Lowercase => "lowercase",
                    CapStyle::Uppercase => "uppercase",
                };
                let val_abs_start = find_in_line(line, line_start, val_start, search_in);
                let val_abs_end = val_abs_start + (val_end - val_start);
                let msg = format!("Prefer {} for magic comment values.", cap_name);
                let mut offense = ctx.offense_with_range(
                    "Style/MagicCommentFormat",
                    &msg,
                    Severity::Convention,
                    val_abs_start,
                    val_abs_end,
                );
                let correction = Correction::replace(val_abs_start, val_abs_end, corrected_val);
                offense = offense.with_correction(correction);
                offenses.push(offense);
            }
        }

        pos = if val_end > pos { val_end } else { pos + 1 };
    }

    offenses
}

fn build_directive_correction(
    directive: &str,
    style: &MagicCommentStyle,
    directive_cap: &Option<CapStyle>,
    wrong_sep: char,
    correct_sep: char,
    has_wrong_sep: bool,
    wrong_cap: bool,
) -> (String, String) {
    // Apply separator fix first, then capitalization
    let mut corrected = if has_wrong_sep {
        directive.replace(wrong_sep, &correct_sep.to_string())
    } else {
        directive.to_string()
    };
    if wrong_cap || directive_cap.is_some() {
        corrected = match directive_cap.as_ref().unwrap() {
            CapStyle::Lowercase => corrected.to_lowercase(),
            CapStyle::Uppercase => corrected.to_uppercase(),
        };
    }

    let style_name = match (directive_cap, style) {
        (Some(CapStyle::Lowercase), MagicCommentStyle::SnakeCase) => "lower snake",
        (Some(CapStyle::Uppercase), MagicCommentStyle::SnakeCase) => "upper snake",
        (Some(CapStyle::Lowercase), MagicCommentStyle::KebabCase) => "lower kebab",
        (Some(CapStyle::Uppercase), MagicCommentStyle::KebabCase) => "upper kebab",
        (None, MagicCommentStyle::SnakeCase) => "snake",
        (None, MagicCommentStyle::KebabCase) => "kebab",
    };
    let msg = format!("Prefer {} case for magic comments.", style_name);
    (msg, corrected)
}

/// Find absolute byte offset of a substring within search_in (relative to line_start)
/// by matching its position in the full line.
fn find_in_line(line: &str, line_start: usize, pos_in_search: usize, search_in: &str) -> usize {
    // search_in is a slice of the line content after the '#' prefix.
    // Find where search_in starts in line.
    let search_in_start_in_line = line.find(search_in.trim_start()).unwrap_or(0);
    let leading_trim = search_in.len() - search_in.trim_start().len();
    let _ = leading_trim;
    // Actually: find search_in within line
    let offset = if let Some(p) = line.find(search_in) {
        p
    } else {
        // Fallback: find the content after '#'
        line.find('#').map(|h| h + 1).unwrap_or(0) + search_in_start_in_line
    };
    line_start + offset + pos_in_search
}

crate::register_cop!("Style/MagicCommentFormat", |cfg| {
    let cop_cfg = cfg.get_cop_config("Style/MagicCommentFormat");

    let style = cop_cfg
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str())
        .map(|s| if s == "kebab_case" { MagicCommentStyle::KebabCase } else { MagicCommentStyle::SnakeCase })
        .unwrap_or(MagicCommentStyle::SnakeCase);

    let parse_cap = |s: &str| -> Option<CapStyle> {
        match s {
            "lowercase" => Some(CapStyle::Lowercase),
            "uppercase" => Some(CapStyle::Uppercase),
            _ => None,
        }
    };

    let directive_cap = cop_cfg
        .and_then(|c| c.raw.get("DirectiveCapitalization"))
        .and_then(|v| v.as_str())
        .and_then(|s| parse_cap(s));

    let value_cap = cop_cfg
        .and_then(|c| c.raw.get("ValueCapitalization"))
        .and_then(|v| v.as_str())
        .and_then(|s| parse_cap(s));

    Some(Box::new(MagicCommentFormat::new(style, directive_cap, value_cap)))
});
