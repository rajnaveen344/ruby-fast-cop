//! Lint/EmptyExpression - Avoid empty expressions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/empty_expression.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct EmptyExpression;

impl EmptyExpression {
    pub fn new() -> Self {
        Self
    }
}

struct EmptyExpressionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for EmptyExpressionVisitor<'_> {
    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        // A ParenthesesNode with no body (None) is an empty expression `()`
        if node.body().is_none() {
            let offense = self.ctx.offense_with_range(
                "Lint/EmptyExpression",
                "Avoid empty expressions.",
                Severity::Warning,
                node.location().start_offset(),
                node.location().end_offset(),
            );
            self.offenses.push(offense);
        }
        ruby_prism::visit_parentheses_node(self, node);
    }
}

impl Cop for EmptyExpression {
    fn name(&self) -> &'static str {
        "Lint/EmptyExpression"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = EmptyExpressionVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/EmptyExpression", |_cfg| {
    Some(Box::new(EmptyExpression::new()))
});
