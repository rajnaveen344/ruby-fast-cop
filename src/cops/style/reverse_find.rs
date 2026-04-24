//! Style/ReverseFind cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

const MSG: &str = "Use `rfind` instead.";

#[derive(Default)]
pub struct ReverseFind;
impl ReverseFind { pub fn new() -> Self { Self } }

impl Cop for ReverseFind {
    fn name(&self) -> &'static str { "Style/ReverseFind" }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(4, 0) { return vec![]; }
        let method = node_name!(node);
        if !matches!(method.as_ref(), "find" | "detect") { return vec![]; }
        let recv = match node.receiver() { Some(r) => r, None => return vec![] };
        let recv_call = match recv.as_call_node() { Some(c) => c, None => return vec![] };
        let inner_name = node_name!(recv_call);
        if !matches!(inner_name.as_ref(), "reverse" | "reverse_each") { return vec![]; }
        if recv_call.arguments().is_some() { return vec![]; }

        let inner_sel = match recv_call.message_loc() { Some(l) => l, None => return vec![] };
        let outer_sel = match node.message_loc() { Some(l) => l, None => return vec![] };
        let start = inner_sel.start_offset();
        let end = outer_sel.end_offset();
        vec![ctx.offense_with_range(self.name(), MSG, Severity::Convention, start, end)
            .with_correction(Correction::replace(start, end, "rfind".to_string()))]
    }
}

crate::register_cop!("Style/ReverseFind", |_cfg| Some(Box::new(ReverseFind::new())));
