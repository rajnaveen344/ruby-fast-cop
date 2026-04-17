//! Layout/TrailingWhitespace - Checks for trailing whitespace in the source code.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/trailing_whitespace.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use regex::Regex;
use std::collections::VecDeque;

pub struct TrailingWhitespace {
    allow_in_heredoc: bool,
}

/// Per-line heredoc body metadata. `None` means the line is not inside a
/// heredoc body. When `Some`, `is_static` indicates a single-quoted delimiter
/// (e.g. `<<~'X'`) and `squiggly_indent` is the minimum leading indentation
/// across the heredoc's non-blank body lines (only meaningful for `<<~`).
#[derive(Clone, Copy, Debug)]
struct HeredocInfo {
    is_static: bool,
    squiggly_indent: usize,
}

impl TrailingWhitespace {
    pub fn new() -> Self {
        Self {
            allow_in_heredoc: false,
        }
    }

    pub fn with_config(allow_in_heredoc: bool) -> Self {
        Self { allow_in_heredoc }
    }

    /// Check if a character is trailing whitespace (space, tab, or fullwidth space U+3000)
    fn is_trailing_ws(c: char) -> bool {
        c == ' ' || c == '\t' || c == '\u{3000}'
    }

    /// Find the start position of trailing whitespace in a line.
    /// Returns None if there's no trailing whitespace.
    fn trailing_ws_start(line: &str) -> Option<usize> {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            return None;
        }

        // Find the rightmost non-whitespace character
        let mut end = chars.len();
        while end > 0 && Self::is_trailing_ws(chars[end - 1]) {
            end -= 1;
        }

        if end < chars.len() {
            Some(end)
        } else {
            None
        }
    }

    /// Detect heredoc body line ranges and per-line metadata.
    ///
    /// Returns a vector of `Option<HeredocInfo>` parallel to `source.lines()`.
    /// Body lines of interpolating heredocs get `is_static=false`; body lines
    /// of single-quoted heredocs get `is_static=true`. `squiggly_indent` is
    /// the minimum leading-ws column across the heredoc's non-blank body lines
    /// (meaningful for `<<~`; for plain `<<` or `<<-` we still compute it but
    /// autocorrect only uses it for `<<~` via the "whitespace_is_indentation"
    /// check below, which still fires harmlessly for non-squiggly heredocs
    /// because their body whitespace is preserved as-is).
    fn find_heredoc_body_lines(source: &str) -> Vec<Option<HeredocInfo>> {
        let lines: Vec<&str> = source.lines().collect();
        let mut out = vec![None::<HeredocInfo>; lines.len()];

        // Regex groups: 1=dash/~, 2=open-quote-or-empty, 3=identifier, 4=close-quote-or-empty.
        let heredoc_re = Regex::new(r#"<<([-~]?)(['"]?)(\w+)(['"]?)"#).unwrap();

        #[derive(Clone)]
        struct Opener {
            id: String,
            is_static: bool,
            body_lines: Vec<usize>,
        }
        let mut queue: VecDeque<Opener> = VecDeque::new();

        let push_openers = |line: &str, queue: &mut VecDeque<Opener>| {
            for cap in heredoc_re.captures_iter(line) {
                let open_q = cap.get(2).map_or("", |m| m.as_str());
                let close_q = cap.get(4).map_or("", |m| m.as_str());
                let id = cap.get(3).map_or("", |m| m.as_str()).to_string();
                let is_static = open_q == "'" && close_q == "'";
                queue.push_back(Opener { id, is_static, body_lines: Vec::new() });
            }
        };

        for (i, line) in lines.iter().enumerate() {
            if let Some(front_id) = queue.front().map(|o| o.id.clone()) {
                let trimmed = line.trim();
                if trimmed == front_id {
                    // Closing delimiter — finalize this heredoc: compute squiggly_indent
                    // as the minimum leading-ws column of its non-blank body lines.
                    let opener = queue.pop_front().unwrap();
                    let indent = opener
                        .body_lines
                        .iter()
                        .filter_map(|&l| {
                            let s = lines[l];
                            if s.trim().is_empty() { None } else { Some(leading_ws_cols(s)) }
                        })
                        .min()
                        .unwrap_or(0);
                    for l in opener.body_lines {
                        out[l] = Some(HeredocInfo {
                            is_static: opener.is_static,
                            squiggly_indent: indent,
                        });
                    }
                } else {
                    // Body line — attribute to the front opener.
                    queue.front_mut().unwrap().body_lines.push(i);
                    // Nested heredoc openers on this line get queued.
                    push_openers(line, &mut queue);
                }
            } else {
                push_openers(line, &mut queue);
            }
        }
        out
    }
}

/// Count the number of leading space characters in a line (tabs also count as 1).
fn leading_ws_cols(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

impl Cop for TrailingWhitespace {
    fn name(&self) -> &'static str {
        "Layout/TrailingWhitespace"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let mut past_end = false;
        let mut in_doc_comment = false;
        let mut byte_offset: usize = 0;

        // Pre-compute heredoc body-line metadata.
        let heredoc_info = Self::find_heredoc_body_lines(ctx.source);

        for (line_index, line) in ctx.source.lines().enumerate() {
            let line_byte_offset = byte_offset;
            byte_offset += line.len();
            if byte_offset < ctx.source.len() {
                byte_offset += 1; // skip the '\n'
            }

            let heredoc = heredoc_info.get(line_index).copied().flatten();
            let is_in_heredoc = heredoc.is_some();

            // =begin/=end doc comments live at column 0 and don't apply inside heredocs.
            if !is_in_heredoc {
                if !in_doc_comment && line.starts_with("=begin") {
                    in_doc_comment = true;
                    continue;
                }
                if in_doc_comment && line.starts_with("=end") {
                    in_doc_comment = false;
                    continue;
                }
            }

            if in_doc_comment && !is_in_heredoc {
                continue;
            }

            if !is_in_heredoc && !in_doc_comment && line == "__END__" {
                past_end = true;
                continue;
            }
            if past_end {
                continue;
            }

            if self.allow_in_heredoc && is_in_heredoc {
                continue;
            }

            let Some(ws_start) = Self::trailing_ws_start(line) else { continue; };
            let line_char_len = line.chars().count();
            let line_num = (line_index + 1) as u32;
            let ws_byte_start = line
                .char_indices()
                .nth(ws_start)
                .map(|(pos, _)| pos)
                .unwrap_or(line.len());
            let ws_byte_end = line.len();

            let mut offense = Offense::new(
                self.name(),
                "Trailing whitespace detected.",
                self.severity(),
                Location::new(line_num, ws_start as u32, line_num, line_char_len as u32),
                ctx.filename,
            );

            let correction = match heredoc {
                None => Some(Correction::delete(
                    line_byte_offset + ws_byte_start,
                    line_byte_offset + ws_byte_end,
                )),
                Some(info) => heredoc_correction(line, line_byte_offset, ws_byte_start, ws_byte_end, info),
            };
            if let Some(c) = correction {
                offense = offense.with_correction(c);
            }
            offenses.push(offense);
        }

        offenses
    }
}

/// Build a correction for trailing whitespace inside a heredoc body line.
///
/// Mirrors RuboCop's `process_line_in_heredoc`:
/// * Static heredoc (`<<'X'` / `<<~'X'`) — no correction (can't interpolate).
/// * Whole-line whitespace whose length fits the squiggly indent — delete the
///   whitespace (after dedent the line is blank anyway).
/// * Whole-line whitespace longer than the squiggly indent — wrap only the
///   excess with `#{'…'}`, preserving the indent prefix literally.
/// * Line with content — wrap the trailing whitespace with `#{'…'}`.
fn heredoc_correction(
    line: &str,
    line_byte_offset: usize,
    ws_byte_start: usize,
    ws_byte_end: usize,
    info: HeredocInfo,
) -> Option<Correction> {
    if info.is_static {
        return None;
    }
    let whitespace_only = ws_byte_start == 0;
    let ws_len = ws_byte_end - ws_byte_start;

    let abs_start = line_byte_offset + ws_byte_start;
    let abs_end = line_byte_offset + ws_byte_end;

    if whitespace_only && ws_len <= info.squiggly_indent {
        // Delete — the line becomes blank after squiggly dedent.
        return Some(Correction::delete(abs_start, abs_end));
    }

    // Wrap only the excess (after the indent prefix, if any) with `#{'…'}`.
    let wrap_start = if whitespace_only {
        abs_start + info.squiggly_indent
    } else {
        abs_start
    };
    let original_ws = &line[wrap_start - line_byte_offset..ws_byte_end];
    let replacement = format!("#{{'{}'}}", original_ws);
    Some(Correction {
        edits: vec![Edit { start_offset: wrap_start, end_offset: abs_end, replacement }],
    })
}

crate::register_cop!("Layout/TrailingWhitespace", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/TrailingWhitespace");
    let allow_in_heredoc = cop_config
        .and_then(|c| c.raw.get("AllowInHeredoc"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(Box::new(TrailingWhitespace::with_config(allow_in_heredoc)))
});
