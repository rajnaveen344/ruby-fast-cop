//! Lint/FlipFlop cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/flip_flop.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct FlipFlop;

impl FlipFlop {
    pub fn new() -> Self { Self }
}

impl Cop for FlipFlop {
    fn name(&self) -> &'static str { "Lint/FlipFlop" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = FlipFlopVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct FlipFlopVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for FlipFlopVisitor<'a> {
    fn visit_flip_flop_node(&mut self, node: &ruby_prism::FlipFlopNode) {
        let loc = node.location();
        self.offenses.push(self.ctx.offense_with_range(
            "Lint/FlipFlop",
            "Avoid the use of flip-flop operators.",
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        ));
        ruby_prism::visit_flip_flop_node(self, node);
    }
}

crate::register_cop!("Lint/FlipFlop", |_cfg| {
    Some(Box::new(FlipFlop::new()))
});
