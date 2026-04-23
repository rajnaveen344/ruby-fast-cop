//! Lint/DuplicateRescueException cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_rescue_exception.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct DuplicateRescueException;

impl DuplicateRescueException {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateRescueException {
    fn name(&self) -> &'static str { "Lint/DuplicateRescueException" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RescueVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RescueVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RescueVisitor<'a> {
    fn check_rescue_chain(&mut self, first: &ruby_prism::RescueNode) {
        let mut seen: Vec<String> = Vec::new();
        self.check_one_rescue(first, &mut seen);
        let mut next = first.subsequent();
        while let Some(rescue) = next {
            self.check_one_rescue(&rescue, &mut seen);
            next = rescue.subsequent();
        }
    }

    fn check_one_rescue(&mut self, rescue: &ruby_prism::RescueNode, seen: &mut Vec<String>) {
        for exc in rescue.exceptions().iter() {
            let loc = exc.location();
            let src = self.ctx.src(loc.start_offset(), loc.end_offset()).to_string();
            if seen.contains(&src) {
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/DuplicateRescueException",
                    "Duplicate `rescue` exception detected.",
                    Severity::Warning,
                    loc.start_offset(),
                    loc.end_offset(),
                ));
            } else {
                seen.push(src);
            }
        }
    }
}

impl<'a> Visit<'_> for RescueVisitor<'a> {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(rescue) = node.rescue_clause() {
            self.check_rescue_chain(&rescue);
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // DefNode wraps body in BeginNode when rescue is present, so
        // rescue is handled transitively via visit_begin_node
        ruby_prism::visit_def_node(self, node);
    }
}

crate::register_cop!("Lint/DuplicateRescueException", |_cfg| {
    Some(Box::new(DuplicateRescueException::new()))
});
