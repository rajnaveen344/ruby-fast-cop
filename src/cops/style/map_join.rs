//! Style/MapJoin - Redundant `map(&:to_s)` before `join`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/map_join.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node};

#[derive(Default)]
pub struct MapJoin;

impl MapJoin {
    pub fn new() -> Self { Self }

    /// Returns (map_method_name, map_call, include_block) if
    /// `join_call.receiver` is a `.map`/`.collect` call with to_s body (block or block-pass).
    /// The third bool indicates whether the map call has a block (so removal must extend
    /// through the block's closing).
    fn extract_map_to_s<'a>(join_call: &CallNode<'a>, source: &str)
        -> Option<(String, CallNode<'a>, bool)>
    {
        let receiver = join_call.receiver()?;
        let call = receiver.as_call_node()?;
        let name = node_name!(call);
        if name != "map" && name != "collect" { return None; }

        // Case A: block_pass `(&:to_s)` stored as CallNode.block() BlockArgumentNode.
        if call.arguments().is_none() {
            if let Some(blk) = call.block() {
                if let Some(ba) = blk.as_block_argument_node() {
                    if let Some(sym_any) = ba.expression() {
                        if let Some(s) = sym_any.as_symbol_node() {
                            if let Some(v) = s.value_loc() {
                                let raw = source.get(v.start_offset()..v.end_offset())?;
                                if raw == "to_s" {
                                    return Some((name.into_owned(), call, false));
                                }
                            }
                        }
                    }
                    return None;
                }
            }
        }

        // Case B: block literal `{ |x| x.to_s }` etc., no args.
        if call.arguments().is_some() { return None; }
        let block_node_any = call.block()?;
        let block = block_node_any.as_block_node()?;
        let pname = param_name(&block)?;
        let body = block.body()?;
        let stmts = body.as_statements_node()?;
        let body_list: Vec<_> = stmts.body().iter().collect();
        if body_list.len() != 1 { return None; }
        let send = body_list[0].as_call_node()?;
        if node_name!(send) != "to_s" { return None; }
        if send.arguments().is_some() { return None; }
        if send.block().is_some() { return None; }
        let inner_recv = send.receiver()?;
        if !is_param_ref(&inner_recv, &pname) { return None; }
        Some((name.into_owned(), call, true))
    }
}

fn param_name(block: &ruby_prism::BlockNode) -> Option<String> {
    let params = block.parameters()?;
    match &params {
        Node::BlockParametersNode { .. } => {
            let bp = params.as_block_parameters_node()?;
            let inner = bp.parameters()?;
            let reqs: Vec<_> = inner.requireds().iter().collect();
            if reqs.len() != 1 { return None; }
            if inner.optionals().iter().next().is_some() { return None; }
            if inner.rest().is_some() { return None; }
            if inner.keywords().iter().next().is_some() { return None; }
            let rp = reqs[0].as_required_parameter_node()?;
            Some(node_name!(rp).into_owned())
        }
        Node::NumberedParametersNode { .. } => {
            let np = params.as_numbered_parameters_node()?;
            if np.maximum() == 1 { Some("_1".into()) } else { None }
        }
        Node::ItParametersNode { .. } => Some("it".into()),
        _ => None,
    }
}

fn is_param_ref(node: &Node, param_name: &str) -> bool {
    match node {
        Node::LocalVariableReadNode { .. } => {
            node_name!(node.as_local_variable_read_node().unwrap()) == param_name
        }
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            node_name!(c) == param_name && c.receiver().is_none() && c.arguments().is_none()
        }
        Node::ItLocalVariableReadNode { .. } => param_name == "it",
        _ => false,
    }
}

impl Cop for MapJoin {
    fn name(&self) -> &'static str { "Style/MapJoin" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "join" { return vec![]; }
        let (map_method, map_send, has_block) = match Self::extract_map_to_s(node, ctx.source) {
            Some(v) => v,
            None => return vec![],
        };

        let sel = map_send.message_loc().expect("call must have selector");
        let off_start = sel.start_offset();
        let off_end = sel.end_offset();

        let message = format!("Remove redundant `{}(&:to_s)` before `join`.", map_method);

        let mut offense = ctx.offense_with_range(
            self.name(),
            &message,
            self.severity(),
            off_start,
            off_end,
        );

        // map_node_end = end of the map call including block (if any) or block_pass args.
        let map_node_end = if has_block {
            // Block is a child of map_send; include it.
            let block_any = map_send.block().expect("block present");
            block_any.location().end_offset()
        } else {
            map_send.location().end_offset()
        };

        if let Some(recv) = map_send.receiver() {
            let recv_end = recv.location().end_offset();
            let dot_loc = map_send.call_operator_loc();
            let start_pos = if let Some(dot) = dot_loc {
                if ctx.line_of(recv_end) < ctx.line_of(dot.start_offset()) {
                    recv_end
                } else {
                    dot.start_offset()
                }
            } else {
                recv_end
            };
            offense = offense.with_correction(Correction::replace(start_pos, map_node_end, String::new()));
        } else {
            let dot = node.call_operator_loc();
            let del_end = dot.map_or(map_node_end, |d| d.end_offset());
            let start_pos = map_send.location().start_offset();
            offense = offense.with_correction(Correction::replace(start_pos, del_end, String::new()));
        }

        vec![offense]
    }
}

crate::register_cop!("Style/MapJoin", |_cfg| Some(Box::new(MapJoin::new())));
