use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

#[derive(Default)]
pub struct MapToHash;

impl MapToHash {
    pub fn new() -> Self { Self }

    fn dot_source<'a>(call: &ruby_prism::CallNode<'a>, source: &'a str) -> &'a str {
        call.call_operator_loc()
            .and_then(|loc| source.get(loc.start_offset()..loc.end_offset()))
            .unwrap_or(".")
    }

    fn is_block_literal(node: &ruby_prism::Node) -> bool {
        matches!(node, ruby_prism::Node::BlockNode { .. })
    }

    fn is_block_argument(node: &ruby_prism::Node) -> bool {
        matches!(node, ruby_prism::Node::BlockArgumentNode { .. })
    }
}

impl Cop for MapToHash {
    fn name(&self) -> &'static str { "Style/MapToHash" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let outer_name = String::from_utf8_lossy(node.name().as_slice());
        if outer_name != "to_h" { return vec![]; }

        if let Some(b) = node.block() {
            if Self::is_block_literal(&b) { return vec![]; }
        }

        if node.arguments().map_or(false, |a| a.arguments().iter().count() > 0) {
            return vec![];
        }

        let receiver = match node.receiver() { Some(r) => r, None => return vec![] };
        let map_call_node = match &receiver {
            ruby_prism::Node::CallNode { .. } => receiver.as_call_node().unwrap(),
            _ => return vec![],
        };

        let map_name = String::from_utf8_lossy(map_call_node.name().as_slice()).into_owned();
        if map_name != "map" && map_name != "collect" { return vec![]; }

        let map_block = match map_call_node.block() {
            Some(b) => b,
            None => return vec![],
        };

        let has_literal_block = Self::is_block_literal(&map_block);
        let has_symbol_proc = if Self::is_block_argument(&map_block) {
            let ba = map_block.as_block_argument_node().unwrap();
            ba.expression().map_or(false, |e| matches!(&e, ruby_prism::Node::SymbolNode { .. }))
        } else { false };

        if !has_literal_block && !has_symbol_proc { return vec![]; }

        let map_selector = map_call_node.message_loc().expect("map has message");
        let map_selector_start = map_selector.start_offset();
        let map_selector_end = map_selector.end_offset();
        let map_selector_src = &ctx.source[map_selector_start..map_selector_end];

        let outer_dot = Self::dot_source(node, ctx.source);

        let message = format!(
            "Pass a block to `to_h` instead of calling `{}{}to_h`.",
            map_selector_src, outer_dot
        );

        let mut offense = ctx.offense_with_range(
            self.name(),
            &message,
            self.severity(),
            map_selector_start,
            map_selector_end,
        );

        let to_h_selector = node.message_loc().expect("to_h has message");
        let outer_dot_loc = node.call_operator_loc();
        let outer_dot_start = outer_dot_loc.as_ref().map(|l| l.start_offset()).unwrap_or(to_h_selector.start_offset());
        let outer_dot_end_to_selector_end = to_h_selector.end_offset();

        let mut edits: Vec<Edit> = Vec::new();

        // Replace map selector with to_h
        edits.push(Edit { start_offset: map_selector_start, end_offset: map_selector_end, replacement: "to_h".to_string() });

        // Replace map's dot with outer dot if different (so safe-nav propagates)
        if let Some(map_dot) = map_call_node.call_operator_loc() {
            let md_src = &ctx.source[map_dot.start_offset()..map_dot.end_offset()];
            if md_src != outer_dot {
                edits.push(Edit { start_offset: map_dot.start_offset(), end_offset: map_dot.end_offset(), replacement: outer_dot.to_string() });
            }
        }

        // Remove `.to_h` (or `&.to_h`) with surrounding whitespace on left
        let mut removal_start = outer_dot_start;
        let bytes = ctx.source.as_bytes();
        while removal_start > 0 {
            let b = bytes[removal_start - 1];
            if b == b' ' || b == b'\t' || b == b'\n' { removal_start -= 1; } else { break; }
        }
        edits.push(Edit { start_offset: removal_start, end_offset: outer_dot_end_to_selector_end, replacement: String::new() });

        // Destructuring arg: |(k, v)| -> |k, v|
        if has_literal_block {
            let block_node = map_block.as_block_node().unwrap();
            if let Some(params) = block_node.parameters() {
                if let ruby_prism::Node::BlockParametersNode { .. } = &params {
                    let bp = params.as_block_parameters_node().unwrap();
                    if let Some(inner) = bp.parameters() {
                        let requireds: Vec<_> = inner.requireds().iter().collect();
                        if requireds.len() == 1 && inner.optionals().iter().next().is_none()
                            && inner.rest().is_none() && inner.keywords().iter().next().is_none()
                        {
                            if let ruby_prism::Node::MultiTargetNode { .. } = &requireds[0] {
                                let mt_loc = requireds[0].location();
                                let mt_src = &ctx.source[mt_loc.start_offset()..mt_loc.end_offset()];
                                if mt_src.starts_with('(') && mt_src.ends_with(')') {
                                    let inner_src = mt_src[1..mt_src.len()-1].to_string();
                                    edits.push(Edit { start_offset: mt_loc.start_offset(), end_offset: mt_loc.end_offset(), replacement: inner_src });
                                }
                            }
                        }
                    }
                }
            }
        }

        offense = offense.with_correction(Correction { edits });
        vec![offense]
    }
}

crate::register_cop!("Style/MapToHash", |_cfg| Some(Box::new(MapToHash::new())));
