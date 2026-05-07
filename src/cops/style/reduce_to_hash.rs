//! Style/ReduceToHash cop
//!
//! Checks for each_with_object/inject/reduce calls that build a hash,
//! suggesting to_h { ... } instead.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Node, Visit};

#[derive(Default)]
pub struct ReduceToHash;

impl ReduceToHash {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ReduceToHash {
    fn name(&self) -> &'static str {
        "Style/ReduceToHash"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ReduceToHashVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct ReduceToHashVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// Result of extracting the pattern from the block
struct HashBuildPattern {
    key_src: String,
    val_src: String,
    elem_arg: Option<String>, // None = numbered params
}

fn is_empty_hash(node: &Node) -> bool {
    node.as_hash_node()
        .map(|h| h.elements().iter().count() == 0)
        .unwrap_or(false)
}

fn is_lvar(node: &Node, name: &[u8]) -> bool {
    node.as_local_variable_read_node()
        .map(|n| n.name().as_slice() == name)
        .unwrap_or(false)
}

fn extract_index_write_ranges(node: &Node) -> Option<((usize, usize), (usize, usize))> {
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"[]=" {
        return None;
    }
    let args = call.arguments()?;
    let mut it = args.arguments().iter();
    let k = it.next()?;
    let v = it.next()?;
    if it.next().is_some() { return None; }
    Some((
        (k.location().start_offset(), k.location().end_offset()),
        (v.location().start_offset(), v.location().end_offset()),
    ))
}

/// Try to extract hash-building pattern from block
fn try_extract(
    node: &CallNode,
    block: &BlockNode,
    source: &str,
    is_each_with_object: bool,
    is_inject_reduce: bool,
) -> Option<HashBuildPattern> {
    let body = block.body()?;

    let is_numblock = block.parameters()
        .map(|p| matches!(p, Node::NumberedParametersNode { .. }))
        .unwrap_or(false);
    let _ = node; // suppress unused warning

    if is_numblock {
        if is_each_with_object {
            let stmts_node = body.as_statements_node()?;
            let stmts: Vec<_> = stmts_node.body().iter().collect();
            if stmts.len() != 1 { return None; }
            let stmt = &stmts[0];
            let recv = stmt.as_call_node().and_then(|c| c.receiver());
            if !recv.map(|r| is_lvar(&r, b"_2")).unwrap_or(false) { return None; }
            let (k, v) = extract_index_write_ranges(stmt)?;
            Some(HashBuildPattern {
                key_src: source[k.0..k.1].to_string(),
                val_src: source[v.0..v.1].to_string(),
                elem_arg: None,
            })
        } else {
            // inject/reduce numblock: `_1[key] = value; _1`
            let stmts_node = body.as_statements_node()?;
            let stmts: Vec<_> = stmts_node.body().iter().collect();
            if stmts.len() != 2 { return None; }
            if !is_lvar(&stmts[1], b"_1") { return None; }
            let recv = stmts[0].as_call_node().and_then(|c| c.receiver());
            if !recv.map(|r| is_lvar(&r, b"_1")).unwrap_or(false) { return None; }
            let (k, v) = extract_index_write_ranges(&stmts[0])?;
            // Rename _2 → _1 in key/value
            let key_src = source[k.0..k.1].replace("_2", "_1");
            let val_src = source[v.0..v.1].replace("_2", "_1");
            Some(HashBuildPattern { key_src, val_src, elem_arg: None })
        }
    } else {
        // Named params
        let bp = block.parameters().and_then(|p| p.as_block_parameters_node())?;
        let inner_params = bp.parameters()?;
        let params: Vec<_> = inner_params.requireds().iter().collect();
        if params.len() != 2 { return None; }

        let (hash_param_idx, elem_param_idx) = if is_each_with_object { (1, 0) } else { (0, 1) };
        let hash_param = params[hash_param_idx].as_required_parameter_node()?;
        let elem_param = params[elem_param_idx].as_required_parameter_node()?;
        let hash_name: Vec<u8> = hash_param.name().as_slice().to_vec();
        let elem_name = String::from_utf8_lossy(elem_param.name().as_slice()).into_owned();

        if is_each_with_object {
            let stmts_node = body.as_statements_node()?;
            let stmts: Vec<_> = stmts_node.body().iter().collect();
            if stmts.len() != 1 { return None; }
            let stmt = &stmts[0];
            let recv = stmt.as_call_node().and_then(|c| c.receiver());
            if !recv.map(|r| is_lvar(&r, &hash_name)).unwrap_or(false) { return None; }
            let (k, v) = extract_index_write_ranges(stmt)?;
            let key_src = source[k.0..k.1].to_string();
            let val_src = source[v.0..v.1].to_string();
            // Check accumulator not in key/value
            let hash_name_str = std::str::from_utf8(&hash_name).unwrap_or("");
            if key_src.contains(hash_name_str) || val_src.contains(hash_name_str) { return None; }
            Some(HashBuildPattern { key_src, val_src, elem_arg: Some(elem_name) })
        } else {
            let stmts_node = body.as_statements_node()?;
            let stmts: Vec<_> = stmts_node.body().iter().collect();
            if stmts.len() != 2 { return None; }
            if !is_lvar(&stmts[1], &hash_name) { return None; }
            let recv = stmts[0].as_call_node().and_then(|c| c.receiver());
            if !recv.map(|r| is_lvar(&r, &hash_name)).unwrap_or(false) { return None; }
            let (k, v) = extract_index_write_ranges(&stmts[0])?;
            let key_src = source[k.0..k.1].to_string();
            let val_src = source[v.0..v.1].to_string();
            let hash_name_str = std::str::from_utf8(&hash_name).unwrap_or("");
            if key_src.contains(hash_name_str) || val_src.contains(hash_name_str) { return None; }
            Some(HashBuildPattern { key_src, val_src, elem_arg: Some(elem_name) })
        }
    }
}

impl<'a> ReduceToHashVisitor<'a> {
    fn check_call(&mut self, node: &CallNode) {
        if !self.ctx.ruby_version_at_least(2, 6) {
            return;
        }

        let method = node_name!(node);
        let is_each_with_object = method == "each_with_object";
        let is_inject_reduce = method == "inject" || method == "reduce";
        if !is_each_with_object && !is_inject_reduce {
            return;
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        {
            let args_vec: Vec<_> = args.arguments().iter().collect();
            if args_vec.len() != 1 || !is_empty_hash(&args_vec[0]) {
                return;
            }
        }

        let block_node_enum = match node.block() {
            Some(b) => b,
            None => return,
        };
        let block = match block_node_enum.as_block_node() {
            Some(b) => b,
            None => return,
        };

        let pattern = match try_extract(node, &block, self.ctx.source, is_each_with_object, is_inject_reduce) {
            Some(p) => p,
            None => return,
        };

        let source = self.ctx.source;
        let block_src_start = block.location().start_offset();
        let is_do_end = source[block_src_start..].starts_with("do");
        // Use column of the call node start (receiver or message), not the block keyword
        let call_start = node.location().start_offset();
        let call_col = self.ctx.col_of(call_start);
        let indent = " ".repeat(call_col);
        let body_expr = format!("[{}, {}]", pattern.key_src, pattern.val_src);

        let replacement = match &pattern.elem_arg {
            None => {
                if is_do_end {
                    format!("to_h do\n{}  {}\n{}end", indent, body_expr, indent)
                } else {
                    format!("to_h {{ {} }}", body_expr)
                }
            }
            Some(arg) => {
                if is_do_end {
                    format!("to_h do |{}|\n{}  {}\n{}end", arg, indent, body_expr, indent)
                } else {
                    format!("to_h {{ |{}| {} }}", arg, body_expr)
                }
            }
        };

        let msg_loc = match node.message_loc() {
            Some(l) => l,
            None => return,
        };
        let off_start = msg_loc.start_offset();
        let off_end = msg_loc.end_offset();
        let corr_end = block.location().end_offset();

        let msg = if is_each_with_object {
            "Use `to_h { ... }` instead of `each_with_object`."
        } else if method == "inject" {
            "Use `to_h { ... }` instead of `inject`."
        } else {
            "Use `to_h { ... }` instead of `reduce`."
        };

        let offense = self.ctx.offense_with_range(
            "Style/ReduceToHash",
            msg,
            Severity::Convention,
            off_start,
            off_end,
        );
        let correction = Correction::replace(off_start, corr_end, replacement);
        self.offenses.push(offense.with_correction(correction));
    }
}

impl<'a> Visit<'_> for ReduceToHashVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/ReduceToHash", |_cfg| {
    Some(Box::new(ReduceToHash::new()))
});
