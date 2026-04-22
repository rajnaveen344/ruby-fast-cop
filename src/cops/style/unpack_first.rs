//! Style/UnpackFirst cop
//!
//! Checks for .unpack(...).first / [0] / .slice(0) / .at(0) — suggest unpack1.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/UnpackFirst";

#[derive(Default)]
pub struct UnpackFirst;

impl UnpackFirst {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for UnpackFirst {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 4) {
            return vec![];
        }
        let mut visitor = UnpackFirstVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct UnpackFirstVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> UnpackFirstVisitor<'a> {
    /// Match: (recv).unpack(fmt).first / [0] / slice(0) / at(0)
    /// Also handle &. (csend) variants
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);

        // Outer method must be: first, [], slice, at
        let is_first_access = match method.as_ref() {
            "first" => {
                // .first with no args or &.first
                node.arguments().is_none()
            }
            "[]" | "slice" | "at" => {
                // must have exactly one arg: integer 0
                match node.arguments() {
                    Some(args) => {
                        let arg_list: Vec<Node> = args.arguments().iter().collect();
                        if arg_list.len() == 1 {
                            matches!(&arg_list[0], Node::IntegerNode { .. }) && {
                                let int_src = self.ctx.src(
                                    arg_list[0].location().start_offset(),
                                    arg_list[0].location().end_offset(),
                                );
                                int_src == "0"
                            }
                        } else {
                            false
                        }
                    }
                    None => false,
                }
            }
            _ => false,
        };

        if !is_first_access {
            return;
        }

        // The receiver must be .unpack(fmt)
        let unpack_call = match node.receiver() {
            Some(recv) => match recv.as_call_node() {
                Some(c) => c,
                None => return,
            },
            None => return,
        };

        let unpack_method = node_name!(unpack_call);
        if unpack_method != "unpack" {
            return;
        }

        // unpack must have exactly one argument (the format string)
        let fmt_src = match unpack_call.arguments() {
            Some(args) => {
                let arg_list: Vec<Node> = args.arguments().iter().collect();
                if arg_list.len() != 1 {
                    return;
                }
                self.ctx.src(
                    arg_list[0].location().start_offset(),
                    arg_list[0].location().end_offset(),
                ).to_string()
            }
            None => return,
        };

        // offense range: from unpack selector start to outer call end
        let unpack_sel_start = unpack_call.message_loc()
            .map(|l| l.start_offset())
            .unwrap_or(unpack_call.location().start_offset());
        let outer_end = node.location().end_offset();

        let current_src = self.ctx.src(unpack_sel_start, outer_end);
        let msg = format!("Use `unpack1({fmt_src})` instead of `{current_src}`.");

        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &msg, Severity::Convention, unpack_sel_start, outer_end,
        ));
    }
}

impl Visit<'_> for UnpackFirstVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/UnpackFirst", |_cfg| {
    Some(Box::new(UnpackFirst::new()))
});
