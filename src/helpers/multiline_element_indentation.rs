//! Shared logic for multiline element indentation checks.
//!
//! Mirrors RuboCop's `MultilineElementIndentation` mixin used by
//! `Layout/FirstHashElementIndentation` and `Layout/FirstArrayElementIndentation`.
//!
//! Computes the "base column" against which the first element's indentation
//! is checked, based on the configured `EnforcedStyle`. Four possible base
//! column types:
//!   * `LeftBraceOrBracket`  — `align_braces` / `align_brackets` style
//!   * `ParentHashKey`       — the hash/array is the value of an outer hash
//!                             pair whose key is on the same line as the
//!                             opening brace/bracket, and the outer hash has
//!                             a following sibling pair on a later line.
//!   * `FirstColumnAfterLeftParenthesis` — `special_inside_parentheses`
//!                             (default) style, the hash/array is directly
//!                             argument to a call whose `(` is on the same
//!                             line as the opening brace/bracket.
//!   * `StartOfLine`         — fallback: the start-of-line indentation of the
//!                             line containing the opening brace/bracket.

use crate::cops::CheckContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    SpecialInsideParentheses,
    Consistent,
    /// `align_braces` for hashes, `align_brackets` for arrays.
    BraceAlignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentBaseType {
    LeftBraceOrBracket,
    FirstColumnAfterLeftParenthesis,
    ParentHashKey,
    StartOfLine,
}

/// Information about a surrounding outer hash pair whose value is the
/// current hash/array being checked. Used for the `ParentHashKey` base.
#[derive(Debug, Clone, Copy)]
pub struct ParentPairInfo {
    /// Column of the outer pair's key (0-indexed).
    pub pair_column: usize,
    /// Whether the outer pair's key and value begin on the same line.
    pub key_and_value_same_line: bool,
    /// Whether there is a right sibling pair beginning on a later line than the
    /// outer pair's last line.
    pub has_right_sibling_on_later_line: bool,
}

/// Compute the base column and base type for the first element check.
///
/// * `left_brace_col` — column of the `{` or `[`.
/// * `left_brace_line_start` — byte offset of the start of the line containing `{`/`[`.
/// * `left_paren` — `Some(col)` when the hash/array is a direct argument of a
///   parenthesized call whose `(` is on the same line as `{`/`[`.
/// * `parent_pair` — information about the surrounding outer hash pair, if any.
pub fn indent_base(
    ctx: &CheckContext,
    style: EnforcedStyle,
    left_brace_col: usize,
    left_brace_line_start: usize,
    left_paren: Option<usize>,
    parent_pair: Option<ParentPairInfo>,
) -> (usize, IndentBaseType) {
    if style == EnforcedStyle::BraceAlignment {
        return (left_brace_col, IndentBaseType::LeftBraceOrBracket);
    }

    if let Some(pp) = parent_pair {
        if pp.key_and_value_same_line && pp.has_right_sibling_on_later_line {
            return (pp.pair_column, IndentBaseType::ParentHashKey);
        }
    }

    if let Some(paren_col) = left_paren {
        if style == EnforcedStyle::SpecialInsideParentheses {
            return (paren_col + 1, IndentBaseType::FirstColumnAfterLeftParenthesis);
        }
    }

    // Default: first non-ws column of the line containing the left brace/bracket.
    let col = first_non_ws_col(ctx, left_brace_line_start);
    (col, IndentBaseType::StartOfLine)
}

/// First non-whitespace column on the line whose start byte offset is `line_start`.
fn first_non_ws_col(ctx: &CheckContext, line_start: usize) -> usize {
    let bytes = ctx.source.as_bytes();
    let mut i = line_start;
    while i < bytes.len() && bytes[i] != b'\n' {
        if bytes[i] != b' ' && bytes[i] != b'\t' {
            return i - line_start;
        }
        i += 1;
    }
    // All whitespace line: fall back to 0.
    0
}

/// Whether the bytes before `col` on the line containing `offset`
/// are all whitespace (i.e. the character at `offset`/`col` begins its line).
pub fn bracket_begins_its_line(ctx: &CheckContext, offset: usize) -> bool {
    ctx.begins_its_line(offset)
}
