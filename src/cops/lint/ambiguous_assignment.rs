//! Lint/AmbiguousAssignment - Detects suspicious `=+`, `=-`, `=*`, `=!` assignments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct AmbiguousAssignment;

impl AmbiguousAssignment {
    pub fn new() -> Self { Self }
}

/// Map from suspicious suffix to the intended operator
fn ambiguous_op(suffix: &str) -> Option<&'static str> {
    match suffix {
        "=-" => Some("-="),
        "=+" => Some("+="),
        "=*" => Some("*="),
        "=!" => Some("!="),
        _ => None,
    }
}

fn check_rhs_start(operator_end: usize, source: &str) -> Option<&'static str> {
    // Extract 2 chars starting at operator position (the `=` is at operator_end - 1)
    // The operator_loc for local_variable_write is the `=` sign
    // The rhs starts after whitespace after `=`
    // We need to check the text `= X` where X is first char of rhs (no space: `=-y` is ambiguous)
    let bytes = source.as_bytes();
    let eq_pos = operator_end.saturating_sub(1);
    if eq_pos + 1 >= bytes.len() {
        return None;
    }
    // Get the 2-char sequence starting at eq_pos: `=X`
    let two = &source[eq_pos..eq_pos + 2.min(source.len() - eq_pos)];
    ambiguous_op(two)
}

impl Cop for AmbiguousAssignment {
    fn name(&self) -> &'static str { "Lint/AmbiguousAssignment" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_operator_loc(&mut self, operator_end: usize) {
        if let Some(intended) = check_rhs_start(operator_end, self.ctx.source) {
            let start = operator_end; // The suspicious char is at operator_end (the `+`/`-`/`*`/`!`)
            let end = operator_end + 1;
            // Actually the offense range is the `=X` part — covers the `=` and the next char
            // RuboCop uses the range from `=` through first char of RHS
            let eq_pos = operator_end.saturating_sub(1);
            let msg = format!("Suspicious assignment detected. Did you mean `{}`?", intended);
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/AmbiguousAssignment",
                &msg,
                Severity::Warning,
                eq_pos,
                eq_pos + 2,
            ));
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let op_end = node.operator_loc().end_offset();
        self.check_operator_loc(op_end);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let op_end = node.operator_loc().end_offset();
        self.check_operator_loc(op_end);
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let op_end = node.operator_loc().end_offset();
        self.check_operator_loc(op_end);
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        let op_end = node.operator_loc().end_offset();
        self.check_operator_loc(op_end);
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let op_end = node.operator_loc().end_offset();
        self.check_operator_loc(op_end);
        ruby_prism::visit_constant_write_node(self, node);
    }
}

crate::register_cop!("Lint/AmbiguousAssignment", |_cfg| Some(Box::new(AmbiguousAssignment::new())));
