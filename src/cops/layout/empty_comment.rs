//! Layout/EmptyComment - Checks for empty comments.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_comment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

pub struct EmptyComment {
    allow_border: bool,
    allow_margin: bool,
}

impl EmptyComment {
    pub fn new(allow_border: bool, allow_margin: bool) -> Self {
        Self { allow_border, allow_margin }
    }

    /// A border comment is one that contains only repeated '#' chars (like ####)
    fn is_border(text: &str) -> bool {
        let content = text.trim_start_matches('#');
        content.is_empty() || content.chars().all(|c| c == '#')
    }

    /// Is this comment empty? (just '#' with optional whitespace after)
    fn is_empty(text: &str) -> bool {
        // text is the full comment text including '#'
        let after_hash = text.strip_prefix('#').unwrap_or("");
        after_hash.trim().is_empty()
    }
}

impl Default for EmptyComment {
    fn default() -> Self {
        Self { allow_border: true, allow_margin: true }
    }
}

impl Cop for EmptyComment {
    fn name(&self) -> &'static str {
        "Layout/EmptyComment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;
        let lines: Vec<&str> = source.split('\n').collect();
        let n = lines.len();

        // Parse lines: find comment positions per line
        // Each line: find if it has a '#' comment and whether it's empty/border
        // We also need to find margin comments (empty comment lines adjacent to non-empty comment lines)

        // First pass: classify each line
        #[derive(Debug, Clone, PartialEq)]
        enum LineKind {
            Code,             // code only (no comment)
            EmptyStandalone,  // line is just an empty comment (no code before it)
            BorderStandalone, // line is just a border comment
            NonEmptyStandalone, // line is a non-empty comment (no code)
            EmptyInline,      // code + empty comment at end
            BorderInline,     // code + border comment (shouldn't happen)
        }

        struct LineInfo<'s> {
            kind: LineKind,
            comment_start: usize,  // byte offset of '#' in source
            comment_end: usize,    // byte offset after comment (end of '#...')
            comment_text: &'s str, // the comment text
            line_byte_start: usize,
        }

        let mut byte_offset: usize = 0;
        let mut line_infos: Vec<LineInfo> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            let line_start = byte_offset;
            byte_offset += line.len();
            if i + 1 < n {
                byte_offset += 1;
            }

            // Find '#' not in string (simplified: find first '#')
            // RuboCop uses token stream; we do simple text scan
            let trimmed = line.trim();
            if !trimmed.contains('#') {
                line_infos.push(LineInfo {
                    kind: LineKind::Code,
                    comment_start: 0,
                    comment_end: 0,
                    comment_text: "",
                    line_byte_start: line_start,
                });
                continue;
            }

            // Find position of '#' (look for it not inside strings — simplified)
            let hash_col = find_comment_hash(line);
            if hash_col.is_none() {
                line_infos.push(LineInfo {
                    kind: LineKind::Code,
                    comment_start: 0,
                    comment_end: 0,
                    comment_text: "",
                    line_byte_start: line_start,
                });
                continue;
            }
            let hash_col = hash_col.unwrap();
            let comment_text = &line[hash_col..];
            let comment_abs_start = line_start + hash_col;
            let comment_abs_end = line_start + line.len();

            let before_hash = line[..hash_col].trim();
            let has_code_before = !before_hash.is_empty();
            let empty = Self::is_empty(comment_text);
            // Border = multiple '#' chars only (like ####). Single '#' is just empty.
            let border = comment_text.len() > 1 && comment_text.chars().all(|c| c == '#');

            let kind = if has_code_before {
                if empty || border {
                    LineKind::EmptyInline
                } else {
                    LineKind::Code
                }
            } else if border {
                LineKind::BorderStandalone
            } else if empty {
                LineKind::EmptyStandalone
            } else {
                LineKind::NonEmptyStandalone
            };

            line_infos.push(LineInfo {
                kind,
                comment_start: comment_abs_start,
                comment_end: comment_abs_end,
                comment_text,
                line_byte_start: line_start,
            });
        }

        // Identify margin comments: empty standalone comment lines adjacent to non-empty comment lines
        // A margin group = consecutive non-empty standalone comments (possibly framed by empty standalone lines)
        // Margin = empty standalone lines that are direct neighbors of non-empty comment lines
        let is_margin = |idx: usize| -> bool {
            if line_infos[idx].kind != LineKind::EmptyStandalone {
                return false;
            }
            // Check neighbors
            let prev_non_empty = (0..idx).rev().find(|&j| {
                matches!(line_infos[j].kind, LineKind::NonEmptyStandalone | LineKind::EmptyStandalone)
            });
            let next_non_empty = (idx+1..n).find(|&j| {
                matches!(line_infos[j].kind, LineKind::NonEmptyStandalone | LineKind::EmptyStandalone)
            });
            // Margin if surrounded by (or adjacent to) non-empty comment line
            let prev_is_comment = prev_non_empty.map(|j| {
                line_infos[j].kind == LineKind::NonEmptyStandalone
                    || (line_infos[j].kind == LineKind::EmptyStandalone)
            }).unwrap_or(false);
            let next_is_comment = next_non_empty.map(|j| {
                line_infos[j].kind == LineKind::NonEmptyStandalone
                    || (line_infos[j].kind == LineKind::EmptyStandalone)
            }).unwrap_or(false);

            // Is there a non-empty comment in the neighborhood?
            let prev_has_text = (0..idx).rev().take_while(|&j| {
                matches!(line_infos[j].kind, LineKind::NonEmptyStandalone | LineKind::EmptyStandalone)
            }).any(|j| line_infos[j].kind == LineKind::NonEmptyStandalone);

            let next_has_text = (idx+1..n).take_while(|j| {
                matches!(line_infos[*j].kind, LineKind::NonEmptyStandalone | LineKind::EmptyStandalone)
            }).any(|j| line_infos[j].kind == LineKind::NonEmptyStandalone);

            (prev_is_comment || next_is_comment) && (prev_has_text || next_has_text)
        };

        for (i, info) in line_infos.iter().enumerate() {
            match info.kind {
                LineKind::BorderStandalone => {
                    if self.allow_border {
                        continue;
                    }
                    // Offense: whole comment
                    let offense = Offense::new(
                        self.name(),
                        "Source code comment is empty.",
                        Severity::Convention,
                        Location::from_offsets(source, info.comment_start, info.comment_end),
                        ctx.filename,
                    ).with_correction(delete_whole_line(source, info.line_byte_start, info.comment_end));
                    offenses.push(offense);
                }
                LineKind::EmptyStandalone => {
                    let margin = is_margin(i);
                    if self.allow_margin && margin {
                        continue;
                    }
                    // Margin comments being disallowed → remove entire line including newline
                    // (they're adjacent to non-empty content which should remain flush)
                    let correction = if !self.allow_margin && margin {
                        delete_whole_line_including_newline(source, info.line_byte_start)
                    } else {
                        // Check if next line is also an empty standalone comment (for correction grouping)
                        let next_is_same = (i + 1 < n)
                            && line_infos[i + 1].kind == LineKind::EmptyStandalone
                            && !(self.allow_margin && is_margin(i + 1));
                        if next_is_same {
                            // Not the last in group → delete whole line including '\n'
                            delete_whole_line_including_newline(source, info.line_byte_start)
                        } else {
                            // Last in group (or single) → delete just '#', leave '\n'
                            delete_whole_line(source, info.line_byte_start, info.comment_end)
                        }
                    };
                    let offense = Offense::new(
                        self.name(),
                        "Source code comment is empty.",
                        Severity::Convention,
                        Location::from_offsets(source, info.comment_start, info.comment_start + 1),
                        ctx.filename,
                    ).with_correction(correction);
                    offenses.push(offense);
                }
                LineKind::EmptyInline => {
                    // Inline empty comment: remove the comment + preceding whitespace
                    // offense is just the '#'
                    let offense = Offense::new(
                        self.name(),
                        "Source code comment is empty.",
                        Severity::Convention,
                        Location::from_offsets(source, info.comment_start, info.comment_start + 1),
                        ctx.filename,
                    ).with_correction(delete_inline_comment(source, info.line_byte_start, info.comment_start));
                    offenses.push(offense);
                }
                _ => {}
            }
        }

        offenses
    }
}

/// Find the byte position of '#' in a line that starts a comment (not in a string).
/// Returns the byte offset within the line.
fn find_comment_hash(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'#' if !in_single && !in_double => return Some(i),
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'\\' if in_single || in_double => { i += 1; }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Correction: delete whole line including the newline character.
fn delete_whole_line_including_newline(source: &str, line_start: usize) -> crate::offense::Correction {
    let bytes = source.as_bytes();
    let mut end = line_start;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    if end < bytes.len() {
        end += 1; // include the '\n'
    }
    Correction::delete(line_start, end)
}

/// Correction: replace whole line content with empty (leaving the newline).
/// For standalone comment lines: `#\n` → `\n` (keeps blank line).
/// If this is the only content (no trailing newline), deletes everything.
fn delete_whole_line(source: &str, line_start: usize, _comment_end: usize) -> crate::offense::Correction {
    let bytes = source.as_bytes();
    // Find end of this line (NOT including the newline char)
    let mut end = line_start;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    // Delete from line_start to end (leave '\n' in place)
    Correction::delete(line_start, end)
}

/// Correction: delete inline comment — '#' + preceding whitespace
fn delete_inline_comment(source: &str, line_start: usize, comment_start: usize) -> crate::offense::Correction {
    let bytes = source.as_bytes();
    // Find end of line
    let mut end = comment_start + 1;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    // Start of deletion: go back over whitespace before '#'
    let mut del_start = comment_start;
    while del_start > line_start && (bytes[del_start - 1] == b' ' || bytes[del_start - 1] == b'\t') {
        del_start -= 1;
    }
    Correction::delete(del_start, end)
}

crate::register_cop!("Layout/EmptyComment", |cfg| {
    let cop_cfg = cfg.get_cop_config("Layout/EmptyComment");
    let allow_border = cop_cfg
        .as_ref()
        .and_then(|c| c.raw.get("AllowBorderComment"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let allow_margin = cop_cfg
        .as_ref()
        .and_then(|c| c.raw.get("AllowMarginComment"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(EmptyComment::new(allow_border, allow_margin)))
});
