//! Shared helpers for RuboCop `Alignment` mixin semantics used by
//! `Layout/ArgumentAlignment`, `Layout/ArrayAlignment`, `Layout/ParameterAlignment`.
//!
//! Port of `each_bad_alignment` from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/alignment.rb
//!
//! An "item" here is a byte range (start_offset, end_offset). For each item
//! whose start offset:
//!   1. falls on a strictly later line than the previous item,
//!   2. "begins its line" (only whitespace precedes it on that line),
//! we compare its 0-indexed byte column against `base_column`. A mismatch is
//! an offense whose range is the item's full source range.
//!
//! `base_column` is computed by callers:
//!   - `with_first_*`: 0-indexed col of the first item,
//!   - `with_fixed_indentation`: indent of the target method line + IndentationWidth.

use crate::cops::CheckContext;
use crate::offense::{Correction, Edit};

/// One offense: byte range covering the misaligned item.
#[derive(Debug, Clone, Copy)]
pub struct MisalignedItem {
    pub start_offset: usize,
    pub end_offset: usize,
}

/// Walk `items` as (start_offset, end_offset) pairs, yielding those that fail
/// alignment against `base_column` (display-column, matching RuboCop).
pub fn each_bad_alignment(
    ctx: &CheckContext,
    items: &[(usize, usize)],
    base_column: usize,
) -> Vec<MisalignedItem> {
    let mut out = Vec::new();
    let mut prev_line: i64 = -1;
    for &(start, end) in items {
        let line = ctx.line_of(start) as i64;
        if line > prev_line && ctx.begins_its_line(start) {
            let col = display_col_of(ctx, start);
            if col != base_column {
                out.push(MisalignedItem { start_offset: start, end_offset: end });
            }
        }
        prev_line = line;
    }
    out
}

/// Compute display-column (Unicode display width) of `offset` from the start of
/// its line. Matches RuboCop's `display_column` which uses Unicode::DisplayWidth.
pub fn display_col_of(ctx: &CheckContext, offset: usize) -> usize {
    let start = ctx.line_start(offset);
    let prefix = &ctx.source[start..offset];
    display_width(prefix)
}

/// Indent (display-width) of the line containing `offset`.
pub fn display_indent_of(ctx: &CheckContext, offset: usize) -> usize {
    let start = ctx.line_start(offset);
    let bytes = ctx.source.as_bytes();
    let mut i = start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    display_width(&ctx.source[start..i])
}

/// Approximate Unicode display width: wide chars (CJK, fullwidth) = 2, others = 1.
/// Sufficient for fixtures using fullwidth Latin and common CJK blocks.
fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
    let cp = c as u32;
    // Quick ASCII path.
    if cp < 0x80 {
        return 1;
    }
    // Common East-Asian Wide / Fullwidth ranges (subset sufficient for typical code).
    let wide = matches!(cp,
        0x1100..=0x115F | // Hangul Jamo
        0x2E80..=0x303E |
        0x3041..=0x33FF |
        0x3400..=0x4DBF |
        0x4E00..=0x9FFF | // CJK Unified
        0xA000..=0xA4CF |
        0xAC00..=0xD7A3 | // Hangul Syllables
        0xF900..=0xFAFF |
        0xFE30..=0xFE4F |
        0xFF00..=0xFF60 | // Fullwidth forms (incl. Ｒｕｂｙ)
        0xFFE0..=0xFFE6 |
        0x20000..=0x2FFFD |
        0x30000..=0x3FFFD
    );
    if wide { 2 } else { 1 }
}

/// Indentation (0-indexed byte column of first non-ws) of the line containing `offset`.
pub fn indent_of(ctx: &CheckContext, offset: usize) -> usize {
    ctx.indentation_of(offset)
}

/// Build a correction that shifts ALL lines of an item from `start` to `end`
/// by `column_delta` (= base_column - item's current column).
/// Mirrors RuboCop's `AlignmentCorrector.correct` which walks each_line of the node.
/// The first line: replace `[line_start..item_start]` with `" ".repeat(base_column)`.
/// Continuation lines: adjust leading whitespace by the same delta.
/// Skips lines that contain heredoc content (lines where item offset > line content).
pub fn alignment_correction(ctx: &CheckContext, start: usize, end: usize, base_column: usize) -> Correction {
    let source = ctx.source.as_bytes();
    let item_col = ctx.col_of(start);
    let delta: isize = base_column as isize - item_col as isize;

    let mut edits: Vec<Edit> = Vec::new();
    let first_line_start = ctx.line_start(start);
    // First line: replace leading whitespace up to item start
    edits.push(Edit {
        start_offset: first_line_start,
        end_offset: start,
        replacement: " ".repeat(base_column),
    });

    // Continuation lines within item range
    let mut pos = first_line_start;
    loop {
        let eol = source[pos..].iter().position(|&b| b == b'\n')
            .map_or(source.len(), |i| pos + i);
        let next_line = eol + 1;
        if next_line > source.len() || eol >= end { break; }
        pos = next_line;
        if pos > end { break; }
        // Find leading whitespace end on this continuation line
        let mut ws_end = pos;
        while ws_end < source.len() && (source[ws_end] == b' ' || source[ws_end] == b'\t') {
            ws_end += 1;
        }
        // Only shift lines that have leading whitespace (skip heredoc body lines at col 0)
        let cur_indent = ws_end - pos;
        if cur_indent > 0 && ws_end < end {
            let new_indent = (cur_indent as isize + delta).max(0) as usize;
            edits.push(Edit {
                start_offset: pos,
                end_offset: ws_end,
                replacement: " ".repeat(new_indent),
            });
        }
    }

    Correction { edits }
}
