//! Style/OrAssignment — Prefer `||=` over conditional assignment patterns.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/or_assignment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Use the double pipe equals operator `||=` instead.";

fn is_var_write(node: &Node) -> bool {
    matches!(
        node,
        Node::LocalVariableWriteNode { .. }
            | Node::InstanceVariableWriteNode { .. }
            | Node::ClassVariableWriteNode { .. }
            | Node::GlobalVariableWriteNode { .. }
    )
}

fn is_var_read(node: &Node) -> bool {
    matches!(
        node,
        Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
    )
}

fn node_src<'a>(node: &Node, source: &'a str) -> &'a str {
    let loc = node.location();
    &source[loc.start_offset()..loc.end_offset()]
}

/// Get LHS name from write node (text before `=`).
fn write_lhs<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    if !is_var_write(node) { return None; }
    let full = node_src(node, source);
    // Find ` =` (space before equals, not `==`)
    let pos = full.find(" =")?;
    // Make sure it's not `==`
    let after = &full[pos + 2..];
    if after.starts_with('=') { return None; }
    Some(full[..pos].trim())
}

/// Get value source text from write node.
fn write_val_src<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    if !is_var_write(node) { return None; }
    let full = node_src(node, source);
    let pos = full.find(" =")?;
    let after = &full[pos + 2..];
    if after.starts_with('=') { return None; }
    Some(after.trim())
}

#[derive(Default)]
pub struct OrAssignment;

impl OrAssignment {
    pub fn new() -> Self { Self }
}

impl Cop for OrAssignment {
    fn name(&self) -> &'static str { "Style/OrAssignment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = OrAssignmentVisitor { ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct OrAssignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> OrAssignmentVisitor<'a> {
    /// `var = var ? var : default`
    fn check_ternary_write(&mut self, write_node: &Node) {
        if !is_var_write(write_node) { return; }
        let var_name = match write_lhs(write_node, self.ctx.source) {
            Some(v) => v,
            None => return,
        };

        // Get value: must be an IfNode
        let val_src = match write_val_src(write_node, self.ctx.source) {
            Some(v) => v,
            None => return,
        };

        // We need the actual value node for structural inspection.
        // The IfNode is embedded in the write node value.
        // Use the write node's own AST access.
        let maybe_if = self.get_write_value_if(write_node);
        let if_node = match maybe_if {
            Some(n) => n,
            None => return,
        };

        let start = write_node.location().start_offset();
        let end = write_node.location().end_offset();

        self.check_if_node_for_or_assign(if_node, var_name, start, end);
    }

    fn get_write_value_if<'b>(&self, write_node: &'b Node) -> Option<ruby_prism::IfNode<'b>> {
        match write_node {
            Node::LocalVariableWriteNode { .. } => {
                write_node.as_local_variable_write_node()?.value().as_if_node()
            }
            Node::InstanceVariableWriteNode { .. } => {
                write_node.as_instance_variable_write_node()?.value().as_if_node()
            }
            Node::ClassVariableWriteNode { .. } => {
                write_node.as_class_variable_write_node()?.value().as_if_node()
            }
            Node::GlobalVariableWriteNode { .. } => {
                write_node.as_global_variable_write_node()?.value().as_if_node()
            }
            _ => None,
        }
    }

    fn check_if_node_for_or_assign(
        &mut self,
        if_node: ruby_prism::IfNode,
        var_name: &str,
        write_start: usize,
        write_end: usize,
    ) {
        let source = self.ctx.source;
        // condition must be same var
        let cond = if_node.predicate();
        let cond_src = node_src(&cond, source);
        if cond_src != var_name { return; }

        // No elsif
        if let Some(sub) = if_node.subsequent() {
            if sub.as_if_node().is_some() { return; }
        }

        // then branch = same var
        let then_stmts = match if_node.statements() {
            Some(s) => s,
            None => return,
        };
        let then_items: Vec<_> = then_stmts.body().iter().collect();
        if then_items.len() != 1 { return; }
        let then_src = node_src(&then_items[0], source);
        if then_src != var_name { return; }

        // else branch
        let else_n = match if_node.subsequent() {
            Some(e) => e,
            None => return,
        };
        let else_n = match else_n.as_else_node() {
            Some(n) => n,
            None => return,
        };
        let else_stmts = match else_n.statements() {
            Some(s) => s,
            None => return,
        };
        let else_items: Vec<_> = else_stmts.body().iter().collect();
        if else_items.len() != 1 { return; }
        let default_src = node_src(&else_items[0], source);

        let correction = format!("{} ||= {}", var_name, default_src);
        let offense = self.ctx.offense_with_range(
            "Style/OrAssignment", MSG, Severity::Convention, write_start, write_end,
        ).with_correction(Correction::replace(write_start, write_end, correction));
        self.offenses.push(offense);
    }

    /// `unless var; var = default; end`
    fn check_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        // No else branch
        if node.else_clause().is_some() { return; }

        let cond = node.predicate();
        if !is_var_read(&cond) { return; }
        let var_name = node_src(&cond, self.ctx.source);

        let then_stmts = match node.statements() {
            Some(s) => s,
            None => return,
        };
        let items: Vec<_> = then_stmts.body().iter().collect();
        if items.len() != 1 { return; }

        let write_node = &items[0];
        if !is_var_write(write_node) { return; }

        let write_var = match write_lhs(write_node, self.ctx.source) {
            Some(v) => v,
            None => return,
        };
        if write_var != var_name { return; }

        let default_src = match write_val_src(write_node, self.ctx.source) {
            Some(v) => v,
            None => return,
        };

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let correction = format!("{} ||= {}", var_name, default_src);
        let offense = self.ctx.offense_with_range(
            "Style/OrAssignment", MSG, Severity::Convention, start, end,
        ).with_correction(Correction::replace(start, end, correction));
        self.offenses.push(offense);
    }

    /// `if var; (empty); else; var = default; end`
    fn check_if_empty_then(&mut self, node: &ruby_prism::IfNode) {
        let cond = node.predicate();
        if !is_var_read(&cond) { return; }
        let var_name = node_src(&cond, self.ctx.source);

        // No elsif
        if let Some(sub) = node.subsequent() {
            if sub.as_if_node().is_some() { return; }
        }

        // Then branch empty
        let then_empty = node.statements()
            .map_or(true, |s| s.body().iter().count() == 0);
        if !then_empty { return; }

        let else_n = match node.subsequent() {
            Some(e) => e,
            None => return,
        };
        let else_n = match else_n.as_else_node() {
            Some(n) => n,
            None => return,
        };
        let else_stmts = match else_n.statements() {
            Some(s) => s,
            None => return,
        };
        let else_items: Vec<_> = else_stmts.body().iter().collect();
        if else_items.len() != 1 { return; }

        let write_node = &else_items[0];
        if !is_var_write(write_node) { return; }

        let write_var = match write_lhs(write_node, self.ctx.source) {
            Some(v) => v,
            None => return,
        };
        if write_var != var_name { return; }

        let default_src = match write_val_src(write_node, self.ctx.source) {
            Some(v) => v,
            None => return,
        };

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let correction = format!("{} ||= {}", var_name, default_src);
        let offense = self.ctx.offense_with_range(
            "Style/OrAssignment", MSG, Severity::Convention, start, end,
        ).with_correction(Correction::replace(start, end, correction));
        self.offenses.push(offense);
    }
}

impl<'a> Visit<'_> for OrAssignmentVisitor<'a> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        self.check_ternary_write(&node.as_node());
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        self.check_ternary_write(&node.as_node());
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        self.check_ternary_write(&node.as_node());
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.check_ternary_write(&node.as_node());
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless_node(node);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if_empty_then(node);
        ruby_prism::visit_if_node(self, node);
    }
}

crate::register_cop!("Style/OrAssignment", |_cfg| {
    Some(Box::new(OrAssignment::new()))
});
