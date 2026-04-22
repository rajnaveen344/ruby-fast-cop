//! Style/EachWithObject cop
//!
//! Looks for inject/reduce calls where the accumulator is returned at end.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct EachWithObject;

impl EachWithObject {
    pub fn new() -> Self {
        Self
    }
}

/// Visitor to detect if a variable name is assigned anywhere in a subtree
struct AssignmentChecker {
    name: String,
    found: bool,
}

impl<'a> Visit<'_> for AssignmentChecker {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n.as_ref() == self.name {
            self.found = true;
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n.as_ref() == self.name {
            self.found = true;
        }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n.as_ref() == self.name {
            self.found = true;
        }
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let n = String::from_utf8_lossy(node.name().as_slice());
        if n.as_ref() == self.name {
            self.found = true;
        }
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
}

fn accumulator_assigned_in(body: &Node, acc_name: &str) -> bool {
    let mut checker = AssignmentChecker {
        name: acc_name.to_string(),
        found: false,
    };
    // We need to visit the body as a program fragment — use check via specific node types
    match body {
        Node::StatementsNode { .. } => {
            checker.visit_statements_node(&body.as_statements_node().unwrap());
        }
        _ => {
            // Visit whatever it is
            visit_node_generic(&mut checker, body);
        }
    }
    checker.found
}

fn visit_node_generic(checker: &mut AssignmentChecker, node: &Node) {
    match node {
        Node::StatementsNode { .. } => {
            checker.visit_statements_node(&node.as_statements_node().unwrap());
        }
        Node::BeginNode { .. } => {
            checker.visit_begin_node(&node.as_begin_node().unwrap());
        }
        Node::IfNode { .. } => {
            checker.visit_if_node(&node.as_if_node().unwrap());
        }
        Node::LocalVariableWriteNode { .. } => {
            checker.visit_local_variable_write_node(&node.as_local_variable_write_node().unwrap());
        }
        Node::LocalVariableOperatorWriteNode { .. } => {
            checker.visit_local_variable_operator_write_node(&node.as_local_variable_operator_write_node().unwrap());
        }
        Node::LocalVariableAndWriteNode { .. } => {
            checker.visit_local_variable_and_write_node(&node.as_local_variable_and_write_node().unwrap());
        }
        Node::LocalVariableOrWriteNode { .. } => {
            checker.visit_local_variable_or_write_node(&node.as_local_variable_or_write_node().unwrap());
        }
        _ => {} // Other node types — not assignment to local var
    }
}

/// Return value of a block body (last statement if lvar)
fn return_value<'a>(body: &Node<'a>) -> Option<Node<'a>> {
    if let Some(stmts) = body.as_statements_node() {
        let items: Vec<_> = stmts.body().iter().collect();
        items.into_iter().last()
    } else {
        // body itself is the single expression — but we can't clone Node
        // Re-extract: check what node types could be a single body
        // We match known single-value types
        if body.as_local_variable_read_node().is_some() {
            Some(body.as_local_variable_read_node().unwrap().as_node())
        } else {
            None
        }
    }
}

fn is_lvar_named<'a>(node: &Node<'a>) -> Option<String> {
    if let Some(lvar) = node.as_local_variable_read_node() {
        Some(String::from_utf8_lossy(lvar.name().as_slice()).to_string())
    } else {
        None
    }
}

fn is_simple_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::SymbolNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::StringNode { .. }
            | Node::NilNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
    )
}

struct EachWithObjectVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> EachWithObjectVisitor<'a> {
    fn check_inject_block(&mut self, call: &ruby_prism::CallNode, block: &ruby_prism::BlockNode) {
        let method_name = node_name!(call);

        // Must have initial value arg
        let method_arg = if let Some(args) = call.arguments() {
            let args_list: Vec<_> = args.arguments().iter().collect();
            if args_list.is_empty() {
                return;
            }
            args_list.into_iter().next().unwrap()
        } else {
            return;
        };

        // Initial value must not be a simple literal
        if is_simple_literal(&method_arg) {
            return;
        }

        // Block must have exactly 2 required params
        let block_params = match block.parameters() {
            Some(p) => p,
            None => return,
        };

        let params_node = match block_params.as_block_parameters_node() {
            Some(bp) => bp,
            None => return,
        };

        let req_params: Vec<_> = params_node
            .parameters()
            .map(|p| p.requireds().iter().collect::<Vec<_>>())
            .unwrap_or_default();

        if req_params.len() != 2 {
            return;
        }

        let acc_name = if let Some(rp) = req_params[0].as_required_parameter_node() {
            String::from_utf8_lossy(rp.name().as_slice()).to_string()
        } else {
            return;
        };

        // Block body must exist and return the accumulator
        let body = match block.body() {
            Some(b) => b,
            None => return,
        };

        let ret = match return_value(&body) {
            Some(v) => v,
            None => return,
        };

        let ret_name = match is_lvar_named(&ret) {
            Some(n) => n,
            None => return,
        };

        if ret_name != acc_name {
            return;
        }

        // Accumulator must not be reassigned in body
        if accumulator_assigned_in(&body, &acc_name) {
            return;
        }

        // Offense on selector
        if let Some(msg_loc) = call.message_loc() {
            let start = msg_loc.start_offset();
            let end = msg_loc.end_offset();
            let msg = format!("Use `each_with_object` instead of `{}`.", method_name);
            self.offenses.push(self.ctx.offense_with_range(
                "Style/EachWithObject",
                &msg,
                Severity::Convention,
                start,
                end,
            ));
        }
    }

    fn check_inject_numblock_body(&mut self, call: &ruby_prism::CallNode, body: &Node) {
        let method_name = node_name!(call);

        let method_arg = if let Some(args) = call.arguments() {
            let args_list: Vec<_> = args.arguments().iter().collect();
            if args_list.is_empty() {
                return;
            }
            args_list.into_iter().next().unwrap()
        } else {
            return;
        };

        if is_simple_literal(&method_arg) {
            return;
        }

        // Return value must be _1
        let ret = match return_value(body) {
            Some(v) => v,
            None => return,
        };

        let ret_name = match is_lvar_named(&ret) {
            Some(n) => n,
            None => return,
        };

        if ret_name != "_1" {
            return;
        }

        if let Some(msg_loc) = call.message_loc() {
            let start = msg_loc.start_offset();
            let end = msg_loc.end_offset();
            let msg = format!("Use `each_with_object` instead of `{}`.", method_name);
            self.offenses.push(self.ctx.offense_with_range(
                "Style/EachWithObject",
                &msg,
                Severity::Convention,
                start,
                end,
            ));
        }
    }

    fn check_inject_numblock(&mut self, call: &ruby_prism::CallNode, body: &Node) {
        let method_name = node_name!(call);

        let method_arg = if let Some(args) = call.arguments() {
            let args_list: Vec<_> = args.arguments().iter().collect();
            if args_list.is_empty() {
                return;
            }
            args_list.into_iter().next().unwrap()
        } else {
            return;
        };

        if is_simple_literal(&method_arg) {
            return;
        }

        // Return value must be _1
        let ret = match return_value(body) {
            Some(v) => v,
            None => return,
        };

        let ret_name = match is_lvar_named(&ret) {
            Some(n) => n,
            None => return,
        };

        if ret_name != "_1" {
            return;
        }

        if let Some(msg_loc) = call.message_loc() {
            let start = msg_loc.start_offset();
            let end = msg_loc.end_offset();
            let msg = format!("Use `each_with_object` instead of `{}`.", method_name);
            self.offenses.push(self.ctx.offense_with_range(
                "Style/EachWithObject",
                &msg,
                Severity::Convention,
                start,
                end,
            ));
        }
    }
}

impl<'a> Visit<'_> for EachWithObjectVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = node_name!(node);
        if method_name == "inject" || method_name == "reduce" {
            if let Some(block_node) = node.block() {
                if let Some(block) = block_node.as_block_node() {
                    let params = block.parameters();
                    let is_numbered = params.as_ref().map(|p| {
                        matches!(p, Node::NumberedParametersNode { .. })
                    }).unwrap_or(false);

                    if is_numbered {
                        // Numbered params block — check if _1 is returned
                        if let Some(body) = block.body() {
                            self.check_inject_numblock_body(node, &body);
                        }
                    } else {
                        self.check_inject_block(node, &block);
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

// We need to handle numblock via a different approach.
// In Prism, `[].reduce({}) do ... _1 ... end` — the block is a BlockNode
// with body containing `_1` as LocalVariableReadNode with name "_1".
// The NumberedParametersNode is the params node inside the block.
// So the block is still a BlockNode — we just check if params_node is NumberedParametersNode.

impl Cop for EachWithObject {
    fn name(&self) -> &'static str {
        "Style/EachWithObject"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = EachWithObjectVisitor {
            ctx,
            offenses: vec![],
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Style/EachWithObject", |_cfg| {
    Some(Box::new(EachWithObject::new()))
});
