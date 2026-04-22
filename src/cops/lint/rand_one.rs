//! Lint/RandOne - Checks for `rand(1)` / `rand(-1)` / `rand(1.0)` calls that always return 0.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::node_name;

#[derive(Default)]
pub struct RandOne;

impl RandOne {
    pub fn new() -> Self { Self }
}

/// Returns true if a Node is an integer/float literal with value +1 or -1 (unary forms too).
fn is_one_value(node: &ruby_prism::Node, source: &str) -> bool {
    let src = &source[node.location().start_offset()..node.location().end_offset()];
    matches!(src.trim(), "1" | "-1" | "1.0" | "-1.0")
}

impl Cop for RandOne {
    fn name(&self) -> &'static str { "Lint/RandOne" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method.as_ref() != "rand" {
            return vec![];
        }

        // Allow: receiver must be nil or Kernel (bare / Kernel. / ::Kernel.)
        if let Some(recv) = node.receiver() {
            // Must be Kernel constant (plain or rooted)
            let is_kernel = match &recv {
                ruby_prism::Node::ConstantReadNode { .. } => {
                    let n = recv.as_constant_read_node().unwrap();
                    String::from_utf8_lossy(n.name().as_slice()) == "Kernel"
                }
                ruby_prism::Node::ConstantPathNode { .. } => {
                    let p = recv.as_constant_path_node().unwrap();
                    // ::Kernel — parent is None (rooted), name == "Kernel"
                    p.name().map_or(false, |id| String::from_utf8_lossy(id.as_slice()) == "Kernel")
                }
                _ => false,
            };
            if !is_kernel {
                return vec![];
            }
        }

        // Must have exactly one argument
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }
        let arg = &arg_list[0];

        // Check if arg is integer/float with value ±1
        let is_offense = match arg {
            ruby_prism::Node::IntegerNode { .. } |
            ruby_prism::Node::FloatNode { .. } => is_one_value(arg, ctx.source),
            _ => false,
        };

        if !is_offense {
            return vec![];
        }

        let call_src = &ctx.source[node.location().start_offset()..node.location().end_offset()];
        let msg = format!("`{}` always returns `0`. Perhaps you meant `rand(2)` or `rand`?", call_src);

        vec![ctx.offense_with_range(
            "Lint/RandOne",
            &msg,
            Severity::Warning,
            node.location().start_offset(),
            node.location().end_offset(),
        )]
    }
}

crate::register_cop!("Lint/RandOne", |_cfg| Some(Box::new(RandOne::new())));
