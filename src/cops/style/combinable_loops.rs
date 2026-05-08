//! Style/CombinableLoops cop
//!
//! Checks for consecutive loops over the same collection that can be combined.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
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

    /// Get the block node from a CallNode that has a block.
    fn get_block<'pr>(node: &'pr Node<'pr>) -> Option<ruby_prism::BlockNode<'pr>> {
        let call = node.as_call_node()?;
        let block = call.block()?;
        block.as_block_node()
    }

    /// Get the block params text from a block node (the `|...|` or empty string).
    fn block_params_src<'pr>(block: &ruby_prism::BlockNode<'pr>, source: &str) -> String {
        if let Some(params) = block.parameters() {
            if let Some(bp) = params.as_block_parameters_node() {
                let loc = bp.location();
                return source[loc.start_offset()..loc.end_offset()].to_string();
            }
            // NumberedParametersNode or ItParametersNode — no explicit params
        }
        String::new()
    }

    /// Build correction for merging curr block loop into prev block loop.
    /// Returns None if params don't match or no body.
    fn build_block_correction<'pr>(
        &self,
        prev: &Node<'pr>,
        curr: &Node<'pr>,
        next_sibling_is_block: bool,
    ) -> Option<Correction> {
        let prev_block = Self::get_block(prev)?;
        let curr_block = Self::get_block(curr)?;

        // Check params match (RuboCop: skip correction if different var names)
        let prev_params = Self::block_params_src(&prev_block, self.ctx.source);
        let curr_params = Self::block_params_src(&curr_block, self.ctx.source);
        if prev_params != curr_params {
            return None;
        }

        let prev_body = prev_block.body()?;
        let curr_body = curr_block.body()?;

        // Op1: remove from prev body end to prev block closing delimiter
        let prev_body_end = prev_body.location().end_offset();
        let prev_closing_end = prev_block.closing_loc().end_offset();

        // Op2: remove from curr node start to curr body start
        let curr_node_start = curr.location().start_offset();
        let curr_body_start = curr_body.location().start_offset();

        // Op3: correct_end_of_block
        // Determine if prev uses braces
        let prev_opening = &self.ctx.source[prev_block.opening_loc().start_offset()..prev_block.opening_loc().end_offset()];
        let prev_is_braces = prev_opening == "{";
        let end_of_block = if prev_is_braces { "}" } else { " end" };

        // curr closing delimiter
        let curr_closing_start = curr_block.closing_loc().start_offset();
        let curr_closing_end = curr_block.closing_loc().end_offset();

        let mut edits = Vec::new();

        // Op1: remove from prev_body_end to prev_closing_end (removes closing delimiter of prev)
        edits.push(Edit {
            start_offset: prev_body_end,
            end_offset: prev_closing_end,
            replacement: String::new(),
        });

        // Op2: remove from curr_node_start to curr_body_start
        edits.push(Edit {
            start_offset: curr_node_start,
            end_offset: curr_body_start,
            replacement: String::new(),
        });

        // Op3: replace curr closing with appropriate end_of_block (if needed)
        if !next_sibling_is_block {
            let curr_closing_src = &self.ctx.source[curr_closing_start..curr_closing_end];
            let curr_is_end = curr_closing_src == "end";
            // Remove curr closing and insert correct one
            if curr_is_end && !prev_is_braces {
                // both do-end, no change needed to closing — but we still remove+reinsert
                // Actually: remove(node.loc.end) + insert_before(node.source_range.end, end_of_block)
                // node.source_range.end = end of the whole curr node
                let curr_node_end = curr.location().end_offset();
                edits.push(Edit {
                    start_offset: curr_closing_start,
                    end_offset: curr_closing_end,
                    replacement: String::new(),
                });
                edits.push(Edit {
                    start_offset: curr_node_end,
                    end_offset: curr_node_end,
                    replacement: end_of_block.to_string(),
                });
            } else if !curr_is_end && prev_is_braces {
                // both braces, same logic
                let curr_node_end = curr.location().end_offset();
                edits.push(Edit {
                    start_offset: curr_closing_start,
                    end_offset: curr_closing_end,
                    replacement: String::new(),
                });
                edits.push(Edit {
                    start_offset: curr_node_end,
                    end_offset: curr_node_end,
                    replacement: end_of_block.to_string(),
                });
            } else {
                // Mixed: prev is braces, curr is do-end, or vice versa
                let curr_node_end = curr.location().end_offset();
                edits.push(Edit {
                    start_offset: curr_closing_start,
                    end_offset: curr_closing_end,
                    replacement: String::new(),
                });
                edits.push(Edit {
                    start_offset: curr_node_end,
                    end_offset: curr_node_end,
                    replacement: end_of_block.to_string(),
                });
            }
        }

        Some(Correction { edits })
    }

    /// Build correction for merging curr for-loop into prev for-loop.
    fn build_for_correction<'pr>(
        &self,
        prev: &Node<'pr>,
        curr: &Node<'pr>,
    ) -> Option<Correction> {
        let prev_for = prev.as_for_node()?;
        let curr_for = curr.as_for_node()?;

        let prev_stmts = prev_for.statements()?;
        let curr_stmts = curr_for.statements()?;

        // Op1: remove from prev body end to prev `end` end
        let prev_body_end = prev_stmts.location().end_offset();
        let prev_end_kw_end = prev_for.end_keyword_loc().end_offset();

        // Op2: remove from curr start to curr body start
        let curr_node_start = curr.location().start_offset();
        let curr_body_start = curr_stmts.location().start_offset();

        // For for-loops, RuboCop's correct_end_of_block returns immediately
        // (no :braces? method), so no Op3 needed.

        let edits = vec![
            Edit {
                start_offset: prev_body_end,
                end_offset: prev_end_kw_end,
                replacement: String::new(),
            },
            Edit {
                start_offset: curr_node_start,
                end_offset: curr_body_start,
                replacement: String::new(),
            },
        ];

        Some(Correction { edits })
    }

    fn check_statements(&mut self, stmts: &[Node]) {
        for i in 1..stmts.len() {
            let curr = &stmts[i];
            let prev = &stmts[i - 1];
            let next_is_block = i + 1 < stmts.len() && self.block_loop_key(&stmts[i + 1]).is_some();

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
                    let correction = self.build_block_correction(prev, curr, next_is_block);
                    let offense = self.ctx.offense_with_range(
                        "Style/CombinableLoops",
                        MSG,
                        Severity::Convention,
                        start,
                        end,
                    );
                    self.offenses.push(if let Some(c) = correction {
                        offense.with_correction(c)
                    } else {
                        offense
                    });
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
                        let correction = self.build_for_correction(prev, curr);
                        let offense = self.ctx.offense_with_range(
                            "Style/CombinableLoops",
                            MSG,
                            Severity::Convention,
                            start,
                            end,
                        );
                        self.offenses.push(if let Some(c) = correction {
                            offense.with_correction(c)
                        } else {
                            offense
                        });
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
