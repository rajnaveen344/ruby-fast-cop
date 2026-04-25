use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

#[derive(Default)]
pub struct MapToSet;

impl MapToSet {
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

impl Cop for MapToSet {
    fn name(&self) -> &'static str { "Style/MapToSet" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // outer is `.to_set`
        let outer_name = String::from_utf8_lossy(node.name().as_slice());
        if outer_name != "to_set" { return vec![]; }

        // outer must not have its own block (block_literal? false)
        if let Some(b) = node.block() {
            if Self::is_block_literal(&b) { return vec![]; }
        }

        // outer arguments must be empty
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

        // map call must have a block (block literal OR block-argument symbol-proc)
        let map_block = match map_call_node.block() {
            Some(b) => b,
            None => return vec![],
        };

        let has_literal_block = Self::is_block_literal(&map_block);
        let has_symbol_proc = if let Some(sym) = if Self::is_block_argument(&map_block) {
            let ba = map_block.as_block_argument_node().unwrap();
            ba.expression()
        } else { None } {
            matches!(&sym, ruby_prism::Node::SymbolNode { .. })
        } else { false };

        if !has_literal_block && !has_symbol_proc { return vec![]; }

        // Offense at map's selector
        let map_selector = map_call_node.message_loc().expect("map has message");
        let map_selector_start = map_selector.start_offset();
        let map_selector_end = map_selector.end_offset();
        let map_selector_src = &ctx.source[map_selector_start..map_selector_end];

        // dot of outer to_set call
        let outer_dot_loc = node.call_operator_loc();
        let outer_dot = Self::dot_source(node, ctx.source);

        // MapToSet message always uses `.` (RuboCop hardcodes `%<method>s.to_set`)
        let _ = outer_dot;
        let message = format!(
            "Pass a block to `to_set` instead of calling `{}.to_set`.",
            map_selector_src
        );

        let mut offense = ctx.offense_with_range(
            self.name(),
            &message,
            self.severity(),
            map_selector_start,
            map_selector_end,
        );

        // Build correction:
        // 1. Replace map selector with "to_set"
        // 2. Remove ".to_set" / "&.to_set" suffix from outer (dot through selector end)
        // 3. If map has its own dot operator, replace with outer dot
        let to_set_selector = node.message_loc().expect("to_set has message");
        let outer_dot_start = outer_dot_loc.as_ref().map(|l| l.start_offset()).unwrap_or(to_set_selector.start_offset());
        let outer_dot_end_to_selector_end = to_set_selector.end_offset();

        let mut edits: Vec<Edit> = Vec::new();

        // Replace map selector with to_set
        edits.push(Edit { start_offset: map_selector_start, end_offset: map_selector_end, replacement: "to_set".to_string() });

        // Remove `.to_set` (or `&.to_set`) with surrounding whitespace on left
        let mut removal_start = outer_dot_start;
        let bytes = ctx.source.as_bytes();
        while removal_start > 0 {
            let b = bytes[removal_start - 1];
            if b == b' ' || b == b'\t' || b == b'\n' { removal_start -= 1; } else { break; }
        }
        edits.push(Edit { start_offset: removal_start, end_offset: outer_dot_end_to_selector_end, replacement: String::new() });

        offense = offense.with_correction(Correction { edits });
        vec![offense]
    }
}

crate::register_cop!("Style/MapToSet", |_cfg| Some(Box::new(MapToSet::new())));
