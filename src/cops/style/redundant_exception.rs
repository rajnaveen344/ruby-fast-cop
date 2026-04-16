//! Style/RedundantException - redundant `RuntimeError` in `raise`/`fail`.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_exception.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG_1: &str = "Redundant `RuntimeError` argument can be removed.";
const MSG_2: &str = "Redundant `RuntimeError.new` call can be replaced with just the message.";

#[derive(Default)]
pub struct RedundantException;

impl RedundantException {
    pub fn new() -> Self {
        Self
    }
}

fn is_runtime_error_const(n: &Node) -> bool {
    match n {
        Node::ConstantReadNode { .. } => {
            let c = n.as_constant_read_node().unwrap();
            node_name!(c) == "RuntimeError"
        }
        Node::ConstantPathNode { .. } => {
            let cp = n.as_constant_path_node().unwrap();
            // Ensure parent is cbase (::RuntimeError) or nil (no parent)
            if cp.parent().is_some() {
                return false;
            }
            cp.name()
                .map(|nm| nm.as_slice() == b"RuntimeError")
                .unwrap_or(false)
        }
        _ => false,
    }
}

fn is_string_like(n: &Node) -> bool {
    matches!(
        n,
        Node::StringNode { .. } | Node::InterpolatedStringNode { .. } | Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. }
    )
}

impl Cop for RedundantException {
    fn name(&self) -> &'static str {
        "Style/RedundantException"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "raise" && method != "fail" {
            return vec![];
        }
        if node.receiver().is_some() {
            return vec![];
        }
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();

        // Exploded: raise RuntimeError, msg (exactly 2 args)
        if arg_list.len() == 2 {
            let first = &arg_list[0];
            let second = &arg_list[1];
            if is_runtime_error_const(first) {
                // Build replacement
                let second_src = &ctx.source
                    [second.location().start_offset()..second.location().end_offset()];
                let arg_str = if is_string_like(second) {
                    second_src.to_string()
                } else {
                    format!("{}.to_s", second_src)
                };
                let has_parens = node.opening_loc().is_some();
                let full = if has_parens {
                    format!("{}({})", method, arg_str)
                } else {
                    format!("{} {}", method, arg_str)
                };
                let offense = ctx
                    .offense(self.name(), MSG_1, self.severity(), &node.location())
                    .with_correction(Correction::replace(
                        node.location().start_offset(),
                        node.location().end_offset(),
                        full,
                    ));
                return vec![offense];
            }
        }

        // Compact: raise RuntimeError.new(msg) (exactly 1 arg, which is call `.new` on RuntimeError w/ 1 arg)
        if arg_list.len() == 1 {
            let first = &arg_list[0];
            if let Node::CallNode { .. } = first {
                let inner = first.as_call_node().unwrap();
                if node_name!(inner) == "new" {
                    if let Some(recv) = inner.receiver() {
                        if is_runtime_error_const(&recv) {
                            // `.new` must have exactly 1 argument
                            if let Some(new_args) = inner.arguments() {
                                let new_arg_list: Vec<_> = new_args.arguments().iter().collect();
                                if new_arg_list.len() == 1 {
                                    let msg_arg = &new_arg_list[0];
                                    let msg_src = &ctx.source[msg_arg.location().start_offset()
                                        ..msg_arg.location().end_offset()];
                                    let replacement = if is_string_like(msg_arg) {
                                        msg_src.to_string()
                                    } else {
                                        format!("{}.to_s", msg_src)
                                    };
                                    let offense = ctx
                                        .offense(
                                            self.name(),
                                            MSG_2,
                                            self.severity(),
                                            &node.location(),
                                        )
                                        .with_correction(Correction::replace(
                                            first.location().start_offset(),
                                            first.location().end_offset(),
                                            replacement,
                                        ));
                                    return vec![offense];
                                }
                            }
                        }
                    }
                }
            }
        }

        vec![]
    }
}
