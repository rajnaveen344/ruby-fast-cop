//! Style/VariableInterpolation - Flags shorthand variable interpolation `"#@x"`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::interpolation::embedded_variable_parts;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct VariableInterpolation;

impl VariableInterpolation {
    pub fn new() -> Self { Self }
}

impl Cop for VariableInterpolation {
    fn name(&self) -> &'static str { "Style/VariableInterpolation" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = VariableInterpolationVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct VariableInterpolationVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> VariableInterpolationVisitor<'a> {
    fn handle(&mut self, node: &Node<'_>) {
        for embed in embedded_variable_parts(node) {
            let var = embed.variable();
            let loc = var.location();
            let start = loc.start_offset();
            let end = loc.end_offset();
            let source = &self.ctx.source[start..end];
            let msg = format!(
                "Replace interpolated variable `{source}` with expression `#{{{source}}}`."
            );
            let offense = self
                .ctx
                .offense_with_range("Style/VariableInterpolation", &msg, Severity::Convention, start, end)
                .with_correction(Correction::replace(start, end, format!("{{{source}}}")));
            self.offenses.push(offense);
        }
    }
}

impl Visit<'_> for VariableInterpolationVisitor<'_> {
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

crate::register_cop!("Style/VariableInterpolation", |_cfg| {
    Some(Box::new(VariableInterpolation::new()))
});
