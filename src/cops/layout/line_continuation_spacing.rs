//! Layout/LineContinuationSpacing - Checks that the backslash of a line
//! continuation is separated from preceding text by exactly one space (default)
//! or zero spaces.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/line_continuation_spacing.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineContinuationSpacingStyle {
    Space,
    NoSpace,
}

pub struct LineContinuationSpacing {
    style: LineContinuationSpacingStyle,
}

impl Default for LineContinuationSpacing {
    fn default() -> Self {
        Self { style: LineContinuationSpacingStyle::Space }
    }
}

impl LineContinuationSpacing {
    pub fn new(style: LineContinuationSpacingStyle) -> Self {
        Self { style }
    }
}

/// Collects byte ranges (start..end) of regions where `\` line continuations
/// must be ignored: string/regexp/xstr/percent-array literals, heredoc bodies.
struct IgnoredRanges {
    source: String,
    ranges: Vec<(usize, usize)>,
}

impl IgnoredRanges {
    fn new(src: &str) -> Self {
        Self { source: src.to_string(), ranges: Vec::new() }
    }

    fn push(&mut self, start: usize, end: usize) {
        self.ranges.push((start, end));
    }

    fn contains(&self, offset: usize) -> bool {
        self.ranges.iter().any(|&(s, e)| offset >= s && offset < e)
    }
}

impl<'pr> Visit<'pr> for IgnoredRanges {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        let loc = node.location();
        self.push(loc.start_offset(), loc.end_offset());
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        let loc = node.location();
        self.push(loc.start_offset(), loc.end_offset());
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
        let loc = node.location();
        self.push(loc.start_offset(), loc.end_offset());
        ruby_prism::visit_x_string_node(self, node);
    }

    fn visit_interpolated_x_string_node(
        &mut self,
        node: &ruby_prism::InterpolatedXStringNode<'pr>,
    ) {
        let loc = node.location();
        self.push(loc.start_offset(), loc.end_offset());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }

    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode<'pr>) {
        let loc = node.location();
        self.push(loc.start_offset(), loc.end_offset());
        ruby_prism::visit_regular_expression_node(self, node);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode<'pr>,
    ) {
        let loc = node.location();
        self.push(loc.start_offset(), loc.end_offset());
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'pr>) {
        // Only percent-array literals (%w / %i / %W / %I) — opening starts with '%'.
        if let Some(open) = node.opening_loc() {
            let opener = &self.source[open.start_offset()..open.end_offset()];
            if opener.starts_with('%') {
                let loc = node.location();
                self.push(loc.start_offset(), loc.end_offset());
            }
        }
        ruby_prism::visit_array_node(self, node);
    }
}

impl Cop for LineContinuationSpacing {
    fn name(&self) -> &'static str {
        "Layout/LineContinuationSpacing"
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.source.contains('\\') {
            return vec![];
        }

        let mut ig = IgnoredRanges::new(ctx.source);
        ig.visit_program_node(node);

        // Add heredoc body ranges (Prism's heredoc node location only covers the
        // `<<-X` opener, not the body lines).
        for (s, e) in find_heredoc_body_byte_ranges(ctx.source) {
            ig.push(s, e);
        }

        // Comments: each line, find first `#` not inside a literal range.
        // Simple approach: line-by-line scan, mark any `#..\n` range as ignored.
        let bytes = ctx.source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let line_start = i;
            // find end of line
            let mut eol = i;
            while eol < bytes.len() && bytes[eol] != b'\n' {
                eol += 1;
            }
            // find first `#` not inside a literal range
            let mut j = line_start;
            while j < eol {
                if bytes[j] == b'#' && !ig.contains(j) {
                    // mark from `#` to end-of-line
                    ig.push(j, eol);
                    break;
                }
                j += 1;
            }
            i = eol + 1;
        }

        // Find __END__ cutoff (line starting with `__END__` exactly).
        let mut end_marker: Option<usize> = None;
        let mut off = 0usize;
        for line in ctx.source.lines() {
            if line == "__END__" {
                end_marker = Some(off);
                break;
            }
            off += line.len() + 1;
        }

        let mut offenses = Vec::new();

        // Walk line-by-line. For each line ending with `\`, evaluate offensive ws.
        let mut byte_offset = 0usize;
        for (line_index, line) in ctx.source.lines().enumerate() {
            let line_start = byte_offset;
            byte_offset += line.len() + 1;

            if let Some(em) = end_marker {
                if line_start >= em {
                    continue;
                }
            }

            if !line.ends_with('\\') {
                continue;
            }
            // The backslash byte position.
            let backslash_pos = line_start + line.len() - 1;

            // Skip if the backslash is inside an ignored range.
            if ig.contains(backslash_pos) {
                continue;
            }

            // Count whitespace immediately before the backslash.
            let line_bytes = line.as_bytes();
            let mut ws_count = 0usize;
            let mut k = line_bytes.len() - 1; // position of '\'
            while k > 0 {
                let b = line_bytes[k - 1];
                if b == b' ' || b == b'\t' {
                    ws_count += 1;
                    k -= 1;
                } else {
                    break;
                }
            }

            let offensive = match self.style {
                LineContinuationSpacingStyle::NoSpace => ws_count >= 1,
                LineContinuationSpacingStyle::Space => ws_count != 1,
            };
            if !offensive {
                continue;
            }

            // For space style: when ws_count == 0, RuboCop's regex `(?<!\s)\\$`
            // captures the single non-ws char + backslash → length 2; offense
            // range = (line.length - 2, line.length).
            // When ws_count >= 2, regex captures `\s{2,}\\$` → length ws_count+1.
            // For no_space style: regex `\s+\\$` → length ws_count+1.
            let (col_start, col_end) = match self.style {
                LineContinuationSpacingStyle::Space if ws_count == 0 => {
                    // Just the backslash (1-char range).
                    let line_char_len = line.chars().count();
                    (line_char_len - 1, line_char_len)
                }
                _ => {
                    let line_char_len = line.chars().count();
                    // ws_count chars of ws + backslash; range covers the ws region.
                    // RuboCop emits range starting at (line.length - offensive_spacing.length - 1)
                    // with length offensive_spacing.length. offensive_spacing INCLUDES backslash
                    // for both styles (regex captures the trailing \\). length = ws_count + 1.
                    // Actually for Space style with ws==0, capture len = 2, start = len-2-1 = -1?
                    // RuboCop code: `line.length - offensive_spacing.length - 1`
                    //   For ws=0 space style: spacing="X\\", len=2, start = N-2-1 = N-3 ... wait
                    // Let's derive from TOML: source `if 2 + 2\` ws=0, line len=9 (chars), expected
                    //   col_start=8, col_end=9. So start = N-1 (the backslash), end = N.
                    // Hmm but RuboCop reports range from col 8 to col 9 = 1 char wide = the `\`.
                    //   Actually RuboCop report: `line.length - offensive_spacing.length - 1` =
                    //   9 - 2 - 1 = 6, length 2 → cols 6..8 (the `2\`). But TOML says 8..9.
                    // So TOML's column_end is actually the END column (exclusive) and our data
                    //   says column_start=8, column_end=9 — which is 1-char range = just `\`.
                    // Hmm conflict. Let me check `if 2 + 2  \`  (ws=2) test: col 8-11.
                    //   Line len=11 (chars). RuboCop: 11-3-1=7, len 3 → cols 7..10. But TOML 8..11.
                    // The TOMLs we have show different offsets. Let me trust TOMLs.
                    // Pattern observed:
                    //   ws=0 space: col_start = N-1, col_end = N (1 char range = backslash).
                    //   ws=2: col_start = N-3, col_end = N (3-char range = `  \`).
                    //   ws=3: col_start = N-4, col_end = N (4-char range = `   \`).
                    // ws=2 with N=11: start=8, end=11 ✓ matches TOML 8..11.
                    // ws=3 with `if 2 + 2    \` N=13: start=8, end=11? TOML says 8..11 ?? Let me re-check
                    // Actually re-read: ws=4 case `if 2 + 2    \` expected col 8..13. N=13. start=N-5=8 ✓.
                    // Pattern: col_end = N (line char len without newline), col_start = col_end - (ws_count + 1) for ws>=1, or col_end = N-1 col_start = N-2 for ws=0 space.
                    let cend = line_char_len - 1; // position of backslash (0-indexed)
                    // Wait: line_char_len for `if 2 + 2 \` = 10 chars. col_end=10? TOML says 10 for ws=1 no_space.
                    // Hmm conflict again. Let me recount: `if 2 + 2 \` — i,f,space,2,space,+,space,2,space,\ = 10 chars. TOML for no_space ws=1 says col_start=8, col_end=10.
                    // So col_end = line_char_len = 10 (= one past the backslash).
                    // ws=1 no_space, col_start = 8, col_end = 10. col_start = col_end - (ws+1) = 10-2 = 8 ✓
                    // ws=2 no_space `if 2 + 2  \` 11 chars, TOML 8..11: col_end=11=line_char_len, col_start=11-3=8 ✓
                    // ws=4 no_space `if 2 + 2    \` 13 chars, TOML 8..13: col_end=13=line_char_len, col_start=13-5=8 ✓
                    // ws=2 space `if 2 + 2  \` 11 chars TOML 8..11: matches.
                    // ws=0 space `if 2 + 2\` 9 chars TOML 8..9: col_end=9=line_char_len, col_start=8=col_end-1.
                    let _ = cend;
                    let cend2 = line_char_len;
                    (cend2 - (ws_count + 1).max(1), cend2)
                }
            };

            // For ws=0 space style, the "offensive range" highlights only the backslash (col N-1..N).
            // BUT autocorrect must replace `X\` with `X \` (insert space before backslash).
            // We compute the byte range to apply correction:
            let line_byte_start = line_start;
            // Offense byte range:
            let (b_start, b_end) = match (self.style, ws_count) {
                (LineContinuationSpacingStyle::Space, 0) => {
                    // backslash byte
                    (backslash_pos, backslash_pos + 1)
                }
                _ => {
                    // ws_count bytes of whitespace + backslash byte
                    (backslash_pos - ws_count, backslash_pos + 1)
                }
            };

            // Correction:
            let replacement = match self.style {
                LineContinuationSpacingStyle::NoSpace => "\\".to_string(),
                LineContinuationSpacingStyle::Space => " \\".to_string(),
            };

            let message = match self.style {
                LineContinuationSpacingStyle::NoSpace => "Use zero spaces in front of backslash.",
                LineContinuationSpacingStyle::Space => "Use one space in front of backslash.",
            };

            let line_num = (line_index + 1) as u32;
            let _ = line_byte_start;
            let mut offense = Offense::new(
                self.name(),
                message,
                self.severity(),
                Location::new(line_num, col_start as u32, line_num, col_end as u32),
                ctx.filename,
            );
            offense = offense.with_correction(Correction::replace(b_start, b_end, replacement));
            offenses.push(offense);
        }

        offenses
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

/// Returns byte ranges (start..end) covering heredoc body lines (everything
/// between an opener line's end and the closing identifier line).
fn find_heredoc_body_byte_ranges(source: &str) -> Vec<(usize, usize)> {
    let heredoc_re = Regex::new(r#"<<([-~]?)(['"]?)(\w+)(['"]?)"#).unwrap();

    #[derive(Clone)]
    struct Opener {
        id: String,
    }
    let mut queue: VecDeque<Opener> = VecDeque::new();
    let mut ranges = Vec::new();
    let mut current_body_start: Option<usize> = None;

    let mut byte_offset = 0usize;
    let lines: Vec<&str> = source.lines().collect();
    let mut line_offsets = Vec::with_capacity(lines.len());
    for line in &lines {
        line_offsets.push(byte_offset);
        byte_offset += line.len() + 1;
    }

    let push_openers = |line: &str, queue: &mut VecDeque<Opener>| {
        for cap in heredoc_re.captures_iter(line) {
            let id = cap.get(3).map_or("", |m| m.as_str()).to_string();
            queue.push_back(Opener { id });
        }
    };

    for (i, line) in lines.iter().enumerate() {
        let line_start = line_offsets[i];
        if let Some(front_id) = queue.front().map(|o| o.id.clone()) {
            let trimmed = line.trim();
            if trimmed == front_id {
                if let Some(start) = current_body_start.take() {
                    ranges.push((start, line_start));
                }
                queue.pop_front();
            } else {
                if current_body_start.is_none() {
                    current_body_start = Some(line_start);
                }
                push_openers(line, &mut queue);
            }
        } else {
            push_openers(line, &mut queue);
            if !queue.is_empty() && current_body_start.is_none() {
                // Body starts on next line.
                let next_off = line_offsets.get(i + 1).copied().unwrap_or(source.len());
                current_body_start = Some(next_off);
            }
        }
    }

    ranges
}

crate::register_cop!("Layout/LineContinuationSpacing", |cfg| {
    let c: Cfg = cfg.typed("Layout/LineContinuationSpacing");
    let style = match c.enforced_style.as_deref() {
        Some("no_space") => LineContinuationSpacingStyle::NoSpace,
        _ => LineContinuationSpacingStyle::Space,
    };
    Some(Box::new(LineContinuationSpacing::new(style)))
});
