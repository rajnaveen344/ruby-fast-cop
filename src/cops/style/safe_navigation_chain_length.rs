//! Style/SafeNavigationChainLength cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct SafeNavigationChainLength {
    max: usize,
}

impl SafeNavigationChainLength {
    pub fn new(max: usize) -> Self { Self { max } }
}

impl Default for SafeNavigationChainLength {
    fn default() -> Self { Self { max: 2 } }
}

impl Cop for SafeNavigationChainLength {
    fn name(&self) -> &'static str { "Style/SafeNavigationChainLength" }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, max: self.max, offenses: Vec::new(), parent_is_csend_with_self_recv: false };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    max: usize,
    offenses: Vec<Offense>,
    /// set when recursing into a node whose parent call is csend and
    /// this node is its receiver — i.e. this node is NOT outermost of its chain.
    parent_is_csend_with_self_recv: bool,
}

impl<'a> V<'a> {
    fn csend_chain_length(node: Node<'a>) -> usize {
        let mut n = node;
        let mut count = 0;
        loop {
            let call = match n.as_call_node() { Some(c) => c, None => break };
            if !call.is_safe_navigation() { break; }
            count += 1;
            n = match call.receiver() { Some(r) => r, None => break };
        }
        count
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        let is_csend = node.is_safe_navigation();
        // Evaluate: is this node outermost csend of a chain?
        // outermost iff csend AND parent wasn't a csend w/ this as receiver.
        if is_csend && !self.parent_is_csend_with_self_recv {
            let chain = Self::csend_chain_length(node.as_node());
            if chain > self.max {
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                let msg = format!(
                    "Avoid safe navigation chains longer than {} calls.",
                    self.max
                );
                self.offenses.push(self.ctx.offense_with_range(
                    "Style/SafeNavigationChainLength",
                    &msg,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }

        // Recurse into receiver with flag propagated
        if let Some(recv) = node.receiver() {
            let prev = self.parent_is_csend_with_self_recv;
            self.parent_is_csend_with_self_recv = is_csend;
            self.visit(&recv);
            self.parent_is_csend_with_self_recv = prev;
        }
        // Recurse into args/block w/ flag cleared
        let prev = self.parent_is_csend_with_self_recv;
        self.parent_is_csend_with_self_recv = false;
        if let Some(args) = node.arguments() {
            for a in args.arguments().iter() { self.visit(&a); }
        }
        if let Some(block) = node.block() { self.visit(&block); }
        self.parent_is_csend_with_self_recv = prev;
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { max: Option<usize> }

crate::register_cop!("Style/SafeNavigationChainLength", |cfg| {
    let c: Cfg = cfg.typed("Style/SafeNavigationChainLength");
    Some(Box::new(SafeNavigationChainLength::new(c.max.unwrap_or(2))))
});
