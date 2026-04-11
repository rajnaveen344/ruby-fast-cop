//! Layout/SpaceInsideReferenceBrackets
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/space_inside_reference_brackets.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::surrounding_space as ss;
use crate::node_name;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/SpaceInsideReferenceBrackets";
const MSG_NO_SPACE: &str = "Do not use space inside reference brackets.";
const MSG_SPACE: &str = "Use space inside reference brackets.";
const MSG_EMPTY_NO_SPACE: &str = "Do not use space inside empty reference brackets.";
const MSG_EMPTY_SPACE_ONE: &str = "Use one space inside empty reference brackets.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceInsideReferenceBracketsStyle {
    NoSpace,
    Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceEmptyBracketsStyle {
    NoSpace,
    Space,
}

pub struct SpaceInsideReferenceBrackets {
    style: SpaceInsideReferenceBracketsStyle,
    empty_style: ReferenceEmptyBracketsStyle,
}

impl SpaceInsideReferenceBrackets {
    pub fn new(
        style: SpaceInsideReferenceBracketsStyle,
        empty_style: ReferenceEmptyBracketsStyle,
    ) -> Self {
        Self { style, empty_style }
    }
}

impl Cop for SpaceInsideReferenceBrackets {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Must be `[]` or `[]=` method
        let method = node_name!(node);
        if method != "[]" && method != "[]=" {
            return vec![];
        }
        // Opening `[` and closing `]` must exist
        let open = match node.opening_loc() {
            Some(o) => o,
            None => return vec![],
        };
        let close = match node.closing_loc() {
            Some(c) => c,
            None => return vec![],
        };
        // Must actually be single `[` / `]` characters (not `(`)
        let source = ctx.source;
        let (ls, le) = (open.start_offset(), open.end_offset());
        let (rs, re) = (close.start_offset(), close.end_offset());
        if le != ls + 1 || source.as_bytes().get(ls) != Some(&b'[') {
            return vec![];
        }
        if re != rs + 1 || source.as_bytes().get(rs) != Some(&b']') {
            return vec![];
        }

        let mut offenses = Vec::new();

        // Empty brackets?
        if ss::is_empty_between(source, le, rs) {
            match self.empty_style {
                ReferenceEmptyBracketsStyle::NoSpace => {
                    if !ss::no_character_between(le, rs) {
                        offenses.push(ctx.offense_with_range(
                            COP_NAME,
                            MSG_EMPTY_NO_SPACE,
                            Severity::Convention,
                            ls,
                            re,
                        ));
                    }
                }
                ReferenceEmptyBracketsStyle::Space => {
                    if !ss::has_exactly_one_space(source, le, rs) {
                        offenses.push(ctx.offense_with_range(
                            COP_NAME,
                            MSG_EMPTY_SPACE_ONE,
                            Severity::Convention,
                            ls,
                            re,
                        ));
                    }
                }
            }
            return offenses;
        }

        // Multiline non-empty: no check
        if !ss::same_line(source, le, rs) {
            return offenses;
        }

        match self.style {
            SpaceInsideReferenceBracketsStyle::NoSpace => {
                let n = ss::count_spaces_after(source, le);
                if n > 0 {
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        MSG_NO_SPACE,
                        Severity::Convention,
                        le,
                        le + n,
                    ));
                }
                let n = ss::count_spaces_before(source, rs);
                if n > 0 {
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        MSG_NO_SPACE,
                        Severity::Convention,
                        rs - n,
                        rs,
                    ));
                }
            }
            SpaceInsideReferenceBracketsStyle::Space => {
                let n = ss::count_spaces_after(source, le);
                if n == 0 {
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        MSG_SPACE,
                        Severity::Convention,
                        le - 1,
                        le,
                    ));
                }
                let n = ss::count_spaces_before(source, rs);
                if n == 0 {
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        MSG_SPACE,
                        Severity::Convention,
                        rs,
                        rs + 1,
                    ));
                }
            }
        }

        offenses
    }
}
