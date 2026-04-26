//! Style/HashLookupMethod cop
//!
//! Enforces either `Hash#[]` or `Hash#fetch` for hash lookup.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

const BRACKET_MSG: &str = "Use `Hash#[]` instead of `Hash#fetch`.";
const FETCH_MSG: &str = "Use `Hash#fetch` instead of `Hash#[]`.";

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    #[default]
    Brackets,
    Fetch,
}

pub struct HashLookupMethod {
    style: EnforcedStyle,
}

impl HashLookupMethod {
    pub fn new() -> Self {
        Self { style: EnforcedStyle::Brackets }
    }

    pub fn with_style(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Default for HashLookupMethod {
    fn default() -> Self {
        Self::new()
    }
}

fn one_arg_no_block(node: &ruby_prism::CallNode) -> bool {
    let args = match node.arguments() {
        Some(a) => a,
        None => return false,
    };
    let count = args.arguments().iter().count();
    count == 1 && node.block().is_none()
}

impl Cop for HashLookupMethod {
    fn name(&self) -> &'static str {
        "Style/HashLookupMethod"
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);

        match self.style {
            EnforcedStyle::Brackets => {
                if method != "fetch" {
                    return vec![];
                }
                if node.receiver().is_none() {
                    return vec![];
                }
                if !one_arg_no_block(node) {
                    return vec![];
                }
                // Offense range = selector (the `fetch` method name).
                let msg_loc = match node.message_loc() {
                    Some(m) => m,
                    None => return vec![],
                };
                vec![ctx.offense_with_range(
                    self.name(),
                    BRACKET_MSG,
                    Severity::Convention,
                    msg_loc.start_offset(),
                    msg_loc.end_offset(),
                )]
            }
            EnforcedStyle::Fetch => {
                if method != "[]" {
                    return vec![];
                }
                if !one_arg_no_block(node) {
                    return vec![];
                }
                let loc = node.location();
                vec![ctx.offense_with_range(
                    self.name(),
                    FETCH_MSG,
                    Severity::Convention,
                    loc.start_offset(),
                    loc.end_offset(),
                )]
            }
        }
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/HashLookupMethod", |cfg| {
    let c: Cfg = cfg.typed("Style/HashLookupMethod");
    let style = match c.enforced_style.as_deref() {
        Some("fetch") => EnforcedStyle::Fetch,
        _ => EnforcedStyle::Brackets,
    };
    Some(Box::new(HashLookupMethod::with_style(style)))
});
