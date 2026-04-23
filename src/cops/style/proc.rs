use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Use `proc` instead of `Proc.new`.";

#[derive(Default)]
pub struct Proc;

impl Proc {
    pub fn new() -> Self {
        Self
    }

    /// Check if a BlockNode (or NumblockNode/ItBlockNode) has `Proc.new` as its call
    fn check_block_receiver(block_receiver: &Node) -> bool {
        // block_receiver should be a CallNode: (const :Proc) :new
        let call = match block_receiver.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        if node_name!(call) != "new" {
            return false;
        }
        if call.arguments().is_some() {
            return false;
        }
        let receiver = match call.receiver() {
            Some(r) => r,
            None => return false,
        };
        // receiver should be constant Proc or ::Proc
        match &receiver {
            Node::ConstantReadNode { .. } => {
                let name = receiver.as_constant_read_node().unwrap();
                name.name().as_slice() == b"Proc"
            }
            Node::ConstantPathNode { .. } => {
                let path = receiver.as_constant_path_node().unwrap();
                // ::Proc — parent is None (cbase)
                if path.parent().is_some() {
                    return false;
                }
                path.name().map_or(false, |id| id.as_slice() == b"Proc")
            }
            _ => false,
        }
    }
}

impl Cop for Proc {
    fn name(&self) -> &'static str {
        "Style/Proc"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_block(&self, node: &ruby_prism::BlockNode, ctx: &CheckContext) -> Vec<Offense> {
        // BlockNode: get the call that this block is attached to
        // In Prism, block nodes don't directly expose their receiver call.
        // We check via the source text: look at what appears before the block's opening.
        // Actually, we need to use check_call + look at blocks from call side.
        // Better: implement via check_call where method=new and receiver=Proc
        // and the call has a block (block_node is Some).
        // We can't do that from check_block. Use check_call instead.
        let _ = (node, ctx);
        vec![]
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "new" {
            return vec![];
        }
        if node.block().is_none() {
            return vec![];
        }
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let is_proc = match &receiver {
            Node::ConstantReadNode { .. } => {
                let name = receiver.as_constant_read_node().unwrap();
                name.name().as_slice() == b"Proc"
            }
            Node::ConstantPathNode { .. } => {
                let path = receiver.as_constant_path_node().unwrap();
                if path.parent().is_some() {
                    false
                } else {
                    path.name().map_or(false, |id| id.as_slice() == b"Proc")
                }
            }
            _ => false,
        };
        if !is_proc {
            return vec![];
        }
        // Offense covers the entire `Proc.new` (receiver + dot + new)
        let start = receiver.location().start_offset();
        let end = node.message_loc()
            .map(|l| l.end_offset())
            .unwrap_or_else(|| node.location().end_offset());
        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)]
    }
}

crate::register_cop!("Style/Proc", |_cfg| Some(Box::new(Proc::new())));
