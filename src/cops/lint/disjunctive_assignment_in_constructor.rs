//! Lint/DisjunctiveAssignmentInConstructor cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/disjunctive_assignment_in_constructor.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;
use ruby_prism::Node;

#[derive(Default)]
pub struct DisjunctiveAssignmentInConstructor;

impl DisjunctiveAssignmentInConstructor {
    pub fn new() -> Self { Self }
}

impl Cop for DisjunctiveAssignmentInConstructor {
    fn name(&self) -> &'static str { "Lint/DisjunctiveAssignmentInConstructor" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let name = node_name!(node);
        if name != "initialize" {
            return vec![];
        }
        let body = match node.body() {
            Some(b) => b,
            None => return vec![],
        };
        check_body(body, ctx)
    }
}

fn check_body(body: Node, ctx: &CheckContext) -> Vec<Offense> {
    let stmts: Vec<Node> = match &body {
        Node::StatementsNode { .. } => {
            body.as_statements_node().unwrap().body().iter().collect()
        }
        _ => vec![body],
    };
    check_lines(stmts, ctx)
}

fn check_lines(lines: Vec<Node>, ctx: &CheckContext) -> Vec<Offense> {
    let mut offenses = Vec::new();
    for line in lines {
        match &line {
            Node::InstanceVariableOrWriteNode { .. } => {
                // @x ||= y — flag it
                let node = line.as_instance_variable_or_write_node().unwrap();
                let op_loc = node.operator_loc();
                let correction = Correction::replace(
                    op_loc.start_offset(),
                    op_loc.end_offset(),
                    "=".to_string(),
                );
                offenses.push(ctx.offense_with_range(
                    "Lint/DisjunctiveAssignmentInConstructor",
                    "Unnecessary disjunctive assignment. Use plain assignment.",
                    Severity::Warning,
                    op_loc.start_offset(),
                    op_loc.end_offset(),
                ).with_correction(correction));
            }
            _ => {
                // Any non-disjunctive-ivar statement — stop (RuboCop breaks here)
                break;
            }
        }
    }
    offenses
}

crate::register_cop!("Lint/DisjunctiveAssignmentInConstructor", |_cfg| {
    Some(Box::new(DisjunctiveAssignmentInConstructor::new()))
});
