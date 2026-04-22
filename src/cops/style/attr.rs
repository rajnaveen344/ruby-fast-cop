//! Style/Attr cop
//!
//! Discourages `attr` — use `attr_reader` or `attr_accessor` instead.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Node};

#[derive(Default)]
pub struct Attr;

impl Attr {
    pub fn new() -> Self {
        Self
    }

    fn is_offending(node: &CallNode) -> bool {
        let method = node_name!(node);
        if method != "attr" {
            return false;
        }
        // Has receiver → skip (e.g., `x.attr arg`)
        if node.receiver().is_some() {
            return false;
        }
        // No args → skip (used as method reference)
        let args = match node.arguments() {
            Some(a) => a,
            None => return false,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return false;
        }
        // First arg must be a symbol (attr :name, ...) — not some other type like integer
        // This skips cases like `attr(1)` which is a custom attr method call
        if !matches!(arg_list[0], Node::SymbolNode { .. }) {
            return false;
        }
        true
    }

    fn message(node: &CallNode) -> String {
        // `attr :name, true` → attr_accessor
        // everything else → attr_reader
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 2 {
                if let Some(second) = arg_list.get(1) {
                    if matches!(second, Node::TrueNode { .. }) {
                        return "Do not use `attr`. Use `attr_accessor` instead.".to_string();
                    }
                }
            }
        }
        "Do not use `attr`. Use `attr_reader` instead.".to_string()
    }
}

impl Cop for Attr {
    fn name(&self) -> &'static str {
        "Style/Attr"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_offending(node) {
            return vec![];
        }
        let msg = Self::message(node);
        let method_loc = node.message_loc().unwrap_or_else(|| node.location());
        vec![ctx.offense(self.name(), &msg, self.severity(), &method_loc)]
    }
}

crate::register_cop!("Style/Attr", |_cfg| Some(Box::new(Attr::new())));
