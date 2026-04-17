//! Style/SelfAssignment - Enforces the use of shorthand for self-assignment.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/self_assignment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const OPS: &[&str] = &["+", "-", "*", "**", "/", "%", "^", "<<", ">>", "|", "&"];

#[derive(Default)]
pub struct SelfAssignment;

impl SelfAssignment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for SelfAssignment {
    fn name(&self) -> &'static str {
        "Style/SelfAssignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = SelfAssignmentVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct SelfAssignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// Variable kind (local, instance, class) and its name
enum VarInfo {
    Local(String),
    Instance(String),
    Class(String),
}

impl VarInfo {
    /// Check if a node is a read of the same variable
    fn matches_read(&self, node: &Node) -> bool {
        match self {
            VarInfo::Local(name) => {
                if let Node::LocalVariableReadNode { .. } = node {
                    let read = node.as_local_variable_read_node().unwrap();
                    let read_name = node_name!(read);
                    return read_name == name.as_str();
                }
                false
            }
            VarInfo::Instance(name) => {
                if let Node::InstanceVariableReadNode { .. } = node {
                    let read = node.as_instance_variable_read_node().unwrap();
                    let read_name = node_name!(read);
                    return read_name == name.as_str();
                }
                false
            }
            VarInfo::Class(name) => {
                if let Node::ClassVariableReadNode { .. } = node {
                    let read = node.as_class_variable_read_node().unwrap();
                    let read_name = node_name!(read);
                    return read_name == name.as_str();
                }
                false
            }
        }
    }
}

impl<'a> SelfAssignmentVisitor<'a> {
    fn check_assignment(
        &mut self,
        var_info: &VarInfo,
        value: &Node,
        assign_start: usize,
        assign_end: usize,
        operator_loc_start: usize,
    ) {
        // Check for arithmetic/bitwise: x = x + y  or  x = x.+(y)
        if let Node::CallNode { .. } = value {
            let call = value.as_call_node().unwrap();
            let method = node_name!(call);

            if !OPS.contains(&method.as_ref()) {
                return;
            }

            // Must have exactly one argument
            if let Some(args) = call.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if arg_list.len() != 1 {
                    return;
                }

                // Receiver must be the same variable
                if let Some(receiver) = call.receiver() {
                    if var_info.matches_read(&receiver) {
                        let msg = format!("Use self-assignment shorthand `{}=`.", method);
                        let offense = self.ctx.offense_with_range(
                            "Style/SelfAssignment",
                            &msg,
                            Severity::Convention,
                            assign_start,
                            assign_end,
                        );

                        // Correction: insert operator before `=`, replace RHS with just the argument
                        let arg_source = self.ctx.src(
                            arg_list[0].location().start_offset(),
                            arg_list[0].location().end_offset(),
                        );
                        let correction = Correction { edits: vec![
                            crate::offense::Edit {
                                start_offset: operator_loc_start,
                                end_offset: operator_loc_start,
                                replacement: method.to_string(),
                            },
                            crate::offense::Edit {
                                start_offset: value.location().start_offset(),
                                end_offset: value.location().end_offset(),
                                replacement: arg_source.to_string(),
                            },
                        ]};

                        self.offenses.push(offense.with_correction(correction));
                    }
                }
            }
            return;
        }

        // Check for boolean: x = x || y  or  x = x && y
        let (lhs_node, rhs_node, operator_str) = match value {
            Node::OrNode { .. } => {
                let or = value.as_or_node().unwrap();
                (or.left(), or.right(), "||")
            }
            Node::AndNode { .. } => {
                let and = value.as_and_node().unwrap();
                (and.left(), and.right(), "&&")
            }
            _ => return,
        };

        if var_info.matches_read(&lhs_node) {
            let msg = format!("Use self-assignment shorthand `{}=`.", operator_str);
            let offense = self.ctx.offense_with_range(
                "Style/SelfAssignment",
                &msg,
                Severity::Convention,
                assign_start,
                assign_end,
            );

            // Correction: insert operator before `=`, replace RHS with just the right side
            let rhs_source = self.ctx.src(
                rhs_node.location().start_offset(),
                rhs_node.location().end_offset(),
            );
            let correction = Correction { edits: vec![
                crate::offense::Edit {
                    start_offset: operator_loc_start,
                    end_offset: operator_loc_start,
                    replacement: operator_str.to_string(),
                },
                crate::offense::Edit {
                    start_offset: value.location().start_offset(),
                    end_offset: value.location().end_offset(),
                    replacement: rhs_source.to_string(),
                },
            ]};

            self.offenses.push(offense.with_correction(correction));
        }
    }
}

impl Visit<'_> for SelfAssignmentVisitor<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = node_name!(node).to_string();
        let var_info = VarInfo::Local(name);
        let value = node.value();
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let op_start = node.operator_loc().start_offset();
        self.check_assignment(&var_info, &value, start, end, op_start);

        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableWriteNode,
    ) {
        let name = node_name!(node).to_string();
        let var_info = VarInfo::Instance(name);
        let value = node.value();
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let op_start = node.operator_loc().start_offset();
        self.check_assignment(&var_info, &value, start, end, op_start);

        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let name = node_name!(node).to_string();
        let var_info = VarInfo::Class(name);
        let value = node.value();
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let op_start = node.operator_loc().start_offset();
        self.check_assignment(&var_info, &value, start, end, op_start);

        ruby_prism::visit_class_variable_write_node(self, node);
    }
}

crate::register_cop!("Style/SelfAssignment", |_cfg| {
    Some(Box::new(SelfAssignment::new()))
});
