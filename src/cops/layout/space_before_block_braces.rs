//! Layout/SpaceBeforeBlockBraces - Checks space before opening block brace.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_before_block_braces.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

#[derive(Clone, Copy, PartialEq)]
pub enum BlockBraceStyle {
    Space,
    NoSpace,
}

pub struct SpaceBeforeBlockBraces {
    style: BlockBraceStyle,
    empty_style: BlockBraceStyle,
    /// block_delimiters_style from Style/BlockDelimiters — if line_count_based + no_space, skip multiline
    block_delimiters_style: String,
}

impl SpaceBeforeBlockBraces {
    pub fn new(style: BlockBraceStyle, empty_style: BlockBraceStyle, block_delimiters_style: String) -> Self {
        Self { style, empty_style, block_delimiters_style }
    }
}

impl Default for SpaceBeforeBlockBraces {
    fn default() -> Self {
        Self {
            style: BlockBraceStyle::Space,
            empty_style: BlockBraceStyle::Space,
            block_delimiters_style: String::new(),
        }
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a SpaceBeforeBlockBraces,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_block(&mut self, opening_loc: ruby_prism::Location, is_empty: bool, is_multiline: bool) {
        let source = self.ctx.source;
        let brace_start = opening_loc.start_offset();

        // Conflict check: no_space + line_count_based + multiline -> skip
        if self.cop.style == BlockBraceStyle::NoSpace
            && self.cop.block_delimiters_style == "line_count_based"
            && is_multiline
        {
            return;
        }

        let effective_style = if is_empty { self.cop.empty_style } else { self.cop.style };

        // Check character before brace
        let bytes = source.as_bytes();
        if brace_start == 0 {
            return;
        }

        // Find the char immediately before the brace
        let prev_byte = bytes[brace_start - 1];
        let has_space = prev_byte == b' ' || prev_byte == b'\t';

        match effective_style {
            BlockBraceStyle::Space => {
                if !has_space {
                    // Space missing to the left of {
                    let offense = Offense::new(
                        "Layout/SpaceBeforeBlockBraces",
                        "Space missing to the left of {.",
                        Severity::Convention,
                        Location::from_offsets(source, brace_start, brace_start + 1),
                        self.ctx.filename,
                    ).with_correction(Correction::insert(brace_start, " "));
                    self.offenses.push(offense);
                }
            }
            BlockBraceStyle::NoSpace => {
                if has_space {
                    // Find start of the space run before brace
                    let mut space_start = brace_start - 1;
                    let space_bytes = source.as_bytes();
                    while space_start > 0 && (space_bytes[space_start - 1] == b' ' || space_bytes[space_start - 1] == b'\t') {
                        space_start -= 1;
                    }
                    let offense = Offense::new(
                        "Layout/SpaceBeforeBlockBraces",
                        "Space detected to the left of {.",
                        Severity::Convention,
                        Location::from_offsets(source, space_start, brace_start),
                        self.ctx.filename,
                    ).with_correction(Correction::delete(space_start, brace_start));
                    self.offenses.push(offense);
                }
            }
        }
    }
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'a>) {
        let src = self.ctx.source;
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        // Only check brace lambdas (not do-end)
        if src.as_bytes().get(opening.start_offset()).copied() == Some(b'{') {
            let is_empty = opening.end_offset() == closing.start_offset();
            let is_multiline = {
                let start_line = crate::helpers::source::line_at_offset(src, opening.start_offset());
                let end_line = crate::helpers::source::line_at_offset(src, closing.start_offset());
                start_line != end_line
            };
            self.check_block(opening, is_empty, is_multiline);
        }
        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        let is_empty = opening.end_offset() == closing.start_offset();
        let is_multiline = {
            let src = self.ctx.source;
            let start_line = crate::helpers::source::line_at_offset(src, opening.start_offset());
            let end_line = crate::helpers::source::line_at_offset(src, closing.start_offset());
            start_line != end_line
        };
        // Only check brace blocks (not do...end)
        let src = self.ctx.source;
        let brace_byte = src.as_bytes().get(opening.start_offset()).copied();
        if brace_byte == Some(b'{') {
            self.check_block(opening, is_empty, is_multiline);
        }
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_it_parameters_node(&mut self, node: &ruby_prism::ItParametersNode<'a>) {
        ruby_prism::visit_it_parameters_node(self, node);
    }
}

impl Cop for SpaceBeforeBlockBraces {
    fn name(&self) -> &'static str {
        "Layout/SpaceBeforeBlockBraces"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, cop: self, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

crate::register_cop!("Layout/SpaceBeforeBlockBraces", |cfg| {
    let cop_cfg = cfg.get_cop_config("Layout/SpaceBeforeBlockBraces");
    let style = cop_cfg
        .as_ref()
        .and_then(|c| c.enforced_style.as_deref())
        .map(|s| if s == "no_space" { BlockBraceStyle::NoSpace } else { BlockBraceStyle::Space })
        .unwrap_or(BlockBraceStyle::Space);

    let empty_style = cop_cfg
        .as_ref()
        .and_then(|c| c.raw.get("EnforcedStyleForEmptyBraces"))
        .and_then(|v| v.as_str())
        .map(|s| if s == "no_space" { BlockBraceStyle::NoSpace } else { BlockBraceStyle::Space })
        .unwrap_or(style);

    let block_delimiters_style = cfg
        .get_cop_config("Style/BlockDelimiters")
        .and_then(|c| c.enforced_style.clone())
        .unwrap_or_default();

    Some(Box::new(SpaceBeforeBlockBraces::new(style, empty_style, block_delimiters_style)))
});
