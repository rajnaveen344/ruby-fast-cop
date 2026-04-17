//! Layout/SpaceInsideArrayLiteralBrackets
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/space_inside_array_literal_brackets.rb
//!
//! Checks that brackets used for array literals have or don't have
//! surrounding space depending on configuration.

use crate::cops::{CheckContext, Cop};
use crate::helpers::surrounding_space as ss;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/SpaceInsideArrayLiteralBrackets";
const MSG_NO_SPACE: &str = "Do not use space inside array brackets.";
const MSG_SPACE: &str = "Use space inside array brackets.";
const MSG_EMPTY_NO_SPACE: &str = "Do not use space inside empty array brackets.";
const MSG_EMPTY_SPACE_ONE: &str = "Use one space inside empty array brackets.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceInsideArrayLiteralBracketsStyle {
    NoSpace,
    Space,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyBracketsStyle {
    NoSpace,
    Space,
}

pub struct SpaceInsideArrayLiteralBrackets {
    style: SpaceInsideArrayLiteralBracketsStyle,
    empty_style: EmptyBracketsStyle,
}

impl SpaceInsideArrayLiteralBrackets {
    pub fn new(
        style: SpaceInsideArrayLiteralBracketsStyle,
        empty_style: EmptyBracketsStyle,
    ) -> Self {
        Self { style, empty_style }
    }

    fn check_brackets(
        &self,
        ctx: &CheckContext,
        node_start: usize,
        left_start: usize,
        left_end: usize,
        right_start: usize,
        right_end: usize,
    ) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;

        // Empty brackets?
        if ss::is_empty_between(source, left_end, right_start) {
            match self.empty_style {
                EmptyBracketsStyle::NoSpace => {
                    if !ss::no_character_between(left_end, right_start) {
                        offenses.push(empty_offense(
                            ctx,
                            left_start,
                            right_end,
                            MSG_EMPTY_NO_SPACE,
                        ));
                    }
                }
                EmptyBracketsStyle::Space => {
                    if !ss::has_exactly_one_space(source, left_end, right_start) {
                        offenses.push(empty_offense(
                            ctx,
                            left_start,
                            right_end,
                            MSG_EMPTY_SPACE_ONE,
                        ));
                    }
                }
            }
            return offenses;
        }

        // Non-empty: style checks
        // Don't flag leading if `[` is at end of its line (next_to_newline)
        let start_ok = ss::next_to_newline_after(source, left_end);
        // Don't flag trailing if `]` begins its line (has only whitespace before it on its line)
        let is_single_line = ss::same_line(source, left_start, right_end);
        let end_ok = !is_single_line && begins_its_line(source, right_start);

        match self.style {
            SpaceInsideArrayLiteralBracketsStyle::NoSpace => {
                // If next token after `[` is a comment (`[ # foo`), allow leading space
                let start_ok = start_ok || ss::next_is_comment(source, left_end);
                self.no_space_offenses(
                    ctx, source, left_end, right_start, start_ok, end_ok, &mut offenses,
                );
            }
            SpaceInsideArrayLiteralBracketsStyle::Space => {
                self.space_offenses(
                    ctx, source, left_end, right_start, start_ok, end_ok, &mut offenses,
                );
            }
            SpaceInsideArrayLiteralBracketsStyle::Compact => {
                self.compact_offenses(
                    ctx,
                    source,
                    node_start,
                    left_start,
                    left_end,
                    right_start,
                    start_ok,
                    end_ok,
                    &mut offenses,
                );
            }
        }

        offenses
    }

    fn no_space_offenses(
        &self,
        ctx: &CheckContext,
        source: &str,
        left_end: usize,
        right_start: usize,
        start_ok: bool,
        end_ok: bool,
        offenses: &mut Vec<Offense>,
    ) {
        if !start_ok {
            let n = ss::count_spaces_after(source, left_end);
            if n > 0 {
                // ensure they're on the same line as `[`
                if !source.as_bytes()[left_end..left_end + n].contains(&b'\n') {
                    offenses.push(space_offense(ctx, left_end, left_end + n, MSG_NO_SPACE));
                }
            }
        }
        if !end_ok {
            let n = ss::count_spaces_before(source, right_start);
            if n > 0 {
                // ensure on same line as `]`
                if !source.as_bytes()[right_start - n..right_start].contains(&b'\n') {
                    offenses.push(space_offense(
                        ctx,
                        right_start - n,
                        right_start,
                        MSG_NO_SPACE,
                    ));
                }
            }
        }
    }

    fn space_offenses(
        &self,
        ctx: &CheckContext,
        source: &str,
        left_end: usize,
        right_start: usize,
        start_ok: bool,
        end_ok: bool,
        offenses: &mut Vec<Offense>,
    ) {
        if !start_ok {
            let n = ss::count_spaces_after(source, left_end);
            if n == 0 {
                // Not followed by newline (start_ok would handle that) and no space.
                let bytes = source.as_bytes();
                if left_end < bytes.len() && bytes[left_end] != b'\n' && bytes[left_end] != b'\r' {
                    offenses.push(space_offense(ctx, left_end - 1, left_end, MSG_SPACE));
                }
            }
        }
        if !end_ok {
            let n = ss::count_spaces_before(source, right_start);
            if n == 0 {
                let bytes = source.as_bytes();
                if right_start > 0 && bytes[right_start - 1] != b'\n' && bytes[right_start - 1] != b'\r' {
                    offenses.push(space_offense(
                        ctx,
                        right_start,
                        right_start + 1,
                        MSG_SPACE,
                    ));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compact_offenses(
        &self,
        ctx: &CheckContext,
        source: &str,
        _node_start: usize,
        left_start: usize,
        left_end: usize,
        right_start: usize,
        start_ok: bool,
        end_ok: bool,
        offenses: &mut Vec<Offense>,
    ) {
        let bytes = source.as_bytes();
        // Left side
        let left_multi = adjacent_bracket(source, left_start, AdjacentSide::Left);
        if left_multi {
            // 2D array: flag the whitespace after outer `[`. RuboCop's
            // side_space_range walks only spaces/tabs (not newlines) so a
            // newline produces a zero-width range at the `[`'s end position,
            // which Location::from_offsets reports as a 1-col-wide marker.
            let ws = is_whitespace_byte(bytes.get(left_end).copied());
            if ws == Some(true) {
                let is_newline = matches!(bytes.get(left_end), Some(&b'\n') | Some(&b'\r'));
                if is_newline {
                    offenses.push(space_offense(ctx, left_end, left_end, MSG_NO_SPACE));
                } else {
                    let n = ss::count_spaces_after(source, left_end);
                    offenses.push(space_offense(ctx, left_end, left_end + n, MSG_NO_SPACE));
                }
            }
        } else {
            // Require space
            if !start_ok {
                let n = ss::count_spaces_after(source, left_end);
                if n == 0 {
                    if left_end < bytes.len() && bytes[left_end] != b'\n' && bytes[left_end] != b'\r' {
                        offenses.push(space_offense(ctx, left_end - 1, left_end, MSG_SPACE));
                    }
                }
            }
        }
        // Right side
        let right_multi = adjacent_bracket(source, right_start, AdjacentSide::Right);
        if right_multi {
            let prev = if right_start > 0 { Some(bytes[right_start - 1]) } else { None };
            let ws = is_whitespace_byte(prev);
            if ws == Some(true) {
                let is_newline = matches!(prev, Some(b'\n') | Some(b'\r'));
                if is_newline {
                    // Zero-width range at the outer `]` itself.
                    offenses.push(space_offense(ctx, right_start, right_start, MSG_NO_SPACE));
                } else {
                    let n = ss::count_spaces_before(source, right_start);
                    offenses.push(space_offense(ctx, right_start - n, right_start, MSG_NO_SPACE));
                }
            }
        } else {
            if !end_ok {
                let n = ss::count_spaces_before(source, right_start);
                if n == 0 {
                    if right_start > 0 && bytes[right_start - 1] != b'\n' && bytes[right_start - 1] != b'\r' {
                        offenses.push(space_offense(ctx, right_start, right_start + 1, MSG_SPACE));
                    }
                }
            }
        }
    }
}

fn is_whitespace_byte(b: Option<u8>) -> Option<bool> {
    b.map(|c| c == b' ' || c == b'\t' || c == b'\n' || c == b'\r')
}

#[derive(Copy, Clone)]
enum AdjacentSide {
    Left,  // Check if there's an adjacent `[` right after this left bracket (multi-dimensional)
    Right, // Check if there's an adjacent `]` right before this right bracket
}

fn adjacent_bracket(source: &str, pos: usize, side: AdjacentSide) -> bool {
    let bytes = source.as_bytes();
    match side {
        AdjacentSide::Left => {
            // pos = offset of `[`. Look after `[`, skipping spaces and newlines.
            let mut i = pos + 1;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n' || bytes[i] == b'\r') {
                i += 1;
            }
            i < bytes.len() && bytes[i] == b'['
        }
        AdjacentSide::Right => {
            // pos = offset of `]`. Look before `]`, skipping spaces and newlines.
            let mut i = pos;
            while i > 0 {
                let c = bytes[i - 1];
                if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                    i -= 1;
                } else {
                    break;
                }
            }
            i > 0 && bytes[i - 1] == b']'
        }
    }
}

fn begins_its_line(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i > 0 {
        let c = bytes[i - 1];
        if c == b'\n' {
            return true;
        }
        if c != b' ' && c != b'\t' {
            return false;
        }
        i -= 1;
    }
    true
}

fn empty_offense(ctx: &CheckContext, start: usize, end: usize, msg: &'static str) -> Offense {
    ctx.offense_with_range(COP_NAME, msg, Severity::Convention, start, end)
}

fn space_offense(ctx: &CheckContext, start: usize, end: usize, msg: &'static str) -> Offense {
    ctx.offense_with_range(COP_NAME, msg, Severity::Convention, start, end)
}

/// Extract bracket positions from an opening location, returning `(start, end)` byte offsets.
/// Only returns Some if the opening is a single `[` character (not e.g. `%w[`).
fn extract_bracket(loc: &ruby_prism::Location) -> (usize, usize) {
    (loc.start_offset(), loc.end_offset())
}

fn is_single_bracket(source: &str, loc: &ruby_prism::Location, bracket: u8) -> bool {
    let (s, e) = extract_bracket(loc);
    e == s + 1 && source.as_bytes().get(s) == Some(&bracket)
}

impl Cop for SpaceInsideArrayLiteralBrackets {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only square-bracket arrays (skip %w[], %i[], etc.)
        let open = match node.opening_loc() {
            Some(o) => o,
            None => return vec![],
        };
        let close = match node.closing_loc() {
            Some(c) => c,
            None => return vec![],
        };
        if !is_single_bracket(ctx.source, &open, b'[') || !is_single_bracket(ctx.source, &close, b']') {
            return vec![];
        }
        let (ls, le) = extract_bracket(&open);
        let (rs, re) = extract_bracket(&close);
        let node_start = node.location().start_offset();
        self.check_brackets(ctx, node_start, ls, le, rs, re)
    }

    fn check_array_pattern(
        &self,
        node: &ruby_prism::ArrayPatternNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let open = match node.opening_loc() {
            Some(o) => o,
            None => return vec![],
        };
        let close = match node.closing_loc() {
            Some(c) => c,
            None => return vec![],
        };
        if !is_single_bracket(ctx.source, &open, b'[') || !is_single_bracket(ctx.source, &close, b']') {
            return vec![];
        }
        let (ls, le) = extract_bracket(&open);
        let (rs, re) = extract_bracket(&close);
        let node_start = node.location().start_offset();
        self.check_brackets(ctx, node_start, ls, le, rs, re)
    }
}

crate::register_cop!("Layout/SpaceInsideArrayLiteralBrackets", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/SpaceInsideArrayLiteralBrackets");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "space" => SpaceInsideArrayLiteralBracketsStyle::Space,
            "compact" => SpaceInsideArrayLiteralBracketsStyle::Compact,
            _ => SpaceInsideArrayLiteralBracketsStyle::NoSpace,
        })
        .unwrap_or(SpaceInsideArrayLiteralBracketsStyle::NoSpace);
    let empty_style = cop_config
        .and_then(|c| c.raw.get("EnforcedStyleForEmptyBrackets"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "space" => EmptyBracketsStyle::Space,
            _ => EmptyBracketsStyle::NoSpace,
        })
        .unwrap_or(EmptyBracketsStyle::NoSpace);
    Some(Box::new(SpaceInsideArrayLiteralBrackets::new(style, empty_style)))
});
