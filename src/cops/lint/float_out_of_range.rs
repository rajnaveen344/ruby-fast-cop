//! Lint/FloatOutOfRange cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/float_out_of_range.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct FloatOutOfRange;

impl FloatOutOfRange {
    pub fn new() -> Self { Self }
}

impl Cop for FloatOutOfRange {
    fn name(&self) -> &'static str { "Lint/FloatOutOfRange" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = FloatVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct FloatVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> ruby_prism::Visit<'a> for FloatVisitor<'a> {
    fn visit_float_node(&mut self, node: &ruby_prism::FloatNode) {
        let val = node.value();
        let loc = node.location();
        let src = self.ctx.src(loc.start_offset(), loc.end_offset());
        // Flag: infinite (out of range large) or zero from non-zero source (underflow)
        let out_of_range = val.is_infinite() || (val == 0.0 && src.bytes().any(|b| matches!(b, b'1'..=b'9')));
        if out_of_range {
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/FloatOutOfRange",
                "Float out of range.",
                Severity::Warning,
                loc.start_offset(),
                loc.end_offset(),
            ));
        }
    }
}

crate::register_cop!("Lint/FloatOutOfRange", |_cfg| {
    Some(Box::new(FloatOutOfRange::new()))
});
