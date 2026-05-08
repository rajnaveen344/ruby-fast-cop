//! Style/HashLookupMethod cop
//!
//! Enforces either `Hash#[]` or `Hash#fetch` for hash lookup.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

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
                let recv = match node.receiver() {
                    Some(r) => r,
                    None => return vec![],
                };
                if !one_arg_no_block(node) {
                    return vec![];
                }
                // Offense range = selector (the `fetch` method name).
                let msg_loc = match node.message_loc() {
                    Some(m) => m,
                    None => return vec![],
                };
                // Get arg source
                let arg_src = {
                    let args = node.arguments().unwrap();
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    let arg_loc = arg_list[0].location();
                    ctx.src(arg_loc.start_offset(), arg_loc.end_offset()).to_string()
                };
                let recv_src = ctx.src(recv.location().start_offset(), recv.location().end_offset()).to_string();
                // If safe nav (&.), wrap whole thing in parens: (recv[key])
                let is_safe_nav = node.call_operator_loc()
                    .map(|l| &ctx.source[l.start_offset()..l.end_offset()] == "&.")
                    .unwrap_or(false);
                let replacement = if is_safe_nav {
                    format!("({}[{}])", recv_src, arg_src)
                } else {
                    format!("{}[{}]", recv_src, arg_src)
                };
                let node_start = node.location().start_offset();
                let node_end = node.location().end_offset();
                let correction = Correction::replace(node_start, node_end, replacement);
                vec![ctx.offense_with_range(
                    self.name(),
                    BRACKET_MSG,
                    Severity::Convention,
                    msg_loc.start_offset(),
                    msg_loc.end_offset(),
                ).with_correction(correction)]
            }
            EnforcedStyle::Fetch => {
                if method != "[]" {
                    return vec![];
                }
                if !one_arg_no_block(node) {
                    return vec![];
                }
                // Get receiver and arg source
                let recv_src = match node.receiver() {
                    Some(r) => ctx.src(r.location().start_offset(), r.location().end_offset()).to_string(),
                    None => return vec![],
                };
                let arg_src = {
                    let args = node.arguments().unwrap();
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    let arg_loc = arg_list[0].location();
                    ctx.src(arg_loc.start_offset(), arg_loc.end_offset()).to_string()
                };
                // Preserve call operator (&. or .) if present
                let op = node.call_operator_loc()
                    .map(|l| ctx.src(l.start_offset(), l.end_offset()).to_string())
                    .unwrap_or_else(|| ".".to_string());
                let replacement = format!("{}{}fetch({})", recv_src, op, arg_src);
                let loc = node.location();
                let correction = Correction::replace(loc.start_offset(), loc.end_offset(), replacement);
                vec![ctx.offense_with_range(
                    self.name(),
                    FETCH_MSG,
                    Severity::Convention,
                    loc.start_offset(),
                    loc.end_offset(),
                ).with_correction(correction)]
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
