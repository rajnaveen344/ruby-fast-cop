//! Lint/ConstantOverwrittenInRescue cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct ConstantOverwrittenInRescue;

impl ConstantOverwrittenInRescue {
    pub fn new() -> Self { Self }
}

impl Cop for ConstantOverwrittenInRescue {
    fn name(&self) -> &'static str { "Lint/ConstantOverwrittenInRescue" }
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

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        if let (Some(op), Some(r)) = (node.operator_loc(), node.reference()) {
            let is_const = r.as_constant_read_node().is_some()
                || r.as_constant_path_node().is_some()
                || r.as_constant_target_node().is_some()
                || r.as_constant_path_target_node().is_some()
                || r.as_constant_write_node().is_some()
                || r.as_constant_path_write_node().is_some();
            if is_const {
                let rloc = r.location();
                let const_text = &self.ctx.source[rloc.start_offset()..rloc.end_offset()];
                let msg = format!("`{}` is overwritten by `rescue =>`.", const_text);
                let off = self.ctx.offense_with_range(
                    "Lint/ConstantOverwrittenInRescue", &msg, Severity::Warning,
                    op.start_offset(), op.end_offset(),
                ).with_correction(Correction::delete(op.start_offset(), rloc.start_offset()));
                self.out.push(off);
            }
        }
        ruby_prism::visit_rescue_node(self, node);
    }
}

crate::register_cop!("Lint/ConstantOverwrittenInRescue", |_cfg| Some(Box::new(ConstantOverwrittenInRescue::new())));
