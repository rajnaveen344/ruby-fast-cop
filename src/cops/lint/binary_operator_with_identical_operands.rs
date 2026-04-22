//! Lint/BinaryOperatorWithIdenticalOperands cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct BinaryOperatorWithIdenticalOperands;

impl Default for BinaryOperatorWithIdenticalOperands {
    fn default() -> Self {
        Self
    }
}

impl BinaryOperatorWithIdenticalOperands {
    pub fn new() -> Self {
        Self
    }
}

/// The flagged binary operator method names (sent via CallNode).
const FLAGGED_OPS: &[&str] = &[
    "==", "!=", "===", "<=>", "=~", ">", ">=", "<", "<=", "|", "^",
];

/// Check if two Prism nodes have identical source text (simple structural equality via source).
fn nodes_identical(source: &str, a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    let a = source.get(a_start..a_end).unwrap_or("");
    let b = source.get(b_start..b_end).unwrap_or("");
    !a.is_empty() && a == b
}

impl Cop for BinaryOperatorWithIdenticalOperands {
    fn name(&self) -> &'static str {
        "Lint/BinaryOperatorWithIdenticalOperands"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = BinaryOpVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct BinaryOpVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for BinaryOpVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);

        if FLAGGED_OPS.contains(&method.as_ref()) {
            // binary_operation? in RuboCop: receiver + exactly 1 argument
            if let Some(receiver) = node.receiver() {
                if let Some(args) = node.arguments() {
                    let args_list: Vec<_> = args.arguments().iter().collect();
                    if args_list.len() == 1 {
                        let recv_start = receiver.location().start_offset();
                        let recv_end = receiver.location().end_offset();
                        let arg = &args_list[0];
                        let arg_start = arg.location().start_offset();
                        let arg_end = arg.location().end_offset();

                        if nodes_identical(self.ctx.source, recv_start, recv_end, arg_start, arg_end) {
                            let start = node.location().start_offset();
                            let end = node.location().end_offset();
                            let msg = format!("Binary operator `{}` has identical operands.", method);
                            self.offenses.push(self.ctx.offense_with_range(
                                "Lint/BinaryOperatorWithIdenticalOperands",
                                &msg,
                                Severity::Warning,
                                start,
                                end,
                            ));
                        }
                    }
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let l_start = node.left().location().start_offset();
        let l_end = node.left().location().end_offset();
        let r_start = node.right().location().start_offset();
        let r_end = node.right().location().end_offset();

        if nodes_identical(self.ctx.source, l_start, l_end, r_start, r_end) {
            // Find the operator "&&"
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/BinaryOperatorWithIdenticalOperands",
                "Binary operator `&&` has identical operands.",
                Severity::Warning,
                start,
                end,
            ));
        }

        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let l_start = node.left().location().start_offset();
        let l_end = node.left().location().end_offset();
        let r_start = node.right().location().start_offset();
        let r_end = node.right().location().end_offset();

        if nodes_identical(self.ctx.source, l_start, l_end, r_start, r_end) {
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/BinaryOperatorWithIdenticalOperands",
                "Binary operator `||` has identical operands.",
                Severity::Warning,
                start,
                end,
            ));
        }

        ruby_prism::visit_or_node(self, node);
    }
}

crate::register_cop!("Lint/BinaryOperatorWithIdenticalOperands", |_cfg| {
    Some(Box::new(BinaryOperatorWithIdenticalOperands::new()))
});
