//! Style/RedundantStructKeywordInit
//!
//! Ruby >= 3.2: `keyword_init: nil`/`keyword_init: true` in `Struct.new(...)`
//! is redundant and can be removed.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node};

#[derive(Default)]
pub struct RedundantStructKeywordInit;

impl RedundantStructKeywordInit {
    pub fn new() -> Self { Self }
}

fn is_struct_new(node: &CallNode) -> bool {
    if node_name!(node) != "new" { return false; }
    let Some(recv) = node.receiver() else { return false };
    match &recv {
        Node::ConstantReadNode { .. } => {
            let c = recv.as_constant_read_node().unwrap();
            String::from_utf8_lossy(c.name().as_slice()) == "Struct"
        }
        Node::ConstantPathNode { .. } => {
            let cp = recv.as_constant_path_node().unwrap();
            if cp.parent().is_some() { return false; }
            cp.name()
                .map(|n| String::from_utf8_lossy(n.as_slice()) == "Struct")
                .unwrap_or(false)
        }
        _ => false,
    }
}

#[derive(Copy, Clone, PartialEq)]
enum KwInit { RedundantTrue, RedundantNil, False }

fn classify_keyword_init(pair: &Node, src: &str) -> Option<KwInit> {
    let assoc = pair.as_assoc_node()?;
    let key = assoc.key();
    let sym = key.as_symbol_node()?;
    let val = sym.value_loc()?;
    if &src.as_bytes()[val.start_offset()..val.end_offset()] != b"keyword_init" {
        return None;
    }
    let value = assoc.value();
    match &value {
        Node::TrueNode { .. } => Some(KwInit::RedundantTrue),
        Node::NilNode { .. } => Some(KwInit::RedundantNil),
        Node::FalseNode { .. } => Some(KwInit::False),
        _ => None,
    }
}

impl Cop for RedundantStructKeywordInit {
    fn name(&self) -> &'static str { "Style/RedundantStructKeywordInit" }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 2) { return vec![]; }
        if !is_struct_new(node) { return vec![]; }

        let Some(args) = node.arguments() else { return vec![] };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        let Some(last) = arg_list.last() else { return vec![] };

        let pairs: Vec<Node> = if let Some(kh) = last.as_keyword_hash_node() {
            kh.elements().iter().collect()
        } else if let Some(h) = last.as_hash_node() {
            h.elements().iter().collect()
        } else {
            return vec![];
        };

        let mut classified: Vec<(usize, KwInit)> = Vec::new();
        for (idx, p) in pairs.iter().enumerate() {
            if let Some(k) = classify_keyword_init(p, ctx.source) {
                classified.push((idx, k));
            }
        }
        if classified.iter().any(|(_, k)| *k == KwInit::False) {
            return vec![];
        }

        let mut offenses = Vec::new();
        for (idx, kind) in classified.iter().rev() {
            let pair = &pairs[*idx];
            let loc = pair.location();
            let pair_start = loc.start_offset();
            let pair_end = loc.end_offset();
            let value_src = match kind {
                KwInit::RedundantTrue => "true",
                KwInit::RedundantNil => "nil",
                _ => continue,
            };
            let msg = format!("Remove the redundant `keyword_init: {}`.", value_src);

            // Look back for a left sibling: either previous pair in hash, or
            // (if this is the first pair) the previous positional arg.
            let left_sibling_end: Option<usize> = if *idx > 0 {
                Some(pairs[*idx - 1].location().end_offset())
            } else if arg_list.len() >= 2 {
                // Last element of arg_list is the hash; previous is the last positional arg.
                Some(arg_list[arg_list.len() - 2].location().end_offset())
            } else {
                None
            };
            let edit_start = left_sibling_end.unwrap_or(pair_start);
            let correction = Correction::delete(edit_start, pair_end);

            offenses.push(
                ctx.offense_with_range(self.name(), &msg, Severity::Convention, pair_start, pair_end)
                    .with_correction(correction),
            );
        }
        offenses
    }
}

crate::register_cop!("Style/RedundantStructKeywordInit", |_cfg| Some(Box::new(RedundantStructKeywordInit::new())));
