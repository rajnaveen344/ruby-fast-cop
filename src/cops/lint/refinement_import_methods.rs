//! Lint/RefinementImportMethods cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct RefinementImportMethods;

impl RefinementImportMethods {
    pub fn new() -> Self { Self }
}

impl Cop for RefinementImportMethods {
    fn name(&self) -> &'static str { "Lint/RefinementImportMethods" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 1) { return vec![]; }
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, in_refine: 0, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    in_refine: usize,
    out: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name = node_name!(node);
        let is_refine = name.as_ref() == "refine" && node.block().is_some();
        if self.in_refine > 0 && (name.as_ref() == "include" || name.as_ref() == "prepend") && node.receiver().is_none() {
            if let Some(mloc) = node.message_loc() {
                let msg = format!("Use `import_methods` instead of `{}` because it is deprecated in Ruby 3.1.", name.as_ref());
                self.out.push(self.ctx.offense_with_range(
                    "Lint/RefinementImportMethods", &msg, Severity::Warning,
                    mloc.start_offset(), mloc.end_offset(),
                ));
            }
        }
        if is_refine { self.in_refine += 1; }
        ruby_prism::visit_call_node(self, node);
        if is_refine { self.in_refine -= 1; }
    }
}

crate::register_cop!("Lint/RefinementImportMethods", |_cfg| Some(Box::new(RefinementImportMethods::new())));
