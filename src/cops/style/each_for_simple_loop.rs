//! Style/EachForSimpleLoop cop
//!
//! Checks for loops using Range#each that can be replaced with Integer#times.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Use `Integer#times` for a simple loop which iterates a fixed number of times.";

#[derive(Default)]
pub struct EachForSimpleLoop;

impl EachForSimpleLoop {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EachForSimpleLoop {
    fn name(&self) -> &'static str {
        "Style/EachForSimpleLoop"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = EachForSimpleLoopVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EachForSimpleLoopVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> EachForSimpleLoopVisitor<'a> {
    fn int_value(node: &Node) -> Option<i64> {
        node.as_integer_node().and_then(|n| {
            let v = n.value();
            // ruby_prism::Integer can be converted via its bytes
            // Try small values first via i32
            let src = &n.location().as_slice();
            std::str::from_utf8(src).ok()
                .and_then(|s| s.trim_start_matches('-').parse::<i64>().ok()
                    .map(|v| if src.starts_with(&b"-"[..]) { -v } else { v }))
        })
    }

    /// Check a CallNode that ends a block — called from visit_call_node
    /// when the call has a block and method is `each`.
    fn check_call_with_block(&mut self, call: &ruby_prism::CallNode) {
        // Method must be `each`
        if call.name().as_slice() != b"each" {
            return;
        }

        // Must have a block
        let block_node = match call.block() {
            Some(b) => b,
            None => return,
        };

        // Block must have no parameters
        if let Some(block) = block_node.as_block_node() {
            let has_params = if let Some(params) = block.parameters() {
                match params {
                    Node::BlockParametersNode { .. } => {
                        let bp = params.as_block_parameters_node().unwrap();
                        if let Some(inner_params) = bp.parameters() {
                            // inner_params is ParametersNode directly
                            !inner_params.requireds().is_empty()
                                || !inner_params.optionals().is_empty()
                                || inner_params.rest().is_some()
                                || !inner_params.posts().is_empty()
                                || !inner_params.keywords().is_empty()
                        } else {
                            false
                        }
                    }
                    Node::NumberedParametersNode { .. } => false, // numbered params — no explicit params
                    Node::ItParametersNode { .. } => false,
                    _ => false,
                }
            } else {
                false
            };

            if has_params {
                return;
            }
        }

        // Receiver must be a parenthesized range
        let recv = match call.receiver() {
            Some(r) => r,
            None => return,
        };

        // Unwrap parentheses
        let range_node = if let Some(paren) = recv.as_parentheses_node() {
            match paren.body() {
                Some(body) => {
                    if let Some(stmts) = body.as_statements_node() {
                        let mut items = stmts.body().iter();
                        let first = match items.next() { Some(n) => n, None => return };
                        if items.next().is_some() { return; }
                        first
                    } else {
                        body
                    }
                }
                None => return,
            }
        } else {
            return;
        };

        // Must be a RangeNode with integer bounds
        let range = match range_node.as_range_node() {
            Some(r) => r,
            None => return,
        };

        let left = match range.left() {
            Some(l) => l,
            None => return,
        };
        let right = match range.right() {
            Some(r) => r,
            None => return,
        };

        let min = match Self::int_value(&left) {
            Some(v) => v,
            None => return,
        };
        let max = match Self::int_value(&right) {
            Some(v) => v,
            None => return,
        };

        // Check if block has args (for non-zero origin, block args make it ineligible)
        let block_has_args = call.block().and_then(|b| b.as_block_node()).map(|bn| {
            bn.parameters().map(|p| match p {
                Node::BlockParametersNode { .. } => true,
                _ => false,
            }).unwrap_or(false)
        }).unwrap_or(false);

        // Conditions from Ruby source:
        // each_range_with_zero_origin? — (0...n).each no args: min==0, no block args required
        // each_range_without_block_argument? — any int range, no block args
        // Combined: offending if no block args (already checked above) OR zero origin
        // Since we checked no block args above for BlockParametersNode, and allow NumberedParametersNode...
        // Actually: we need to check "no block args" (no explicit block param node)

        let start = recv.location().start_offset();
        let end = if let Some(msg_loc) = call.message_loc() {
            msg_loc.end_offset()
        } else {
            call.location().end_offset()
        };

        self.offenses.push(self.ctx.offense_with_range(
            "Style/EachForSimpleLoop",
            MSG,
            Severity::Convention,
            start,
            end,
        ));
    }
}

impl<'a> Visit<'_> for EachForSimpleLoopVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call_with_block(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/EachForSimpleLoop", |_cfg| {
    Some(Box::new(EachForSimpleLoop::new()))
});
