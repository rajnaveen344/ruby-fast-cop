//! Lint/SelfAssignment - Checks for self-assignments like `x = x`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::find_comment_start;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Self-assignment detected.";

pub struct LintSelfAssignment {
    allow_rbs_inline_annotation: bool,
}

impl LintSelfAssignment {
    pub fn new(allow_rbs_inline_annotation: bool) -> Self {
        Self { allow_rbs_inline_annotation }
    }
}

impl Default for LintSelfAssignment {
    fn default() -> Self { Self::new(false) }
}

impl Cop for LintSelfAssignment {
    fn name(&self) -> &'static str { "Lint/SelfAssignment" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = SelfAssignmentVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SelfAssignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a LintSelfAssignment,
    offenses: Vec<Offense>,
}

impl<'a> SelfAssignmentVisitor<'a> {
    fn has_rbs_inline_annotation(&self, node_end_offset: usize) -> bool {
        if !self.cop.allow_rbs_inline_annotation {
            return false;
        }
        // Check if there's a #: comment on the same line after the node
        let line_text = self.ctx.line_text(node_end_offset);
        // Find comment on this line
        if let Some(comment_pos) = find_comment_start(line_text) {
            let comment = line_text[comment_pos..].trim();
            return comment.starts_with("#:");
        }
        false
    }

    fn add_offense(&mut self, start: usize, end: usize, rbs_check_offset: usize) {
        if self.has_rbs_inline_annotation(rbs_check_offset) {
            return;
        }
        self.offenses.push(self.ctx.offense_with_range(
            self.cop.name(), MSG, self.cop.severity(), start, end,
        ));
    }

    /// Check simple variable self-assignment: lvar = lvar, ivar = ivar, etc.
    fn check_simple_assignment(&mut self, node_start: usize, node_end: usize, lhs_name: &str, rhs: &Node, expected_rhs_type: &str) {
        let rhs_matches = match (expected_rhs_type, rhs) {
            ("lvar", Node::LocalVariableReadNode { .. }) => {
                let r = rhs.as_local_variable_read_node().unwrap();
                let rhs_name = String::from_utf8_lossy(r.name().as_slice());
                rhs_name.as_ref() == lhs_name
            }
            ("ivar", Node::InstanceVariableReadNode { .. }) => {
                let r = rhs.as_instance_variable_read_node().unwrap();
                let rhs_name = String::from_utf8_lossy(r.name().as_slice());
                rhs_name.as_ref() == lhs_name
            }
            ("cvar", Node::ClassVariableReadNode { .. }) => {
                let r = rhs.as_class_variable_read_node().unwrap();
                let rhs_name = String::from_utf8_lossy(r.name().as_slice());
                rhs_name.as_ref() == lhs_name
            }
            ("gvar", Node::GlobalVariableReadNode { .. }) => {
                let r = rhs.as_global_variable_read_node().unwrap();
                let rhs_name = String::from_utf8_lossy(r.name().as_slice());
                rhs_name.as_ref() == lhs_name
            }
            _ => false,
        };

        if rhs_matches {
            self.add_offense(node_start, node_end, rhs.location().end_offset());
        }
    }
}

impl Visit<'_> for SelfAssignmentVisitor<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let lhs_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let rhs = node.value();
        self.check_simple_assignment(
            node.location().start_offset(),
            node.location().end_offset(),
            &lhs_name, &rhs, "lvar",
        );
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let lhs_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let rhs = node.value();
        self.check_simple_assignment(
            node.location().start_offset(),
            node.location().end_offset(),
            &lhs_name, &rhs, "ivar",
        );
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let lhs_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let rhs = node.value();
        self.check_simple_assignment(
            node.location().start_offset(),
            node.location().end_offset(),
            &lhs_name, &rhs, "cvar",
        );
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        let lhs_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let rhs = node.value();
        self.check_simple_assignment(
            node.location().start_offset(),
            node.location().end_offset(),
            &lhs_name, &rhs, "gvar",
        );
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        // Foo = Foo (same name, same namespace)
        let rhs = node.value();
        if let Some(const_read) = rhs.as_constant_read_node() {
            let lhs_name = String::from_utf8_lossy(node.name().as_slice());
            let rhs_name = String::from_utf8_lossy(const_read.name().as_slice());
            if lhs_name == rhs_name {
                // Both are bare constants (no namespace)
                self.add_offense(
                    node.location().start_offset(),
                    node.location().end_offset(),
                    rhs.location().end_offset(),
                );
            }
        }
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        // foo, bar = foo, bar  or  foo, bar = [foo, bar]
        let lefts: Vec<_> = node.lefts().iter().collect();
        let rhs = node.value();

        // rhs should be an ArrayNode
        let rhs_elements: Vec<Node> = if let Some(arr) = rhs.as_array_node() {
            arr.elements().iter().collect()
        } else {
            // Not an array or an implicit array on RHS - skip
            ruby_prism::visit_multi_write_node(self, node);
            return;
        };

        if lefts.len() != rhs_elements.len() {
            ruby_prism::visit_multi_write_node(self, node);
            return;
        }

        let all_match = lefts.iter().zip(rhs_elements.iter()).all(|(lhs, rhs_elem)| {
            rhs_matches_lhs(rhs_elem, lhs)
        });

        if all_match {
            // For RBS check, use the first LHS element
            let rbs_offset = if !lefts.is_empty() {
                lefts[0].location().end_offset()
            } else {
                node.location().end_offset()
            };
            self.add_offense(
                node.location().start_offset(),
                node.location().end_offset(),
                rbs_offset,
            );
        }

        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        // foo ||= foo
        let lhs_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let rhs = node.value();
        if let Some(read) = rhs.as_local_variable_read_node() {
            let rhs_name = String::from_utf8_lossy(read.name().as_slice());
            if lhs_name == rhs_name.as_ref() {
                self.add_offense(
                    node.location().start_offset(),
                    node.location().end_offset(),
                    rhs.location().end_offset(),
                );
            }
        }
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        // foo &&= foo
        let lhs_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let rhs = node.value();
        if let Some(read) = rhs.as_local_variable_read_node() {
            let rhs_name = String::from_utf8_lossy(read.name().as_slice());
            if lhs_name == rhs_name.as_ref() {
                self.add_offense(
                    node.location().start_offset(),
                    node.location().end_offset(),
                    rhs.location().end_offset(),
                );
            }
        }
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();

        if method == "[]=" {
            self.handle_key_assignment(node);
        } else if method.ends_with('=') && method != "==" && method != "!=" && method != "<=" && method != ">=" && method != "===" {
            self.handle_attribute_assignment(node);
        }

        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> SelfAssignmentVisitor<'a> {
    fn handle_key_assignment(&mut self, node: &ruby_prism::CallNode) {
        // foo["bar"] = foo["bar"] or foo[1] = foo[1]
        let args: Vec<_> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => return,
        };
        if args.is_empty() {
            return;
        }

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        // Last arg is the value
        let value_node = &args[args.len() - 1];
        let key_args = &args[..args.len() - 1];

        // The value must be a call to [] on the same receiver with same key args
        let value_call = match value_node.as_call_node() {
            Some(c) => c,
            None => return,
        };

        let value_method = String::from_utf8_lossy(value_call.name().as_slice());
        if value_method.as_ref() != "[]" {
            return;
        }

        let value_receiver = match value_call.receiver() {
            Some(r) => r,
            None => return,
        };

        // Compare receivers by source
        let recv_src = source_of(&receiver, self.ctx);
        let value_recv_src = source_of(&value_receiver, self.ctx);
        if recv_src != value_recv_src {
            return;
        }

        // Compare key arguments - none should be a method call (non-literal)
        let value_args: Vec<_> = match value_call.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => Vec::new(),
        };

        if key_args.len() != value_args.len() {
            return;
        }

        // Check that no key arg is a method call (could return different results)
        for key_arg in key_args {
            if is_method_call(key_arg) {
                return;
            }
        }

        // Compare key args by source
        for (k, v) in key_args.iter().zip(value_args.iter()) {
            let k_src = source_of(k, self.ctx);
            let v_src = source_of(v, self.ctx);
            if k_src != v_src {
                return;
            }
        }

        self.add_offense(
            node.location().start_offset(),
            node.location().end_offset(),
            node.location().end_offset(),
        );
    }

    fn handle_attribute_assignment(&mut self, node: &ruby_prism::CallNode) {
        // foo.bar = foo.bar
        let args: Vec<_> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => return,
        };
        if args.len() != 1 {
            return;
        }

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        let first_arg = &args[0];
        let arg_call = match first_arg.as_call_node() {
            Some(c) => c,
            None => return,
        };

        // The argument must have no arguments itself (a simple reader)
        if let Some(arg_args) = arg_call.arguments() {
            if arg_args.arguments().iter().count() > 0 {
                return;
            }
        }

        // Check it has a block -- if so, skip
        if arg_call.block().is_some() {
            return;
        }

        let arg_receiver = match arg_call.receiver() {
            Some(r) => r,
            None => return,
        };

        // Compare receivers by source
        let recv_src = source_of(&receiver, self.ctx);
        let arg_recv_src = source_of(&arg_receiver, self.ctx);
        if recv_src != arg_recv_src {
            return;
        }

        // Compare method names: lhs is "bar=" and rhs should be "bar"
        let lhs_method = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let rhs_method = String::from_utf8_lossy(arg_call.name().as_slice()).to_string();

        // Strip trailing = from lhs
        let lhs_base = lhs_method.strip_suffix('=').unwrap_or(&lhs_method);
        if lhs_base != rhs_method {
            return;
        }

        self.add_offense(
            node.location().start_offset(),
            node.location().end_offset(),
            node.location().end_offset(),
        );
    }
}

fn source_of<'a>(node: &Node, ctx: &'a CheckContext) -> &'a str {
    let loc = node.location();
    &ctx.source[loc.start_offset()..loc.end_offset()]
}

fn is_method_call(node: &Node) -> bool {
    // A "method call" in RuboCop's context means a non-literal call
    // Local variable reads, constants, literals are not method calls
    matches!(node, Node::CallNode { .. })
}

/// Check if rhs matches lhs for multi-assignment self-assignment detection
fn rhs_matches_lhs(rhs: &Node, lhs: &Node) -> bool {
    match lhs {
        Node::LocalVariableTargetNode { .. } => {
            let lhs_node = lhs.as_local_variable_target_node().unwrap();
            let lhs_name = String::from_utf8_lossy(lhs_node.name().as_slice());
            if let Some(read) = rhs.as_local_variable_read_node() {
                let rhs_name = String::from_utf8_lossy(read.name().as_slice());
                return lhs_name == rhs_name;
            }
            false
        }
        Node::InstanceVariableTargetNode { .. } => {
            let lhs_node = lhs.as_instance_variable_target_node().unwrap();
            let lhs_name = String::from_utf8_lossy(lhs_node.name().as_slice());
            if let Some(read) = rhs.as_instance_variable_read_node() {
                let rhs_name = String::from_utf8_lossy(read.name().as_slice());
                return lhs_name == rhs_name;
            }
            false
        }
        Node::ClassVariableTargetNode { .. } => {
            let lhs_node = lhs.as_class_variable_target_node().unwrap();
            let lhs_name = String::from_utf8_lossy(lhs_node.name().as_slice());
            if let Some(read) = rhs.as_class_variable_read_node() {
                let rhs_name = String::from_utf8_lossy(read.name().as_slice());
                return lhs_name == rhs_name;
            }
            false
        }
        Node::GlobalVariableTargetNode { .. } => {
            let lhs_node = lhs.as_global_variable_target_node().unwrap();
            let lhs_name = String::from_utf8_lossy(lhs_node.name().as_slice());
            if let Some(read) = rhs.as_global_variable_read_node() {
                let rhs_name = String::from_utf8_lossy(read.name().as_slice());
                return lhs_name == rhs_name;
            }
            false
        }
        _ => false,
    }
}
