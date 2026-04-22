//! Style/ColonMethodCall cop
//!
//! Checks for method calls using `::` instead of `.`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::CallNode;

#[derive(Default)]
pub struct ColonMethodCall;

impl ColonMethodCall {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ColonMethodCall {
    fn name(&self) -> &'static str {
        "Style/ColonMethodCall"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Must have call_operator_loc that is `::`
        let op_loc = match node.call_operator_loc() {
            Some(loc) => loc,
            None => return vec![],
        };

        let op = String::from_utf8_lossy(op_loc.as_slice());
        if op != "::" {
            return vec![];
        }

        // Skip if method name starts with uppercase (constant)
        let method = node_name!(node);
        let first_char = method.chars().next().unwrap_or('_');
        if first_char.is_uppercase() {
            return vec![];
        }

        // Java interop: skip if receiver is ConstantReadNode named "Java"
        // or if receiver is a ConstantPathNode whose root is "Java"
        if let Some(recv) = node.receiver() {
            if let Some(cr) = recv.as_constant_read_node() {
                let recv_name = node_name!(cr);
                if recv_name == "Java" {
                    return vec![];
                }
            }
            // Also check ConstantPathNode root
            if let Some(cp) = recv.as_constant_path_node() {
                if let Some(parent) = cp.parent() {
                    if let Some(cr) = parent.as_constant_read_node() {
                        let parent_name = node_name!(cr);
                        if parent_name == "Java" {
                            return vec![];
                        }
                    }
                }
            }
        }

        vec![ctx.offense(self.name(), "Do not use `::` for method calls.", self.severity(), &op_loc)]
    }
}

crate::register_cop!("Style/ColonMethodCall", |_cfg| Some(Box::new(ColonMethodCall::new())));
