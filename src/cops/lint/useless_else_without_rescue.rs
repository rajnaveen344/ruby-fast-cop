//! Lint/UselessElseWithoutRescue cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/useless_else_without_rescue.rb
//!
//! NOTE: This syntax is no longer valid on Ruby 2.6 or higher.
//! The cop only fires when target_ruby_version <= 2.5.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct UselessElseWithoutRescue;

impl UselessElseWithoutRescue {
    pub fn new() -> Self { Self }
}

const MSG: &str = "`else` without `rescue` is useless.";

impl Cop for UselessElseWithoutRescue {
    fn name(&self) -> &'static str { "Lint/UselessElseWithoutRescue" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only applies to Ruby <= 2.5
        if ctx.target_ruby_version > 2.5 {
            return vec![];
        }
        let mut visitor = ElseVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct ElseVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for ElseVisitor<'a> {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        // Has else clause but no rescue clause
        if node.else_clause().is_some() && node.rescue_clause().is_none() {
            let else_node = node.else_clause().unwrap();
            let kw_loc = else_node.else_keyword_loc();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/UselessElseWithoutRescue",
                MSG,
                Severity::Warning,
                kw_loc.start_offset(),
                kw_loc.end_offset(),
            ));
        }
        ruby_prism::visit_begin_node(self, node);
    }
}

crate::register_cop!("Lint/UselessElseWithoutRescue", |_cfg| {
    Some(Box::new(UselessElseWithoutRescue::new()))
});
