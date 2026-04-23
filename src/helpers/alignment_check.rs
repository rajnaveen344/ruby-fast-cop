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
