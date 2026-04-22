//! Lint/RescueException - Avoid rescuing the Exception class.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/rescue_exception.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct RescueException;

impl RescueException {
    pub fn new() -> Self {
        Self
    }

    /// Returns true if node is `Exception` or `::Exception` (top-level only).
    fn targets_exception(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let n = node.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(n.name().as_slice());
                name == "Exception"
            }
            Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                // ::Exception — parent must be None (root)
                if cp.parent().is_some() {
                    return false;
                }
                let const_id = match cp.name() {
                    Some(id) => id,
                    None => return false,
                };
                let name = String::from_utf8_lossy(const_id.as_slice());
                name == "Exception"
            }
            _ => false,
        }
    }
}

struct RescueExceptionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for RescueExceptionVisitor<'_> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        let exceptions: Vec<Node> = node.exceptions().iter().collect();

        if !exceptions.is_empty() && exceptions.iter().any(RescueException::targets_exception) {
            // Range: "rescue" keyword start to end of last exception (+ reference if present)
            let rescue_start = node.location().start_offset();

            let end_offset = if let Some(ref_node) = node.reference() {
                ref_node.location().end_offset()
            } else {
                exceptions.last().unwrap().location().end_offset()
            };

            let offense = self.ctx.offense_with_range(
                "Lint/RescueException",
                "Avoid rescuing the `Exception` class. Perhaps you meant to rescue `StandardError`?",
                Severity::Warning,
                rescue_start,
                end_offset,
            );
            self.offenses.push(offense);
        }

        ruby_prism::visit_rescue_node(self, node);
    }
}

impl Cop for RescueException {
    fn name(&self) -> &'static str {
        "Lint/RescueException"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RescueExceptionVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/RescueException", |_cfg| {
    Some(Box::new(RescueException::new()))
});
