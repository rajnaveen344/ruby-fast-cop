//! Lint/RedundantWithIndex - Detect redundant `with_index` when index is unused.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_EACH_WITH_INDEX: &str = "Use `each` instead of `each_with_index`.";
const MSG_WITH_INDEX: &str = "Remove redundant `with_index`.";

#[derive(Default)]
pub struct RedundantWithIndex;

impl RedundantWithIndex {
    pub fn new() -> Self { Self }
}

impl Cop for RedundantWithIndex {
    fn name(&self) -> &'static str { "Lint/RedundantWithIndex" }
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

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        let method_str = method.as_ref();

        if method_str == "each_with_index" {
            // Must have a block, receiver must be a call (not bare `each_with_index`)
            if node.receiver().is_some() {
                if let Some(block) = node.block() {
                    if let Some(block_node) = block.as_block_node() {
                        if !index_used_in_block(&block_node) {
                            self.report_each_with_index(node);
                        }
                    }
                }
            }
        } else if method_str == "with_index" {
            // Must have a receiver that is itself a call
            if let Some(recv) = node.receiver() {
                // Receiver must be a chained call (itself has a receiver), not bare `ary.with_index`
                if let Some(recv_call) = recv.as_call_node() {
                    if recv_call.receiver().is_none() {
                        // Bare call like `ary.with_index` — skip
                        ruby_prism::visit_call_node(self, node);
                        return;
                    }
                    if let Some(block) = node.block() {
                        if let Some(block_node) = block.as_block_node() {
                            if !index_used_in_block(&block_node) {
                                self.report_with_index(node);
                            }
                        }
                    }
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn report_each_with_index(&mut self, node: &ruby_prism::CallNode) {
        // Offense at message_loc (the "each_with_index" identifier)
        if let Some(msg_loc) = node.message_loc() {
            let start = msg_loc.start_offset();
            let end = msg_loc.end_offset();

            // Correction: replace "each_with_index" with "each"
            let correction = Correction::replace(start, end, "each");

            self.offenses.push(
                self.ctx.offense_with_range(
                    "Lint/RedundantWithIndex",
                    MSG_EACH_WITH_INDEX,
                    Severity::Warning,
                    start,
                    end,
                ).with_correction(correction),
            );
        }
    }

    fn report_with_index(&mut self, node: &ruby_prism::CallNode) {
        // Offense at message_loc (the "with_index" identifier)
        if let Some(msg_loc) = node.message_loc() {
            let offense_start = msg_loc.start_offset();
            // Offense ends at closing paren if args present (e.g. with_index(1) → col 22)
            // or at message_loc end if no args
            let offense_end = if let Some(close) = node.closing_loc() {
                close.end_offset()
            } else {
                msg_loc.end_offset()
            };

            // Correction: delete from call_operator_loc start through closing paren (if any args)
            // i.e., delete ".with_index" or ".with_index(1)"
            let delete_start = if let Some(op_loc) = node.call_operator_loc() {
                op_loc.start_offset()
            } else {
                offense_start - 1 // fallback: include the dot
            };
            let delete_end = offense_end;

            let correction = Correction::delete(delete_start, delete_end);

            self.offenses.push(
                self.ctx.offense_with_range(
                    "Lint/RedundantWithIndex",
                    MSG_WITH_INDEX,
                    Severity::Warning,
                    offense_start,
                    offense_end,
                ).with_correction(correction),
            );
        }
    }
}

/// Returns true if the block uses the index (second block param or _2+ in numblock).
fn index_used_in_block(block: &ruby_prism::BlockNode) -> bool {
    // Check named parameters: if 2+ parameters, index is bound; check if it's actually used
    // RuboCop flags when index param is not bound at all (only 1 named param)
    // If 2+ params bound, the index IS bound → not redundant
    if let Some(params) = block.parameters() {
        if let Some(block_params) = params.as_block_parameters_node() {
            // BlockParametersNode.parameters() returns Option<ParametersNode>
            if let Some(inner_params) = block_params.parameters() {
                let required_count = inner_params.requireds().iter().count();
                if required_count >= 2 {
                    return true; // index is bound as a named param
                }
            }
        }
    }

    // For numblocks: check if body references _2 or higher
    if let Some(body) = block.body() {
        if body_uses_numbered_param_2_or_higher(&body) {
            return true;
        }
    }

    false
}

/// Returns true if the node or any descendant is a LocalVariableReadNode for _2, _3, etc.
fn body_uses_numbered_param_2_or_higher(node: &Node) -> bool {
    match node {
        Node::LocalVariableReadNode { .. } => {
            let n = node.as_local_variable_read_node().unwrap();
            let name = String::from_utf8_lossy(n.name().as_slice());
            is_numbered_param_2_or_higher(name.as_ref())
        }
        Node::LocalVariableWriteNode { .. } => {
            let n = node.as_local_variable_write_node().unwrap();
            body_uses_numbered_param_2_or_higher(&n.value())
        }
        Node::StatementsNode { .. } => {
            let stmts = node.as_statements_node().unwrap();
            stmts.body().iter().any(|child| body_uses_numbered_param_2_or_higher(&child))
        }
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                if body_uses_numbered_param_2_or_higher(&recv) {
                    return true;
                }
            }
            if let Some(args) = call.arguments() {
                if args.arguments().iter().any(|a| body_uses_numbered_param_2_or_higher(&a)) {
                    return true;
                }
            }
            false
        }
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            if body_uses_numbered_param_2_or_higher(&n.predicate()) { return true; }
            if let Some(stmts) = n.statements() {
                if stmts.body().iter().any(|c| body_uses_numbered_param_2_or_higher(&c)) { return true; }
            }
            if let Some(sub) = n.subsequent() {
                if body_uses_numbered_param_2_or_higher(&sub) { return true; }
            }
            false
        }
        _ => false,
    }
}

fn is_numbered_param_2_or_higher(name: &str) -> bool {
    // _2, _3, ..., _9
    if name.len() == 2 && name.starts_with('_') {
        let c = name.as_bytes()[1];
        return c >= b'2' && c <= b'9';
    }
    false
}

crate::register_cop!("Lint/RedundantWithIndex", |_cfg| Some(Box::new(RedundantWithIndex::new())));
