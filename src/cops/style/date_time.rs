//! Style/DateTime cop
//!
//! Prefer `Time` over `DateTime`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const CLASS_MSG: &str = "Prefer `Time` over `DateTime`.";
const COERCION_MSG: &str = "Do not use `#to_datetime`.";

#[derive(Default)]
pub struct DateTime {
    allow_coercion: bool,
}

impl DateTime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(allow_coercion: bool) -> Self {
        Self { allow_coercion }
    }

    /// Receiver is bare `DateTime` const (optionally `::DateTime`).
    fn receiver_is_datetime(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let c = node.as_constant_read_node().unwrap();
                String::from_utf8_lossy(c.name().as_slice()) == "DateTime"
            }
            Node::ConstantPathNode { .. } => {
                let c = node.as_constant_path_node().unwrap();
                // Only flag `::DateTime` (parent is None or cbase). Skip `Foo::Bar::DateTime`.
                if c.parent().is_some() {
                    return false;
                }
                let name_id = c.name();
                if let Some(id) = name_id {
                    String::from_utf8_lossy(id.as_slice()) == "DateTime"
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Last argument matches `Date::SOMETHING` (historic-date sentinel).
    fn last_arg_is_date_const(call: &ruby_prism::CallNode) -> bool {
        let args = match call.arguments() {
            Some(a) => a,
            None => return false,
        };
        let last = args.arguments().iter().last();
        let last = match last {
            Some(n) => n,
            None => return false,
        };
        // Want: ConstantPathNode whose parent is ConstantReadNode "Date" (with optional cbase).
        let cp = match last.as_constant_path_node() {
            Some(c) => c,
            None => return false,
        };
        let parent = match cp.parent() {
            Some(p) => p,
            None => return false,
        };
        // Parent can be `Date` (ConstantReadNode) or `::Date` (ConstantPathNode w/ no parent).
        match &parent {
            Node::ConstantReadNode { .. } => {
                let cr = parent.as_constant_read_node().unwrap();
                String::from_utf8_lossy(cr.name().as_slice()) == "Date"
            }
            Node::ConstantPathNode { .. } => {
                let p2 = parent.as_constant_path_node().unwrap();
                if p2.parent().is_some() {
                    return false;
                }
                p2.name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()) == "Date")
                    .unwrap_or(false)
            }
            _ => false,
        }
    }
}

impl Cop for DateTime {
    fn name(&self) -> &'static str {
        "Style/DateTime"
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        // to_datetime coercion
        if method == "to_datetime" && !self.allow_coercion {
            // No args expected for `obj.to_datetime`
            if node.arguments().is_some() {
                return vec![];
            }
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            return vec![ctx.offense_with_range(self.name(), COERCION_MSG, self.severity(), start, end)];
        }

        // DateTime.* call
        if !Self::receiver_is_datetime(&receiver) {
            return vec![];
        }
        // Skip historic-date pattern: `DateTime.x(_, Date::ENGLAND)`
        if Self::last_arg_is_date_const(node) {
            return vec![];
        }
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        vec![ctx.offense_with_range(self.name(), CLASS_MSG, self.severity(), start, end)]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_coercion: bool,
}

crate::register_cop!("Style/DateTime", |cfg| {
    let c: Cfg = cfg.typed("Style/DateTime");
    Some(Box::new(DateTime::with_config(c.allow_coercion)))
});
