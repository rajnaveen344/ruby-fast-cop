//! Layout/CommentIndentation
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/comment_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Location, Offense, Severity};

const COP_NAME: &str = "Layout/CommentIndentation";

pub struct CommentIndentation {
    allow_for_alignment: bool,
    indentation_width: usize,
    access_modifier_outdent: bool,
}

impl CommentIndentation {
    pub fn new(allow_for_alignment: bool, indentation_width: usize, access_modifier_outdent: bool) -> Self {
        Self { allow_for_alignment, indentation_width, access_modifier_outdent }
    }
}

impl Default for CommentIndentation {
    fn default() -> Self {
        Self::new(true, 2, false)
    }
}

impl CommentIndentation {
    fn is_own_line_comment(&self, lines: &[&str], line_idx: usize) -> bool {
        if line_idx >= lines.len() {
            return false;
        }
        let line = lines[line_idx];
        // Own line = the line is whitespace then #
        let trimmed = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        trimmed.starts_with('#')
    }

    fn line_after_comment<'a>(&self, lines: &[&'a str], comment_idx: usize) -> Option<&'a str> {
        // Return the first non-blank line after the comment
        for i in (comment_idx + 1)..lines.len() {
            let line = lines[i];
            if !line.trim().is_empty() {
                return Some(line);
            }
        }
        None
    }

    fn correct_indentation(&self, next_line: Option<&str>) -> usize {
        let Some(line) = next_line else { return 0 };
        let indent = line.len() - line.trim_start_matches(|c: char| c == ' ' || c == '\t').len();
        let extra = if self.less_indented(line) { self.indentation_width } else { 0 };
        indent + extra
    }

    fn less_indented(&self, line: &str) -> bool {
        let trimmed = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        if trimmed.starts_with("end")
            || trimmed.starts_with(')')
            || trimmed.starts_with(']')
            || trimmed.starts_with('}')
        {
            return true;
        }
        // Access modifier outdent
        if self.access_modifier_outdent
            && (trimmed.starts_with("private")
                || trimmed.starts_with("protected")
                || trimmed.starts_with("public"))
        {
            let rest = &trimmed[7.min(trimmed.len())..];
            if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('(') {
                return true;
            }
        }
        false
    }

    fn two_alternatives(&self, line: &str) -> bool {
        let trimmed = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        trimmed.starts_with("else")
            || trimmed.starts_with("elsif")
            || trimmed.starts_with("when")
            || trimmed.starts_with("in ")
            || trimmed.starts_with("in\t")
            || trimmed == "in"
            || trimmed.starts_with("rescue")
            || trimmed.starts_with("ensure")
    }

    fn col_of_str(line: &str) -> usize {
        line.len() - line.trim_start_matches(|c: char| c == ' ' || c == '\t').len()
    }
}

impl Cop for CommentIndentation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let all_lines: Vec<&str> = source.lines().collect();

        // Find __END__ boundary
        let end_marker = all_lines.iter().position(|&l| l == "__END__");

        let lines = match end_marker {
            Some(idx) => &all_lines[..idx],
            None => &all_lines[..],
        };

        let mut offenses: Vec<Offense> = Vec::new();
        // Track consecutive own-line comments and their columns for AllowForAlignment
        // A comment is "correctly aligned" if it matches a preceding inline comment's column

        // Collect all inline comment columns: line_idx -> column of #
        // For allow_for_alignment: scan for `code # comment` on lines before us
        let mut inline_comment_cols: Vec<Option<usize>> = vec![None; lines.len()];
        for (i, line) in lines.iter().enumerate() {
            // Inline comment: not an own-line comment, but line has #
            if line.trim().starts_with('#') {
                // own line comment — skip
                continue;
            }
            // Find # position (not in string)
            if let Some(hash_pos) = crate::helpers::source::find_comment_start(line) {
                if hash_pos > 0 {
                    inline_comment_cols[i] = Some(hash_pos);
                }
            }
        }

        for (idx, _line) in lines.iter().enumerate() {
            if !self.is_own_line_comment(lines, idx) {
                continue;
            }
            let line = lines[idx];
            let column = Self::col_of_str(line);

            let next_line = self.line_after_comment(lines, idx);
            let original_correct = self.correct_indentation(next_line);
            let mut correct = original_correct;

            let column_delta = correct as isize - column as isize;
            if column_delta == 0 {
                continue;
            }

            // two_alternatives: if next line is else/elsif/when/etc.
            // RuboCop increments correct_comment_indentation and uses the higher value for message.
            // BUT @column_delta stays at original value, so autocorrect uses original_correct.
            if let Some(nl) = next_line {
                if self.two_alternatives(nl) {
                    let alt_correct = correct + self.indentation_width;
                    if column == alt_correct {
                        continue; // acceptable at the higher level
                    }
                    // Use the higher value for the offense message (RuboCop behavior)
                    correct = alt_correct;
                }
            }

            // AllowForAlignment: check if aligned with a preceding inline comment
            if self.allow_for_alignment {
                // Look backward through own-line comments above us and inline comment columns
                let mut allow = false;
                for j in (0..idx).rev() {
                    if self.is_own_line_comment(lines, j) {
                        // Stop: another own-line comment above (must have same col to continue)
                        if Self::col_of_str(lines[j]) == column {
                            // Same column as comment above, keep looking
                            continue;
                        } else {
                            break;
                        }
                    } else {
                        // Non-comment line: check inline comment col
                        if let Some(ic) = inline_comment_cols[j] {
                            if ic == column {
                                allow = true;
                            }
                        }
                        break;
                    }
                }
                if allow {
                    continue;
                }
            }

            // Offense: the comment token (from column to end of trimmed content)
            let line_start = line_byte_offset(source, idx + 1);
            let comment_start = line_start + column;
            let trimmed_len = line.trim_end().len().saturating_sub(column);
            let comment_end = comment_start + trimmed_len.max(1);

            let loc = Location::from_offsets(source, comment_start, comment_end.min(source.len()));
            let message = format!(
                "Incorrect indentation detected (column {} instead of {}).",
                column, correct
            );

            // Correction: replace leading whitespace on this comment AND preceding consecutive comments
            // with same column (RuboCop's autocorrect_preceding_comments).
            let new_indent = " ".repeat(original_correct);
            let mut edits = vec![crate::offense::Edit {
                start_offset: line_start,
                end_offset: comment_start,
                replacement: new_indent.clone(),
            }];
            // Walk backward: collect own-line comments on consecutive previous lines with same column
            let mut prev_idx = idx as isize - 1;
            while prev_idx >= 0 {
                let pi = prev_idx as usize;
                if !self.is_own_line_comment(lines, pi) {
                    break;
                }
                if Self::col_of_str(lines[pi]) != column {
                    break;
                }
                // Check consecutive: prev line is pi, current comment is idx, they must be adjacent
                // RuboCop: loc.line == ref_loc.line - 1 (1-based, so pi+1 == idx)
                // We already walked from idx backwards so pi = idx - (steps). Must be adjacent.
                let prev_ls = line_byte_offset(source, pi + 1);
                let prev_cs = prev_ls + Self::col_of_str(lines[pi]);
                edits.push(crate::offense::Edit {
                    start_offset: prev_ls,
                    end_offset: prev_cs,
                    replacement: new_indent.clone(),
                });
                prev_idx -= 1;
            }
            let correction = crate::offense::Correction { edits };

            offenses.push(
                Offense::new(COP_NAME, &message, Severity::Convention, loc, ctx.filename)
                    .with_correction(correction),
            );
        }

        offenses
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_for_alignment: Option<bool>,
}

crate::register_cop!("Layout/CommentIndentation", |cfg| {
    let c: Cfg = cfg.typed("Layout/CommentIndentation");
    let allow_for_alignment = c.allow_for_alignment.unwrap_or(true);
    let width = cfg.get_cop_config("Layout/IndentationWidth")
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(2);
    // Check AccessModifierIndentation style
    let outdent = cfg.get_cop_config("Layout/AccessModifierIndentation")
        .and_then(|c| c.enforced_style.as_deref().map(|s| s == "outdent"))
        .unwrap_or(false);
    Some(Box::new(CommentIndentation::new(allow_for_alignment, width, outdent)))
});
