//! Lint/IdentityComparison - Use `equal?` instead of `==` when comparing object_id.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/identity_comparison.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node, Visit};

#[derive(Default)]
pub struct IdentityComparison;

impl IdentityComparison {
    pub fn new() -> Self {
        Self
    }

    /// Returns receiver source if node is `something.object_id` with a receiver.
    fn object_id_receiver<'a>(node: &Node<'a>, source: &'a str) -> Option<&'a str> {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            let method = node_name!(call);
            if method == "object_id" {
                if let Some(recv) = call.receiver() {
                    let loc = recv.location();
                    return Some(&source[loc.start_offset()..loc.end_offset()]);
                }
            }
        }
        None
    }
}

struct IdentityComparisonVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl IdentityComparisonVisitor<'_> {
    fn check_call(&mut self, node: &CallNode) {
        let method = node_name!(node);
        if method != "==" && method != "!=" {
            return;
        }

        let args = node.arguments();
        let rhs_opt = args.and_then(|a| {
            let list: Vec<Node> = a.arguments().iter().collect();
            list.into_iter().next()
        });
        let rhs = match rhs_opt {
            Some(n) => n,
            None => return,
        };

        let lhs = match node.receiver() {
            Some(n) => n,
            None => return,
        };

        let lhs_recv = match IdentityComparison::object_id_receiver(&lhs, self.ctx.source) {
            Some(s) => s,
            None => return,
        };
        let rhs_recv = match IdentityComparison::object_id_receiver(&rhs, self.ctx.source) {
            Some(s) => s,
            None => return,
        };

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        let (msg, replacement) = if method == "==" {
            (
                "Use `equal?` instead of `==` when comparing `object_id`.",
                format!("{}.equal?({})", lhs_recv, rhs_recv),
            )
        } else {
            (
                "Use `!equal?` instead of `!=` when comparing `object_id`.",
                format!("!{}.equal?({})", lhs_recv, rhs_recv),
            )
        };

        let correction = Correction::replace(start, end, &replacement);
        let offense = self
            .ctx
            .offense_with_range("Lint/IdentityComparison", msg, Severity::Warning, start, end)
            .with_correction(correction);
        self.offenses.push(offense);
    }
}

impl Visit<'_> for IdentityComparisonVisitor<'_> {
    fn visit_call_node(&mut self, node: &CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for IdentityComparison {
    fn name(&self) -> &'static str {
        "Lint/IdentityComparison"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = IdentityComparisonVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/IdentityComparison", |_cfg| {
    Some(Box::new(IdentityComparison::new()))
});
