//! Style/RedundantSelfAssignment cop
//!
//! Detects `foo = foo.mutating_method(...)` where assignment is redundant.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

/// Methods that mutate receiver in place.
const MUTATION_METHODS: &[&str] = &[
    "append", "clear", "collect!", "concat", "delete", "delete_if", "fill",
    "filter!", "flatten!", "gsub!", "keep_if", "map!", "merge!", "pop", "push",
    "prepend", "reject!", "replace", "reverse!", "rotate!", "select!", "shift",
    "shuffle!", "slice!", "sort!", "sort_by!", "squeeze!", "strip!", "sub!",
    "transform_keys!", "transform_values!", "uniq!", "unshift", "update",
];

const MSG: &str = "Redundant self assignment detected. Method `%s` modifies its receiver in place.";

#[derive(Default)]
pub struct RedundantSelfAssignment;

impl RedundantSelfAssignment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantSelfAssignment {
    fn name(&self) -> &'static str {
        "Style/RedundantSelfAssignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = SelfAssignVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        let root = result.node();
        let prog = root.as_program_node().unwrap();
        ruby_prism::visit_program_node(&mut visitor, &prog);
        visitor.offenses
    }
}

struct SelfAssignVisitor<'a> {
    cop: &'a RedundantSelfAssignment,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl SelfAssignVisitor<'_> {
    fn check_rhs_mutation(&mut self, eq_start: usize, eq_end: usize, assign_start: usize, assign_end: usize, lhs_name_bytes: &[u8], rhs_node: ruby_prism::Node) {
        let rhs_call = match rhs_node.as_call_node() {
            Some(c) => c,
            None => return,
        };
        let method = node_name!(rhs_call);
        if !MUTATION_METHODS.contains(&method.as_ref()) {
            return;
        }
        let recv = match rhs_call.receiver() {
            Some(r) => r,
            None => return,
        };
        // Receiver name must match lhs
        let recv_name: Vec<u8> = match recv {
            ruby_prism::Node::LocalVariableReadNode { .. } => {
                recv.as_local_variable_read_node().unwrap().name().as_slice().to_vec()
            }
            ruby_prism::Node::InstanceVariableReadNode { .. } => {
                recv.as_instance_variable_read_node().unwrap().name().as_slice().to_vec()
            }
            ruby_prism::Node::ClassVariableReadNode { .. } => {
                recv.as_class_variable_read_node().unwrap().name().as_slice().to_vec()
            }
            ruby_prism::Node::GlobalVariableReadNode { .. } => {
                recv.as_global_variable_read_node().unwrap().name().as_slice().to_vec()
            }
            _ => return,
        };
        if recv_name != lhs_name_bytes {
            return;
        }
        let msg = MSG.replacen("%s", method.as_ref(), 1);
        let rhs_src = &self.ctx.source[rhs_node.location().start_offset()..rhs_node.location().end_offset()];
        let correction = Correction::replace(assign_start, assign_end, rhs_src.to_string());
        self.offenses.push(
            self.ctx.offense_with_range(self.cop.name(), &msg, self.cop.severity(), eq_start, eq_end)
                .with_correction(correction)
        );
    }

    fn check_call_setter(&mut self, node: &ruby_prism::CallNode) {
        // `other.foo = other.foo.method(...)` → CallNode{message: "foo=", receiver: other, args: [rhs]}
        let method = node_name!(node);
        if !method.ends_with('=') {
            return;
        }
        let attr_name = &method[..method.len() - 1];

        let lhs_recv = match node.receiver() {
            Some(r) => r,
            None => return,
        };
        // Skip `self.foo = ...`
        if lhs_recv.as_self_node().is_some() {
            return;
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return;
        }
        let rhs = &arg_list[0];
        let rhs_call = match rhs.as_call_node() {
            Some(c) => c,
            None => return,
        };
        let mut_method = node_name!(rhs_call);
        if !MUTATION_METHODS.contains(&mut_method.as_ref()) {
            return;
        }
        // rhs receiver must be `lhs_recv.attr_name`
        let rhs_recv = match rhs_call.receiver() {
            Some(r) => r,
            None => return,
        };
        let rhs_recv_call = match rhs_recv.as_call_node() {
            Some(c) => c,
            None => return,
        };
        let rhs_recv_method = node_name!(rhs_recv_call);
        if rhs_recv_method.as_ref() != attr_name {
            return;
        }
        let rhs_recv_recv = match rhs_recv_call.receiver() {
            Some(r) => r,
            None => return,
        };
        // lhs_recv must match rhs_recv_recv (ignoring &.)
        let lhs_src = &self.ctx.source[lhs_recv.location().start_offset()..lhs_recv.location().end_offset()];
        let rhs_recv_src = &self.ctx.source[rhs_recv_recv.location().start_offset()..rhs_recv_recv.location().end_offset()];
        if lhs_src.replace("&.", ".") != rhs_recv_src.replace("&.", ".") {
            return;
        }

        // Offense position: find `=` sign
        // In `other.foo = other.foo.method(...)`, we need the `=` column.
        // The fixture shows column for `=` (0-indexed).
        // node.location() covers `other.foo = other.foo.method(...)`
        // Find `=` after lhs_recv end + attr portion
        let lhs_end = lhs_recv.location().end_offset();
        // After lhs_recv, there's `.foo =` or `&.foo =`
        let rest = &self.ctx.source[lhs_end..];
        // Find the space before `=` (pattern: `.attr_name =`)
        let eq_pos = rest.find('=').unwrap_or(0);
        let eq_start = lhs_end + eq_pos;
        let eq_end = eq_start + 1;

        let msg = MSG.replacen("%s", mut_method.as_ref(), 1);
        let rhs_src = &self.ctx.source[rhs.location().start_offset()..rhs.location().end_offset()];
        let correction = Correction::replace(
            node.location().start_offset(),
            node.location().end_offset(),
            rhs_src.to_string(),
        );
        self.offenses.push(
            self.ctx.offense_with_range(self.cop.name(), &msg, self.cop.severity(), eq_start, eq_end)
                .with_correction(correction)
        );
    }
}

impl Visit<'_> for SelfAssignVisitor<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = node.name().as_slice().to_vec();
        let eq_loc = node.operator_loc();
        self.check_rhs_mutation(
            eq_loc.start_offset(), eq_loc.end_offset(),
            node.location().start_offset(), node.location().end_offset(),
            &name, node.value(),
        );
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let name = node.name().as_slice().to_vec();
        let eq_loc = node.operator_loc();
        self.check_rhs_mutation(
            eq_loc.start_offset(), eq_loc.end_offset(),
            node.location().start_offset(), node.location().end_offset(),
            &name, node.value(),
        );
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let name = node.name().as_slice().to_vec();
        let eq_loc = node.operator_loc();
        self.check_rhs_mutation(
            eq_loc.start_offset(), eq_loc.end_offset(),
            node.location().start_offset(), node.location().end_offset(),
            &name, node.value(),
        );
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        let name = node.name().as_slice().to_vec();
        let eq_loc = node.operator_loc();
        self.check_rhs_mutation(
            eq_loc.start_offset(), eq_loc.end_offset(),
            node.location().start_offset(), node.location().end_offset(),
            &name, node.value(),
        );
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call_setter(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/RedundantSelfAssignment", |_cfg| {
    Some(Box::new(RedundantSelfAssignment::new()))
});
