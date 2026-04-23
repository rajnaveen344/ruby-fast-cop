//! Layout/SpaceAfterNot - Checks for space after `!`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_after_not.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct SpaceAfterNot;

impl SpaceAfterNot {
    pub fn new() -> Self {
        Self
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        let source = self.ctx.source;
        let bytes = source.as_bytes();

        // Check if this is a `!` prefix call (prefix bang)
        let method_bytes = node.name().as_slice();
        if method_bytes == b"!" {
            let loc = node.location();
            let node_start = loc.start_offset();

            // Check if first byte is `!`
            if bytes.get(node_start).copied() == Some(b'!') {
                // receiver start
                if let Some(recv) = node.receiver() {
                    let recv_start = recv.location().start_offset();
                    // If receiver doesn't start right after `!`, there's space
                    if recv_start > node_start + 1 {
                        let node_end = loc.end_offset();
                        let correction = Correction::delete(node_start + 1, recv_start);
                        self.offenses.push(Offense::new(
                            "Layout/SpaceAfterNot",
                            "Do not leave space between `!` and its argument.",
                            Severity::Convention,
                            Location::from_offsets(source, node_start, node_end),
                            self.ctx.filename,
                        ).with_correction(correction));
                    }
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for SpaceAfterNot {
    fn name(&self) -> &'static str {
        "Layout/SpaceAfterNot"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

crate::register_cop!("Layout/SpaceAfterNot", |_cfg| {
    Some(Box::new(SpaceAfterNot::new()))
});
