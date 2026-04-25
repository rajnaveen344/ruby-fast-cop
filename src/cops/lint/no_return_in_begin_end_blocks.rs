//! Lint/NoReturnInBeginEndBlocks cop
//!
//! Translates RuboCop's NoReturnInBeginEndBlocks. Flags `return` statements
//! that appear inside a `begin..end` block placed on the RHS of an assignment.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct NoReturnInBeginEndBlocks;

impl NoReturnInBeginEndBlocks {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for NoReturnInBeginEndBlocks {
    fn name(&self) -> &'static str {
        "Lint/NoReturnInBeginEndBlocks"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = AssignVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct AssignVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> AssignVisitor<'a> {
    fn check_value(&mut self, value: Node) {
        // Only flag if the RHS is an explicit `begin..end` (BeginNode / kwbegin).
        if let Node::BeginNode { .. } = &value {
            let begin = value.as_begin_node().unwrap();
            let mut finder = ReturnFinder { offenses: Vec::new(), ctx: self.ctx };
            finder.visit_begin_node(&begin);
            self.offenses.extend(finder.offenses);
        }
    }
}

struct ReturnFinder<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for ReturnFinder<'a> {
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        let loc = node.location();
        self.offenses.push(self.ctx.offense_with_range(
            "Lint/NoReturnInBeginEndBlocks",
            "Do not `return` in `begin..end` blocks in assignment contexts.",
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        ));
        ruby_prism::visit_return_node(self, node);
    }
}

impl<'a> Visit<'_> for AssignVisitor<'a> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_instance_variable_write_node(self, node);
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_class_variable_write_node(self, node);
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_global_variable_write_node(self, node);
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_write_node(self, node);
    }
    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_path_write_node(self, node);
    }

    // op-asgn (`+=`, `-=`, …)
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_instance_variable_operator_write_node(&mut self, node: &ruby_prism::InstanceVariableOperatorWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_instance_variable_operator_write_node(self, node);
    }
    fn visit_class_variable_operator_write_node(&mut self, node: &ruby_prism::ClassVariableOperatorWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_class_variable_operator_write_node(self, node);
    }
    fn visit_global_variable_operator_write_node(&mut self, node: &ruby_prism::GlobalVariableOperatorWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_global_variable_operator_write_node(self, node);
    }
    fn visit_constant_operator_write_node(&mut self, node: &ruby_prism::ConstantOperatorWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_operator_write_node(self, node);
    }
    fn visit_constant_path_operator_write_node(&mut self, node: &ruby_prism::ConstantPathOperatorWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_path_operator_write_node(self, node);
    }

    // ||= / &&=
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }
    fn visit_class_variable_or_write_node(&mut self, node: &ruby_prism::ClassVariableOrWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_class_variable_or_write_node(self, node);
    }
    fn visit_global_variable_or_write_node(&mut self, node: &ruby_prism::GlobalVariableOrWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_global_variable_or_write_node(self, node);
    }
    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_or_write_node(self, node);
    }
    fn visit_constant_path_or_write_node(&mut self, node: &ruby_prism::ConstantPathOrWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_path_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_instance_variable_and_write_node(&mut self, node: &ruby_prism::InstanceVariableAndWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_instance_variable_and_write_node(self, node);
    }
    fn visit_class_variable_and_write_node(&mut self, node: &ruby_prism::ClassVariableAndWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_class_variable_and_write_node(self, node);
    }
    fn visit_global_variable_and_write_node(&mut self, node: &ruby_prism::GlobalVariableAndWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_global_variable_and_write_node(self, node);
    }
    fn visit_constant_and_write_node(&mut self, node: &ruby_prism::ConstantAndWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_and_write_node(self, node);
    }
    fn visit_constant_path_and_write_node(&mut self, node: &ruby_prism::ConstantPathAndWriteNode) {
        self.check_value(node.value());
        ruby_prism::visit_constant_path_and_write_node(self, node);
    }
}

crate::register_cop!("Lint/NoReturnInBeginEndBlocks", |_cfg| {
    Some(Box::new(NoReturnInBeginEndBlocks::new()))
});
