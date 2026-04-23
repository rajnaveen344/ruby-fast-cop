//! Lint/DuplicateCaseCondition cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_case_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct DuplicateCaseCondition;

impl DuplicateCaseCondition {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateCaseCondition {
    fn name(&self) -> &'static str { "Lint/DuplicateCaseCondition" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = CaseVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct CaseVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for CaseVisitor<'a> {
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        let mut seen: Vec<String> = Vec::new();
        for condition in node.conditions().iter() {
            if let Some(when_node) = condition.as_when_node() {
                for cond in when_node.conditions().iter() {
                    let loc = cond.location();
                    let src = self.ctx.src(loc.start_offset(), loc.end_offset()).to_string();
                    if seen.contains(&src) {
                        self.offenses.push(self.ctx.offense_with_range(
                            "Lint/DuplicateCaseCondition",
                            "Duplicate `when` condition detected.",
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
        ruby_prism::visit_case_node(self, node);
    }
}

crate::register_cop!("Lint/DuplicateCaseCondition", |_cfg| {
    Some(Box::new(DuplicateCaseCondition::new()))
});
