//! Metrics/CollectionLiteralLength cop
//!
//! Flags array/hash/Set literals with too many entries.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Metrics/CollectionLiteralLength";
const MSG: &str = "Avoid hard coding large quantities of data in code. \
Prefer reading the data from an external source.";

pub struct CollectionLiteralLength {
    length_threshold: usize,
}

impl CollectionLiteralLength {
    pub fn new() -> Self { Self { length_threshold: 250 } }
    pub fn with_config(length_threshold: usize) -> Self { Self { length_threshold } }
}

impl Default for CollectionLiteralLength {
    fn default() -> Self { Self::new() }
}

impl CollectionLiteralLength {
    fn report_first_line_range(&self, source: &str, start: usize, end: usize) -> (usize, usize) {
        let line_end = source[start..end].find('\n').map(|p| start + p).unwrap_or(end);
        (start, line_end)
    }
}

impl Cop for CollectionLiteralLength {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        let count = node.elements().iter().count();
        if count < self.length_threshold { return vec![]; }
        let loc = node.location();
        let (s, e) = self.report_first_line_range(ctx.source, loc.start_offset(), loc.end_offset());
        vec![ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, s, e)]
    }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        let count = node.elements().iter().count();
        if count < self.length_threshold { return vec![]; }
        let loc = node.location();
        let (s, e) = self.report_first_line_range(ctx.source, loc.start_offset(), loc.end_offset());
        vec![ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, s, e)]
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Set[...] — method `[]`, receiver is ConstantRead `Set`.
        let method = node_name!(node);
        if method != "[]" { return vec![]; }
        let recv = match node.receiver() { Some(r) => r, None => return vec![] };
        if !matches!(&recv, Node::ConstantReadNode { .. }) { return vec![]; }
        let cname = String::from_utf8_lossy(
            recv.as_constant_read_node().unwrap().name().as_slice()
        );
        if cname != "Set" { return vec![]; }
        let count = node.arguments().map_or(0, |a| a.arguments().iter().count());
        if count < self.length_threshold { return vec![]; }
        let loc = node.location();
        let (s, e) = self.report_first_line_range(ctx.source, loc.start_offset(), loc.end_offset());
        vec![ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, s, e)]
    }
}

crate::register_cop!("Metrics/CollectionLiteralLength", |cfg| {
    let cop_config = cfg.get_cop_config("Metrics/CollectionLiteralLength");
    let threshold = cop_config
        .and_then(|c| c.raw.get("LengthThreshold"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(250);
    Some(Box::new(CollectionLiteralLength::with_config(threshold)))
});
