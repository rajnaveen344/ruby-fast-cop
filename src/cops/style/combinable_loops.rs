//! Style/CombinableLoops cop
//!
//! Checks for consecutive loops over the same collection that can be combined.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Combine this loop with the previous loop.";

#[derive(Default)]
pub struct CombinableLoops;

impl CombinableLoops {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for CombinableLoops {
    fn name(&self) -> &'static str {
        "Style/CombinableLoops"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = CombinableLoopsVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct CombinableLoopsVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> CombinableLoopsVisitor<'a> {
    fn node_src(&self, node: &Node) -> &str {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        &self.ctx.source[start..end]
    }

    /// Extract loop key from a CallNode that has a block:
    /// (receiver_src, method_name, send_args_src)
    fn call_loop_key(&self, call: &ruby_prism::CallNode) -> Option<(String, String, String)> {
        let method = String::from_utf8_lossy(call.name().as_slice()).to_string();

        // Method must start with 'each' or end with '_each'
        if !method.starts_with("each") && !method.ends_with("_each") {
            return None;
        }

        let recv_src = match call.receiver() {
            Some(r) => self.node_src(&r).to_string(),
            None => String::new(),
        };

        // Include send arguments in key to distinguish each_slice(2) from each_slice(3)
        let send_args_src = match call.arguments() {
            Some(a) => self.node_src(&a.as_node()).to_string(),
            None => String::new(),
        };

        Some((recv_src, method, send_args_src))
    }

    /// Check if a node is a block-style loop (call with block) and extract its key
    fn block_loop_key(&self, node: &Node) -> Option<(String, String, String)> {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                // Must have a block
                if call.block().is_none() {
                    return None;
                }
                self.call_loop_key(&call)
            }
            _ => None,
        }
    }

    /// Check if a node has a non-empty body
    fn has_body(&self, node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if let Some(block) = call.block() {
                    if let Some(bn) = block.as_block_node() {
                        return bn.body().is_some();
                    }
                }
                false
            }
            Node::ForNode { .. } => {
                node.as_for_node().unwrap().statements().is_some()
            }
            _ => false,
        }
    }

    fn for_collection_src(&self, node: &Node) -> Option<String> {
        let for_node = node.as_for_node()?;
        Some(self.node_src(&for_node.collection()).to_string())
    }

    fn check_statements(&mut self, stmts: &[Node]) {
        for i in 1..stmts.len() {
            let curr = &stmts[i];
            let prev = &stmts[i - 1];

            if !self.has_body(curr) || !self.has_body(prev) {
                continue;
            }

            // Check block loops (CallNode with block)
            if let (Some(curr_key), Some(prev_key)) = (
                self.block_loop_key(curr),
                self.block_loop_key(prev),
            ) {
                if curr_key == prev_key {
                    let start = curr.location().start_offset();
                    let end = curr.location().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Style/CombinableLoops",
                        MSG,
                        Severity::Convention,
                        start,
                        end,
                    ));
                }
                continue;
            }

            // Check for loops
            if matches!(curr, Node::ForNode { .. }) && matches!(prev, Node::ForNode { .. }) {
                if let (Some(curr_coll), Some(prev_coll)) = (
                    self.for_collection_src(curr),
                    self.for_collection_src(prev),
                ) {
                    if curr_coll == prev_coll {
                        let start = curr.location().start_offset();
                        let end = curr.location().end_offset();
                        self.offenses.push(self.ctx.offense_with_range(
                            "Style/CombinableLoops",
                            MSG,
                            Severity::Convention,
                            start,
                            end,
                        ));
                    }
                }
            }
        }
    }
}

impl<'a> Visit<'_> for CombinableLoopsVisitor<'a> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode) {
        let children: Vec<_> = node.body().iter().collect();
        self.check_statements(&children);
        ruby_prism::visit_statements_node(self, node);
    }
}

crate::register_cop!("Style/CombinableLoops", |_cfg| {
    Some(Box::new(CombinableLoops::new()))
});
