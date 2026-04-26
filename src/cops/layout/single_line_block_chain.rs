//! Layout/SingleLineBlockChain - Method call chained on the same line as a single-line block.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/single_line_block_chain.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct SingleLineBlockChain;

impl SingleLineBlockChain {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for SingleLineBlockChain {
    fn name(&self) -> &'static str {
        "Layout/SingleLineBlockChain"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let (block_open_line, block_close_line) = match &receiver {
            ruby_prism::Node::CallNode { .. } => {
                let recv_call = receiver.as_call_node().unwrap();
                let block = match recv_call.block() {
                    Some(b) => b,
                    None => return vec![],
                };
                match get_block_lines(&block, ctx) {
                    Some(v) => v,
                    None => return vec![],
                }
            }
            ruby_prism::Node::LambdaNode { .. } => match get_block_lines(&receiver, ctx) {
                Some(v) => v,
                None => return vec![],
            },
            _ => return vec![],
        };

        if block_open_line < block_close_line {
            return vec![];
        }

        let dot_loc = match node.call_operator_loc() {
            Some(l) => l,
            None => return vec![],
        };

        let dot_line = ctx.line_of(dot_loc.start_offset());
        let dot_col = ctx.col_of(dot_loc.start_offset());

        let (sel_end, sel_col) = if let Some(m) = node.message_loc() {
            (m.end_offset(), ctx.col_of(m.start_offset()))
        } else if let Some(o) = node.opening_loc() {
            (o.end_offset(), ctx.col_of(o.start_offset()))
        } else {
            return vec![];
        };

        if dot_line > block_close_line {
            return vec![];
        }
        if dot_col >= sel_col {
            return vec![];
        }

        let start = dot_loc.start_offset();
        let end = sel_end;
        let mut off = ctx.offense_with_range(
            self.name(),
            "Put method call on a separate line if chained to a single line block.",
            self.severity(),
            start,
            end,
        );
        off.correction = Some(Correction::insert(start, "\n"));
        vec![off]
    }
}

fn get_block_lines(node: &ruby_prism::Node, ctx: &CheckContext) -> Option<(usize, usize)> {
    match node {
        ruby_prism::Node::BlockNode { .. } => {
            let b = node.as_block_node().unwrap();
            let open = b.opening_loc();
            let close = b.closing_loc();
            Some((
                ctx.line_of(open.start_offset()),
                ctx.line_of(close.start_offset()),
            ))
        }
        ruby_prism::Node::LambdaNode { .. } => {
            let l = node.as_lambda_node().unwrap();
            let open = l.opening_loc();
            let close = l.closing_loc();
            Some((
                ctx.line_of(open.start_offset()),
                ctx.line_of(close.start_offset()),
            ))
        }
        _ => None,
    }
}

crate::register_cop!("Layout/SingleLineBlockChain", |_cfg| Some(Box::new(
    SingleLineBlockChain::new()
)));
