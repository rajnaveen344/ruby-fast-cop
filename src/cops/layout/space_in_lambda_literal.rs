//! Layout/SpaceInLambdaLiteral - Checks space between -> and ( in lambda literals.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_in_lambda_literal.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

#[derive(Clone, Copy, PartialEq)]
pub enum LambdaSpaceStyle {
    RequireSpace,
    RequireNoSpace,
}

pub struct SpaceInLambdaLiteral {
    style: LambdaSpaceStyle,
}

impl SpaceInLambdaLiteral {
    pub fn new(style: LambdaSpaceStyle) -> Self {
        Self { style }
    }
}

impl Default for SpaceInLambdaLiteral {
    fn default() -> Self {
        Self { style: LambdaSpaceStyle::RequireNoSpace }
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: LambdaSpaceStyle,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'a>) {
        let source = self.ctx.source;
        let bytes = source.as_bytes();

        // Only care about arrow lambdas with parenthesized params: ->(...)
        // LambdaNode has operator_loc (the ->)
        let operator_loc = node.operator_loc();
        let params = node.parameters();

        // Check if params is a BlockParametersNode (parenthesized)
        if let Some(params_node) = params {
            // Cast to BlockParametersNode to get opening_loc
            let params_start = if let Some(bp) = params_node.as_block_parameters_node() {
                bp.opening_loc()
                    .map(|l| l.start_offset())
                    .unwrap_or_else(|| params_node.location().start_offset())
            } else {
                params_node.location().start_offset()
            };

            // Only handle parenthesized params (opening is `(`)
            if bytes.get(params_start).copied() != Some(b'(') {
                ruby_prism::visit_lambda_node(self, node);
                return;
            }

            // arrow end = operator_loc.end_offset() (right after ->)
            let arrow_end = operator_loc.end_offset();
            // space between arrow and (
            let space_count = if params_start >= arrow_end { params_start - arrow_end } else { 0 };
            let has_space = space_count > 0;

            match self.style {
                LambdaSpaceStyle::RequireSpace => {
                    if !has_space {
                        // Offense: lambda start to params end
                        let params_end = params_node.location().end_offset();
                        let lambda_start = node.location().start_offset();
                        let correction = Correction::insert(arrow_end, " ");
                        self.offenses.push(
                            Offense::new(
                                "Layout/SpaceInLambdaLiteral",
                                "Use a space between `->` and `(` in lambda literals.",
                                Severity::Convention,
                                Location::from_offsets(source, lambda_start, params_end),
                                self.ctx.filename,
                            ).with_correction(correction)
                        );
                    }
                }
                LambdaSpaceStyle::RequireNoSpace => {
                    if has_space {
                        // Offense: the space range (arrow_end to params_start)
                        let correction = Correction::delete(arrow_end, params_start);
                        self.offenses.push(
                            Offense::new(
                                "Layout/SpaceInLambdaLiteral",
                                "Do not use spaces between `->` and `(` in lambda literals.",
                                Severity::Convention,
                                Location::from_offsets(source, arrow_end, params_start),
                                self.ctx.filename,
                            ).with_correction(correction)
                        );
                    }
                }
            }
        }

        ruby_prism::visit_lambda_node(self, node);
    }
}

impl Cop for SpaceInLambdaLiteral {
    fn name(&self) -> &'static str {
        "Layout/SpaceInLambdaLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, style: self.style, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

crate::register_cop!("Layout/SpaceInLambdaLiteral", |cfg| {
    let style = cfg
        .get_cop_config("Layout/SpaceInLambdaLiteral")
        .and_then(|c| c.enforced_style.as_deref())
        .map(|s| match s {
            "require_space" => LambdaSpaceStyle::RequireSpace,
            _ => LambdaSpaceStyle::RequireNoSpace,
        })
        .unwrap_or(LambdaSpaceStyle::RequireNoSpace);
    Some(Box::new(SpaceInLambdaLiteral::new(style)))
});
