//! Layout/SpaceInsideHashLiteralBraces
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/space_inside_hash_literal_braces.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::surrounding_space as ss;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/SpaceInsideHashLiteralBraces";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceInsideHashLiteralBracesStyle {
    Space,
    NoSpace,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashEmptyBracesStyle {
    Space,
    NoSpace,
}

pub struct SpaceInsideHashLiteralBraces {
    style: SpaceInsideHashLiteralBracesStyle,
    empty_style: HashEmptyBracesStyle,
}

impl SpaceInsideHashLiteralBraces {
    pub fn new(
        style: SpaceInsideHashLiteralBracesStyle,
        empty_style: HashEmptyBracesStyle,
    ) -> Self {
        Self { style, empty_style }
    }

    /// Returns Some((left_start, right_start)) of the brace byte offsets, or None if
    /// the hash isn't surrounded by literal `{...}`.
    fn braces(
        source: &str,
        open: &ruby_prism::Location,
        close: &ruby_prism::Location,
    ) -> Option<(usize, usize)> {
        let ls = open.start_offset();
        let le = open.end_offset();
        let rs = close.start_offset();
        let re = close.end_offset();
        if le != ls + 1 || source.as_bytes().get(ls) != Some(&b'{') {
            return None;
        }
        if re != rs + 1 || source.as_bytes().get(rs) != Some(&b'}') {
            return None;
        }
        Some((ls, rs))
    }

    fn check_braces(
        &self,
        ctx: &CheckContext,
        left: usize,
        right: usize,
    ) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;

        // Whitespace-only braces? e.g. `{}` or `{ }` or `{\n}`.
        if ss::is_empty_between(source, left + 1, right) {
            self.check_empty(ctx, left, right, &mut offenses);
            return offenses;
        }

        // Left side: check between `{` (at left) and first content byte (at left+1+n_space_after)
        self.check_left(ctx, left, right, &mut offenses);
        // Right side: check between last content byte and `}` (at right)
        self.check_right(ctx, left, right, &mut offenses);

        offenses
    }

    fn check_empty(
        &self,
        ctx: &CheckContext,
        left: usize,
        right: usize,
        offenses: &mut Vec<Offense>,
    ) {
        let source = ctx.source;
        let is_multiline = !ss::same_line(source, left, right);

        match self.empty_style {
            HashEmptyBracesStyle::NoSpace => {
                // Flag if there is any whitespace (space/tab/newline) inside the braces.
                // Range is the inner whitespace (left+1..right) — RuboCop's
                // `range_between(left_brace.end_pos, right_brace.begin_pos)`.
                if right > left + 1 {
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        "Space inside empty hash literal braces detected.",
                        Severity::Convention,
                        left + 1,
                        right,
                    ));
                }
            }
            HashEmptyBracesStyle::Space => {
                // Flag if no space at all, or if multiline (which is neither space nor no-space).
                // RuboCop's incorrect_style_detected uses `range = brace` (just `{`) when
                // expect_space=true and actual=no_space.
                if right == left + 1 || is_multiline {
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        "Space inside empty hash literal braces missing.",
                        Severity::Convention,
                        left,
                        left + 1,
                    ));
                }
            }
        }
    }

    fn check_left(
        &self,
        ctx: &CheckContext,
        left: usize,
        right: usize,
        offenses: &mut Vec<Offense>,
    ) {
        let source = ctx.source;
        let bytes = source.as_bytes();
        // Find the first non-space byte after `{`
        let mut i = left + 1;
        let mut saw_newline = false;
        while i < right && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n' || bytes[i] == b'\r') {
            if bytes[i] == b'\n' || bytes[i] == b'\r' {
                saw_newline = true;
            }
            i += 1;
        }
        if i >= right {
            return;
        }
        // If a newline sits between `{` and the first content, skip (multiline hash)
        if saw_newline {
            return;
        }
        // RuboCop: `return if token2.comment?` — skip if first content is a comment.
        if bytes[i] == b'#' {
            return;
        }
        let has_space = i > left + 1;

        // Determine expected style
        // is_same_braces: compared against the next token; here next is content (not a brace).
        // So is_same_braces = false in general.
        // BUT the Ruby code checks token1.type == token2.type, and if content is `{` (nested hash),
        // then both tokens are `{` → is_same_braces = true.
        let is_same_braces = bytes[i] == b'{';
        let expect_space = if is_same_braces && self.style == SpaceInsideHashLiteralBracesStyle::Compact {
            false
        } else {
            self.style != SpaceInsideHashLiteralBracesStyle::NoSpace
        };

        if expect_space && !has_space {
            offenses.push(ctx.offense_with_range(
                COP_NAME,
                "Space inside { missing.",
                Severity::Convention,
                left,
                left + 1,
            ));
        } else if !expect_space && has_space {
            // Range is the space to the right of `{`
            offenses.push(ctx.offense_with_range(
                COP_NAME,
                "Space inside { detected.",
                Severity::Convention,
                left + 1,
                i,
            ));
        }
    }

    fn check_right(
        &self,
        ctx: &CheckContext,
        left: usize,
        right: usize,
        offenses: &mut Vec<Offense>,
    ) {
        let source = ctx.source;
        let bytes = source.as_bytes();
        // Find the last non-space byte before `}`
        let mut i = right;
        let mut saw_newline = false;
        while i > left + 1 {
            let c = bytes[i - 1];
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                if c == b'\n' || c == b'\r' {
                    saw_newline = true;
                }
                i -= 1;
            } else {
                break;
            }
        }
        if i <= left + 1 {
            return;
        }
        if saw_newline {
            return;
        }
        let has_space = i < right;
        // Check if previous char is `}` → is_same_braces
        let is_same_braces = bytes[i - 1] == b'}';
        let expect_space = if is_same_braces && self.style == SpaceInsideHashLiteralBracesStyle::Compact {
            false
        } else {
            self.style != SpaceInsideHashLiteralBracesStyle::NoSpace
        };

        if expect_space && !has_space {
            offenses.push(ctx.offense_with_range(
                COP_NAME,
                "Space inside } missing.",
                Severity::Convention,
                right,
                right + 1,
            ));
        } else if !expect_space && has_space {
            // Range is the space to the left of `}`
            offenses.push(ctx.offense_with_range(
                COP_NAME,
                "Space inside } detected.",
                Severity::Convention,
                i,
                right,
            ));
        }
    }
}

impl Cop for SpaceInsideHashLiteralBraces {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        let open = node.opening_loc();
        let close = node.closing_loc();
        match Self::braces(ctx.source, &open, &close) {
            Some((l, r)) => self.check_braces(ctx, l, r),
            None => vec![],
        }
    }

    fn check_hash_pattern(
        &self,
        node: &ruby_prism::HashPatternNode,
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
        match Self::braces(ctx.source, &open, &close) {
            Some((l, r)) => self.check_braces(ctx, l, r),
            None => vec![],
        }
    }
}

crate::register_cop!("Layout/SpaceInsideHashLiteralBraces", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/SpaceInsideHashLiteralBraces");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "no_space" => SpaceInsideHashLiteralBracesStyle::NoSpace,
            "compact" => SpaceInsideHashLiteralBracesStyle::Compact,
            _ => SpaceInsideHashLiteralBracesStyle::Space,
        })
        .unwrap_or(SpaceInsideHashLiteralBracesStyle::Space);
    let empty_style = cop_config
        .and_then(|c| c.raw.get("EnforcedStyleForEmptyBraces"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "space" => HashEmptyBracesStyle::Space,
            _ => HashEmptyBracesStyle::NoSpace,
        })
        .unwrap_or(HashEmptyBracesStyle::NoSpace);
    Some(Box::new(SpaceInsideHashLiteralBraces::new(style, empty_style)))
});
