//! Lint/EmptyEnsure cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/empty_ensure.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;
use ruby_prism::Visit;

#[derive(Default)]
pub struct EmptyEnsure;

impl EmptyEnsure {
    pub fn new() -> Self { Self }
}

impl Cop for EmptyEnsure {
    fn name(&self) -> &'static str { "Lint/EmptyEnsure" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = EnsureVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EnsureVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for EnsureVisitor<'a> {
    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode) {
        if node.statements().is_none() {
            let kw_loc = node.ensure_keyword_loc();
            // Correction: remove just the `ensure` keyword token (RuboCop removes keyword only)
            let correction = Correction::delete(kw_loc.start_offset(), kw_loc.end_offset());
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/EmptyEnsure",
                "Empty `ensure` block detected.",
                Severity::Warning,
                kw_loc.start_offset(),
                kw_loc.end_offset(),
            ).with_correction(correction));
        }
        ruby_prism::visit_ensure_node(self, node);
    }
}

crate::register_cop!("Lint/EmptyEnsure", |_cfg| {
    Some(Box::new(EmptyEnsure::new()))
});
