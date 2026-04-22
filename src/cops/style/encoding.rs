//! Style/Encoding cop
//!
//! Flags unnecessary `# encoding: utf-8` magic comments (UTF-8 is the default since Ruby 2.0).

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Offense, Severity};

const MSG: &str = "Unnecessary utf-8 encoding comment.";

#[derive(Default)]
pub struct Encoding;

impl Encoding {
    pub fn new() -> Self {
        Self
    }

    /// Check if a comment line is a UTF-8 encoding magic comment.
    /// Returns (is_utf8_encoding, is_vim_style, is_emacs_style)
    fn is_utf8_encoding_comment(line: &str) -> (bool, bool, bool) {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            return (false, false, false);
        }
        let content = trimmed[1..].trim();

        // Emacs style: `# -*- encoding: utf-8 -*-` or `# -*- encoding: utf-8; mode: X -*-`
        if content.starts_with("-*-") && content.ends_with("-*-") {
            let inner = content[3..content.len() - 3].trim();
            // Check if any part is encoding: utf-8
            for part in inner.split(';') {
                let p = part.trim();
                if let Some((k, v)) = p.split_once(':') {
                    let key = k.trim().to_lowercase();
                    if (key == "encoding" || key == "fileencoding") && v.trim().to_lowercase() == "utf-8" {
                        return (true, false, true);
                    }
                }
            }
            return (false, false, false);
        }

        // Vim style: `# vim:filetype=ruby, fileencoding=utf-8`
        if content.starts_with("vim:") || content.starts_with("vim: ") {
            let rest = &content[4..].trim_start_matches(' ');
            for part in rest.split(',') {
                let p = part.trim();
                if let Some((k, v)) = p.split_once('=') {
                    let key = k.trim().to_lowercase();
                    if (key == "fileencoding" || key == "encoding") && v.trim().to_lowercase() == "utf-8" {
                        return (true, true, false);
                    }
                }
            }
            return (false, false, false);
        }

        // Standard: `# encoding: utf-8` or `# coding: utf-8` or `# -*- coding: utf-8 -*-`
        // Also matches `# Encoding: UTF-8`
        // Various forms: `encoding:`, `coding:`, `encoding =`
        let c_lower = content.to_lowercase();
        let encoding_key = if let Some(pos) = find_encoding_key(&c_lower) {
            pos
        } else {
            return (false, false, false);
        };

        let after_colon = c_lower[encoding_key..].trim();
        let value = after_colon.trim_start_matches(':').trim_start_matches(' ').trim();
        // value might be wrapped in spaces: ` utf-8`
        let value = value.split_whitespace().next().unwrap_or("").trim_matches('-');
        // Normalize: "utf-8", "utf8", "UTF-8", "UTF8"
        let normalized = value.replace('-', "").to_lowercase();
        if normalized == "utf8" {
            (true, false, false)
        } else {
            (false, false, false)
        }
    }
}

fn find_encoding_key(content: &str) -> Option<usize> {
    // Look for `coding:` or `encoding:`
    if let Some(pos) = content.find("encoding:") {
        return Some(pos + "encoding".len());
    }
    if let Some(pos) = content.find("coding:") {
        return Some(pos + "coding".len());
    }
    None
}

impl Cop for Encoding {
    fn name(&self) -> &'static str {
        "Style/Encoding"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let lines: Vec<&str> = ctx.source.lines().collect();

        // Only check the first two lines (+ shebang skip)
        let mut line_idx = 0;
        let max_check = if lines.len() >= 1 && lines[0].starts_with("#!") {
            2  // shebang on line 0, check lines 0..2
        } else {
            // Check lines until we find a non-comment, non-empty line
            // But encoding must be in first 2 lines
            2
        };

        // Track which lines are candidates (only lines before a blank line break the magic)
        // RuboCop: encoding comment must be adjacent to top (no blank line between top and comment)
        let mut offset = 0usize;
        for (i, line) in lines.iter().enumerate() {
            if i >= lines.len() {
                break;
            }
            let (is_utf8, is_vim, is_emacs) = Self::is_utf8_encoding_comment(line);
            if is_utf8 {
                let line_start = offset;
                let line_end = line_start + line.len();

                if is_vim {
                    // Vim: remove encoding part from the comment
                    // e.g. `# vim:filetype=ruby, fileencoding=utf-8` → `# vim: filetype=ruby`
                    let corrected = remove_vim_encoding(line);
                    let correction = Correction::replace(line_start, line_end, corrected);
                    offenses.push(
                        ctx.offense_with_range(self.name(), MSG, self.severity(), line_start, line_end)
                            .with_correction(correction)
                    );
                } else if is_emacs {
                    // Emacs: remove encoding part or whole line
                    let corrected = remove_emacs_encoding(line);
                    if corrected.is_empty() {
                        // Remove whole line including newline
                        let nl_end = if line_end < ctx.source.len() { line_end + 1 } else { line_end };
                        let correction = Correction::replace(line_start, nl_end, String::new());
                        offenses.push(
                            ctx.offense_with_range(self.name(), MSG, self.severity(), line_start, line_end)
                                .with_correction(correction)
                        );
                    } else {
                        let correction = Correction::replace(line_start, line_end, corrected);
                        offenses.push(
                            ctx.offense_with_range(self.name(), MSG, self.severity(), line_start, line_end)
                                .with_correction(correction)
                        );
                    }
                } else {
                    // Standard: remove the line content (keep the newline if it's not the last line)
                    // RuboCop replaces the comment line with empty string, keeping the newline.
                    // So: `# encoding: utf-8\n` → `\n` (line becomes empty)
                    // But if there's a next line, we delete the whole line to avoid blank line.
                    // Actually RuboCop: for multi-line source, delete the encoding line entirely.
                    // For single-line source, keep the newline.
                    // Check if there are lines after this one:
                    let has_next_line = line_end < ctx.source.len()
                        && ctx.source.as_bytes().get(line_end).copied() == Some(b'\n')
                        && line_end + 1 < ctx.source.len();
                    let (del_start, del_end) = if has_next_line {
                        // Delete line + its newline (so next line moves up)
                        (line_start, line_end + 1)
                    } else {
                        // Last line or no newline: just delete content, keep newline
                        (line_start, line_end)
                    };
                    let correction = Correction::replace(del_start, del_end, String::new());
                    offenses.push(
                        ctx.offense_with_range(self.name(), MSG, self.severity(), line_start, line_end)
                            .with_correction(correction)
                    );
                }
            } else {
                // Non-encoding line: check if we should stop looking
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    // Blank line breaks the magic comment zone — stop
                    offset += line.len() + 1;
                    break;
                }
                if !trimmed.starts_with('#') {
                    // Non-comment line — stop
                    offset += line.len() + 1;
                    break;
                }
            }
            offset += line.len() + 1; // +1 for \n
        }

        offenses
    }
}

fn remove_vim_encoding(line: &str) -> String {
    // Remove `, fileencoding=utf-8` or `, encoding=utf-8` from vim comment
    let trimmed = line.trim();
    let content = &trimmed[1..].trim();  // skip `#`
    // Find `vim:` prefix
    let rest = &content[4..];  // skip `vim:`
    let rest = rest.trim_start_matches(' ');

    let mut parts: Vec<&str> = rest.split(',').collect();
    parts.retain(|p| {
        let p = p.trim();
        if let Some((k, v)) = p.split_once('=') {
            let key = k.trim().to_lowercase();
            if (key == "fileencoding" || key == "encoding") && v.trim().to_lowercase() == "utf-8" {
                return false;
            }
        }
        true
    });

    if parts.is_empty() {
        String::new()
    } else {
        // Rebuild: `# vim: part1, part2`
        let joined = parts.iter().map(|p| p.trim()).collect::<Vec<_>>().join(", ");
        format!("# vim: {}", joined)
    }
}

fn remove_emacs_encoding(line: &str) -> String {
    let trimmed = line.trim();
    let content = &trimmed[1..].trim();
    let inner = &content[3..content.len() - 3].trim();

    let mut parts: Vec<&str> = inner.split(';').collect();
    parts.retain(|p| {
        let p = p.trim();
        if let Some((k, v)) = p.split_once(':') {
            let key = k.trim().to_lowercase();
            if (key == "encoding" || key == "fileencoding") && v.trim().to_lowercase() == "utf-8" {
                return false;
            }
        }
        if let Some((k, v)) = p.split_once('=') {
            let key = k.trim().to_lowercase();
            if (key == "encoding" || key == "fileencoding") && v.trim().to_lowercase() == "utf-8" {
                return false;
            }
        }
        true
    });

    if parts.is_empty() {
        String::new()
    } else {
        let joined = parts.iter().map(|p| p.trim()).collect::<Vec<_>>().join("; ");
        format!("# -*- {} -*-", joined)
    }
}

crate::register_cop!("Style/Encoding", |_cfg| {
    Some(Box::new(Encoding::new()))
});
