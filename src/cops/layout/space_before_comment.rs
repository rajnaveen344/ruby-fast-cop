//! Layout/SpaceBeforeComment - Checks for space before end-of-line comments.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_before_comment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

#[derive(Default)]
pub struct SpaceBeforeComment;

impl SpaceBeforeComment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for SpaceBeforeComment {
    fn name(&self) -> &'static str {
        "Layout/SpaceBeforeComment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;
        let mut byte_offset: usize = 0;
        let mut in_doc_comment = false;

        for line in source.lines() {
            let line_start = byte_offset;
            byte_offset += line.len();
            if byte_offset < source.len() {
                byte_offset += 1; // '\n'
            }

            // Track =begin/=end doc comment blocks - skip them entirely
            if !in_doc_comment && line.starts_with("=begin") {
                in_doc_comment = true;
                continue;
            }
            if in_doc_comment {
                if line.starts_with("=end") {
                    in_doc_comment = false;
                }
                continue;
            }

            // Find '#' that starts a comment on this line
            let comment_pos = find_comment_in_line(line);
            if let Some(hash_col) = comment_pos {
                // It's a standalone comment (line starts with #) — skip
                if line[..hash_col].trim().is_empty() {
                    continue;
                }

                // Check byte immediately before '#'
                if hash_col == 0 {
                    continue;
                }
                let before = line.as_bytes()[hash_col - 1];
                if before == b' ' || before == b'\t' {
                    continue; // already has space
                }

                // Offense: no space before '#'
                let abs_hash = line_start + hash_col;
                let comment_end = line_start + line.len();
                let col_start = line[..hash_col].chars().count() as u32;
                let col_end = line.chars().count() as u32;

                // line number
                let line_num = source[..line_start].chars().filter(|&c| c == '\n').count() as u32 + 1;

                offenses.push(Offense::new(
                    self.name(),
                    "Put a space before an end-of-line comment.",
                    Severity::Convention,
                    Location::new(line_num, col_start, line_num, col_end),
                    ctx.filename,
                ).with_correction(Correction::insert(abs_hash, " ")));

                let _ = comment_end;
            }
        }

        offenses
    }
}

/// Find byte index of '#' that starts a comment in this line.
/// Handles strings, heredoc marker lines, etc. minimally:
/// we skip '#' inside single/double quoted strings.
fn find_comment_in_line(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\'' if !in_double => {
                in_single = !in_single;
            }
            b'"' if !in_single => {
                in_double = !in_double;
            }
            b'\\' if in_single || in_double => {
                i += 1; // skip escaped char
            }
            b'#' if !in_single && !in_double => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

crate::register_cop!("Layout/SpaceBeforeComment", |_cfg| {
    Some(Box::new(SpaceBeforeComment::new()))
});
