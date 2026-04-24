//! Style/ObjectThen cop — prefer `then` or `yield_self`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    #[default]
    Then,
    YieldSelf,
}

pub struct ObjectThen { style: EnforcedStyle }

impl Default for ObjectThen {
    fn default() -> Self { Self { style: EnforcedStyle::Then } }
}

impl ObjectThen {
    pub fn new(style: EnforcedStyle) -> Self { Self { style } }
}

impl Cop for ObjectThen {
    fn name(&self) -> &'static str { "Style/ObjectThen" }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 6) { return vec![]; }
        let method = node_name!(node);
        if method != "then" && method != "yield_self" { return vec![]; }

        // Valid shape: block (BlockNode), OR exactly-1 arg = block-pass.
        let has_block_node = node.block().and_then(|b| b.as_block_node()).is_some();
        let has_block_via_pass = node.block().and_then(|b| b.as_block_argument_node()).is_some();
        // Count positional args excluding block_argument
        let positional_arg_count: usize = match node.arguments() {
            Some(a) => a.arguments().iter().filter(|n| n.as_block_argument_node().is_none()).count(),
            None => 0,
        };
        // Flag when: has_block_node and 0 positional args; OR has_block_via_pass and 0 positional args.
        let valid_shape = (has_block_node && positional_arg_count == 0)
            || (has_block_via_pass && positional_arg_count == 0);
        if !valid_shape { return vec![]; }

        let preferred = match self.style {
            EnforcedStyle::Then => "then",
            EnforcedStyle::YieldSelf => "yield_self",
        };
        if method == preferred { return vec![]; }

        let msg_loc = match node.message_loc() { Some(l) => l, None => return vec![] };
        let start = msg_loc.start_offset();
        let end = msg_loc.end_offset();
        // If replacing with `then` and receiver is absent, use `self.then`
        let replacement = if self.style == EnforcedStyle::Then && node.receiver().is_none() {
            "self.then".to_string()
        } else {
            preferred.to_string()
        };
        let msg = format!("Prefer `{}` over `{}`.", preferred, method);
        vec![ctx.offense_with_range(self.name(), &msg, Severity::Convention, start, end)
            .with_correction(Correction::replace(start, end, replacement))]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: Option<String> }

crate::register_cop!("Style/ObjectThen", |cfg| {
    let c: Cfg = cfg.typed("Style/ObjectThen");
    let style = match c.enforced_style.as_deref() {
        Some("yield_self") => EnforcedStyle::YieldSelf,
        _ => EnforcedStyle::Then,
    };
    Some(Box::new(ObjectThen::new(style)))
});
