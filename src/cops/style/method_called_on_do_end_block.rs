//! Style/MethodCalledOnDoEndBlock - Checks for methods called on do...end blocks.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/method_called_on_do_end_block.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

/// Checks for methods called on a do...end block. The point of this check is that
/// it's easy to miss the call tacked on to the block when reading code.
///
/// # Examples
///
/// ```ruby
/// # bad
/// a do
///   b
/// end.c
///
/// # good
/// a { b }.c
///
/// # good
/// foo = a do
///   b
/// end
/// foo.c
/// ```
#[derive(Default)]
pub struct MethodCalledOnDoEndBlock;

impl MethodCalledOnDoEndBlock {
    pub fn new() -> Self {
        Self
    }

    /// Check if a Node is a do...end block
    fn is_do_end_block_node(&self, node: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        if let ruby_prism::Node::BlockNode { .. } = node {
            let block = node.as_block_node().unwrap();
            // Check if it uses 'do' keyword by looking at the opening
            let open_loc = block.opening_loc();
            let start = open_loc.start_offset();
            let end = open_loc.end_offset();
            if let Some(text) = ctx.source.get(start..end) {
                return text == "do";
            }
        }
        false
    }
}

impl Cop for MethodCalledOnDoEndBlock {
    fn name(&self) -> &'static str {
        "Style/MethodCalledOnDoEndBlock"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // If this call itself has a block, it's allowed (chained block pattern)
        // e.g., `a do...end.each do...end` is acceptable
        if node.block().is_some() {
            return vec![];
        }

        // Check if the receiver is a do...end block
        // The AST structure for `a do...end.c` has the CallNode with block as receiver
        if let Some(receiver) = node.receiver() {
            // Check for CallNode that has a block attached (the do...end part)
            if let ruby_prism::Node::CallNode { .. } = &receiver {
                let recv_call = receiver.as_call_node().unwrap();
                if let Some(ref block) = recv_call.block() {
                    if self.is_do_end_block_node(block, ctx) {
                        // Get the block's closing location ("end") and the method location
                        // The offense should highlight from "end" through the method call
                        let block_node = block.as_block_node().unwrap();
                        let closing_loc = block_node.closing_loc();
                        let closing_start = closing_loc.start_offset();

                        // Get the end of the method call (call operator + method name)
                        let call_end = if let Some(msg_loc) = node.message_loc() {
                            msg_loc.end_offset()
                        } else if let Some(op_loc) = node.call_operator_loc() {
                            op_loc.end_offset()
                        } else {
                            node.location().end_offset()
                        };

                        return vec![ctx.offense_with_range(
                            self.name(),
                            "Avoid chaining a method call on a do...end block.",
                            self.severity(),
                            closing_start,
                            call_end,
                        )];
                    }
                }
            }
        }
        vec![]
    }
}

crate::register_cop!("Style/MethodCalledOnDoEndBlock", |_cfg| {
    Some(Box::new(MethodCalledOnDoEndBlock::new()))
});
