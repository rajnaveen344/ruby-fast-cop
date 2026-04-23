//! Lint/RegexpAsCondition cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/regexp_as_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct RegexpAsCondition;

impl RegexpAsCondition {
    pub fn new() -> Self { Self }
}

const MSG: &str = "Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.";

impl Cop for RegexpAsCondition {
    fn name(&self) -> &'static str { "Lint/RegexpAsCondition" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RegexpCondVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RegexpCondVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RegexpCondVisitor<'a> {
    fn check_node_as_condition(&mut self, node: Node) {
        match &node {
            Node::MatchLastLineNode { .. } => {
                let loc = node.location();
                let src = self.ctx.src(loc.start_offset(), loc.end_offset());
                let correction = Correction::replace(
                    loc.start_offset(),
                    loc.end_offset(),
                    format!("{} =~ $_", src),
                );
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/RegexpAsCondition",
                    MSG,
                    Severity::Warning,
                    loc.start_offset(),
                    loc.end_offset(),
                ).with_correction(correction));
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = node_name!(call);
                // Handle !regexp case
                if method == "!" {
                    if let Some(recv) = call.receiver() {
                        self.check_node_as_condition(recv);
                    }
                }
            }
            _ => {}
        }
    }
}

impl<'a> Visit<'_> for RegexpCondVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_node_as_condition(node.predicate());
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_node_as_condition(node.predicate());
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        self.check_node_as_condition(node.predicate());
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.check_node_as_condition(node.predicate());
        ruby_prism::visit_until_node(self, node);
    }
}

crate::register_cop!("Lint/RegexpAsCondition", |_cfg| {
    Some(Box::new(RegexpAsCondition::new()))
});
