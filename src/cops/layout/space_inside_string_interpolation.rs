//! Layout/SpaceInsideStringInterpolation - checks whitespace inside `#{...}`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::interpolation::embedded_statements_parts;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_NO_SPACE: &str = "Do not use space inside string interpolation.";
const MSG_SPACE: &str = "Use space inside string interpolation.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    NoSpace,
    Space,
}

impl Default for EnforcedStyle {
    fn default() -> Self { EnforcedStyle::NoSpace }
}

pub struct SpaceInsideStringInterpolation {
    style: EnforcedStyle,
}

impl SpaceInsideStringInterpolation {
    pub fn new(style: EnforcedStyle) -> Self { Self { style } }
}

impl Default for SpaceInsideStringInterpolation {
    fn default() -> Self { Self::new(EnforcedStyle::NoSpace) }
}

impl Cop for SpaceInsideStringInterpolation {
    fn name(&self) -> &'static str { "Layout/SpaceInsideStringInterpolation" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, style: self.style, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn handle(&mut self, node: &Node<'_>) {
        for begin in embedded_statements_parts(node) {
            self.check_begin(&begin);
        }
    }

    fn check_begin(&mut self, begin: &ruby_prism::EmbeddedStatementsNode<'_>) {
        let opening = begin.opening_loc();
        let closing = begin.closing_loc();
        let open_start = opening.start_offset();
        let open_end = opening.end_offset();
        let close_start = closing.start_offset();
        let close_end = closing.end_offset();

        let src = self.ctx.source;
        // Skip multiline interpolations.
        if src[open_start..close_end].contains('\n') {
            return;
        }

        let content = &src[open_end..close_start];
        let leading = content.bytes().take_while(|b| *b == b' ' || *b == b'\t').count();
        let trailing_ws = content
            .bytes()
            .rev()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        let content_all_ws = leading == content.len();
        // Empty brackets: either truly empty or only whitespace (no content tokens).
        if content_all_ws {
            return;
        }

        match self.style {
            EnforcedStyle::NoSpace => {
                if leading > 0 {
                    let s = open_end;
                    let e = open_end + leading;
                    self.offenses.push(
                        self.ctx
                            .offense_with_range(
                                "Layout/SpaceInsideStringInterpolation",
                                MSG_NO_SPACE,
                                Severity::Convention,
                                s,
                                e,
                            )
                            .with_correction(Correction::delete(s, e)),
                    );
                }
                if trailing_ws > 0 {
                    let s = close_start - trailing_ws;
                    let e = close_start;
                    self.offenses.push(
                        self.ctx
                            .offense_with_range(
                                "Layout/SpaceInsideStringInterpolation",
                                MSG_NO_SPACE,
                                Severity::Convention,
                                s,
                                e,
                            )
                            .with_correction(Correction::delete(s, e)),
                    );
                }
            }
            EnforcedStyle::Space => {
                if leading == 0 {
                    self.offenses.push(
                        self.ctx
                            .offense_with_range(
                                "Layout/SpaceInsideStringInterpolation",
                                MSG_SPACE,
                                Severity::Convention,
                                open_start,
                                open_end,
                            )
                            .with_correction(Correction::insert(open_end, " ")),
                    );
                }
                if trailing_ws == 0 {
                    self.offenses.push(
                        self.ctx
                            .offense_with_range(
                                "Layout/SpaceInsideStringInterpolation",
                                MSG_SPACE,
                                Severity::Convention,
                                close_start,
                                close_end,
                            )
                            .with_correction(Correction::insert(close_start, " ")),
                    );
                }
            }
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_symbol_node(self, node);
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }
}
