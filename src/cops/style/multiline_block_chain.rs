//! Style/MultilineBlockChain cop
//!
//! Checks for chaining a block after a multiline block.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/MultilineBlockChain";
const MSG: &str = "Avoid multi-line chains of blocks.";

#[derive(Default)]
pub struct MultilineBlockChain;

impl MultilineBlockChain {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for MultilineBlockChain {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MultilineBlockChainVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct MultilineBlockChainVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> MultilineBlockChainVisitor<'a> {
    /// Returns the block node if `node` is a call that has a block as its block argument.
    /// Handles CallNode (do/end and {}) cases.
    fn block_of_call<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
        let call = node.as_call_node()?;
        let block = call.block()?;
        // Must be a BlockNode (do...end or {...})
        if block.as_block_node().is_some() {
            return Some(block);
        }
        // NumberedParametersNode / ItParametersNode also count
        if matches!(block, Node::NumberedParametersNode { .. } | Node::ItParametersNode { .. }) {
            return Some(block);
        }
        None
    }

    /// Returns true if the node spans multiple lines.
    fn is_multiline_node(node: &Node, ctx: &CheckContext) -> bool {
        let loc = node.location();
        ctx.line_of(loc.start_offset()) < ctx.line_of(loc.end_offset())
    }

    /// Walk the receiver chain of `call` to find the first call node whose block is multiline.
    /// Returns Some((end_keyword_start, message_loc_end_of_outermost)).
    ///
    /// RuboCop: for each call in send_node, find receiver that is any_block_type and multiline.
    /// Offense = range_between(receiver.loc.end.begin_pos, node.send_node.source_range.end_pos)
    ///
    /// In Prism: receiver.loc.end.begin_pos = closing_loc().start_offset() of the block
    /// node.send_node.source_range.end_pos = outer_call.message_loc().end_offset()
    fn find_multiline_block_in_receiver_chain(
        call: &ruby_prism::CallNode,
        ctx: &CheckContext,
    ) -> Option<(usize, usize)> {
        // Walk the receiver chain, collecting the call chain nodes
        let outer_msg_end = call.message_loc()?.end_offset();

        let mut current_recv = call.receiver()?;
        loop {
            // Check if current_recv is a call whose block is multiline
            if let Some(block) = Self::block_of_call(&current_recv) {
                if Self::is_multiline_node(&current_recv, ctx) {
                    // Get closing_loc of the block (the `end` keyword start)
                    let end_kw_start = closing_loc_start(&block)?;
                    return Some((end_kw_start, outer_msg_end));
                }
                // The receiver is a call with a block but NOT multiline — don't flag
                return None;
            }
            // Check if current_recv is a plain call (no block) — walk deeper
            if let Some(inner_call) = current_recv.as_call_node() {
                // This call has no block in the chain; check if ITS receiver continues
                if let Some(inner_recv) = inner_call.receiver() {
                    current_recv = inner_recv;
                } else {
                    return None;
                }
            } else {
                // Not a call at all (e.g. local variable, literal) — no block in chain
                return None;
            }
        }
    }
}

/// Get the start offset of the closing `end` (or `}`) of a block node.
fn closing_loc_start(block: &Node) -> Option<usize> {
    if let Some(bn) = block.as_block_node() {
        // BlockNode.closing_loc() returns Location (not Option)
        return Some(bn.closing_loc().start_offset());
    }
    // NumberedParametersNode / ItParametersNode: use end of the node location minus 3 for "end"
    // Actually these wrap the whole block including end keyword.
    // Fallback: use start of the last line of the node.
    let loc = block.location();
    Some(loc.end_offset().saturating_sub(3)) // approximate
}

impl Visit<'_> for MultilineBlockChainVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Only check calls that have a block (do...end or {...})
        if node.block().is_some() {
            if let Some((end_kw_start, msg_end)) =
                Self::find_multiline_block_in_receiver_chain(node, self.ctx)
            {
                let offense = self.ctx.offense_with_range(
                    COP_NAME, MSG, Severity::Convention, end_kw_start, msg_end,
                );
                self.offenses.push(offense);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/MultilineBlockChain", |_cfg| {
    Some(Box::new(MultilineBlockChain::new()))
});
