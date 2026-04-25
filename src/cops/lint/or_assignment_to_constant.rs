//! Lint/OrAssignmentToConstant cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Avoid using or-assignment with constants.";

#[derive(Default)]
pub struct OrAssignmentToConstant;

impl OrAssignmentToConstant {
    pub fn new() -> Self { Self }
}

impl Cop for OrAssignmentToConstant {
    fn name(&self) -> &'static str { "Lint/OrAssignmentToConstant" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    out: Vec<Offense>,
}

impl<'a, 'b> V<'a, 'b> {
    fn emit(&mut self, op_start: usize, op_end: usize) {
        let off = self.ctx.offense_with_range(
            "Lint/OrAssignmentToConstant", MSG, Severity::Warning,
            op_start, op_end,
        ).with_correction(Correction::replace(op_start, op_end, "="));
        self.out.push(off);
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        let op = node.operator_loc();
        self.emit(op.start_offset(), op.end_offset());
        ruby_prism::visit_constant_or_write_node(self, node);
    }
    fn visit_constant_path_or_write_node(&mut self, node: &ruby_prism::ConstantPathOrWriteNode) {
        let op = node.operator_loc();
        self.emit(op.start_offset(), op.end_offset());
        ruby_prism::visit_constant_path_or_write_node(self, node);
    }
}

crate::register_cop!("Lint/OrAssignmentToConstant", |_cfg| Some(Box::new(OrAssignmentToConstant::new())));
