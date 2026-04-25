//! Style/ReduceToHash cop
//!
//! Detects `each_with_object({}) { |e, h| h[k] = v }` and
//! `inject({}) / reduce({}) { |h, e| h[k] = v; h }` — replaceable with `to_h`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Style/ReduceToHash";

#[derive(Default)]
pub struct ReduceToHash;

impl ReduceToHash {
    pub fn new() -> Self { Self }
}

impl Cop for ReduceToHash {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 6) { return vec![]; }
        let method = node_name!(node);
        let method_str = method.to_string();
        let is_each_with_object = method_str == "each_with_object";
        let is_reduce = method_str == "inject" || method_str == "reduce";
        if !is_each_with_object && !is_reduce { return vec![]; }

        let block_node = match node.block() { Some(b) => b, None => return vec![] };
        let block = match &block_node {
            Node::BlockNode { .. } => block_node.as_block_node().unwrap(),
            _ => return vec![],
        };

        let args: Vec<Node> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => Vec::new(),
        };
        if args.len() != 1 { return vec![]; }
        let arg0 = &args[0];
        let Node::HashNode { .. } = arg0 else { return vec![] };
        if arg0.as_hash_node().unwrap().elements().iter().count() != 0 { return vec![]; }

        let Some(params_node) = block.parameters() else { return vec![] };
        let (_elem_name, hash_name): (String, String) = match &params_node {
            Node::BlockParametersNode { .. } => {
                let bp = params_node.as_block_parameters_node().unwrap();
                let Some(inner) = bp.parameters() else { return vec![] };
                let req: Vec<Node> = inner.requireds().iter().collect();
                if req.len() != 2 { return vec![]; }
                let p0 = match req[0].as_required_parameter_node() {
                    Some(p) => String::from_utf8_lossy(p.name().as_slice()).into_owned(),
                    None => return vec![],
                };
                let p1 = match req[1].as_required_parameter_node() {
                    Some(p) => String::from_utf8_lossy(p.name().as_slice()).into_owned(),
                    None => return vec![],
                };
                if is_each_with_object { (p0, p1) } else { (p1, p0) }
            }
            Node::NumberedParametersNode { .. } => {
                let np = params_node.as_numbered_parameters_node().unwrap();
                if np.maximum() != 2 { return vec![]; }
                if is_each_with_object {
                    ("_1".to_string(), "_2".to_string())
                } else {
                    ("_2".to_string(), "_1".to_string())
                }
            }
            _ => return vec![],
        };

        let body = match block.body() { Some(b) => b, None => return vec![] };
        let stmts = match &body {
            Node::StatementsNode { .. } => body.as_statements_node().unwrap(),
            _ => return vec![],
        };
        let body_stmts: Vec<Node> = stmts.body().iter().collect();
        let assign_stmt = if is_each_with_object {
            if body_stmts.len() != 1 { return vec![]; }
            &body_stmts[0]
        } else {
            if body_stmts.len() != 2 { return vec![]; }
            let last = &body_stmts[1];
            let Node::LocalVariableReadNode { .. } = last else { return vec![] };
            let n = String::from_utf8_lossy(
                last.as_local_variable_read_node().unwrap().name().as_slice()
            );
            if n != hash_name { return vec![]; }
            &body_stmts[0]
        };
        let assign_call = match assign_stmt.as_call_node() { Some(c) => c, None => return vec![] };
        let amethod = node_name!(assign_call);
        if &*amethod != "[]=" { return vec![]; }
        let arecv = match assign_call.receiver() { Some(r) => r, None => return vec![] };
        let Node::LocalVariableReadNode { .. } = arecv else { return vec![] };
        let arecv_name = String::from_utf8_lossy(
            arecv.as_local_variable_read_node().unwrap().name().as_slice()
        );
        if arecv_name != hash_name { return vec![]; }

        let sel_loc = match node.message_loc() { Some(l) => l, None => return vec![] };
        let msg = format!("Use `to_h {{ ... }}` instead of `{}`.", method_str);
        vec![ctx.offense_with_range(
            COP_NAME, &msg, Severity::Convention,
            sel_loc.start_offset(), sel_loc.end_offset(),
        )]
    }
}

crate::register_cop!("Style/ReduceToHash", |_cfg| {
    Some(Box::new(ReduceToHash::new()))
});
