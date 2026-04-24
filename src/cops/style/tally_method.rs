//! Style/TallyMethod cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct TallyMethod;
impl TallyMethod { pub fn new() -> Self { Self } }

fn src<'a>(n: &Node<'a>, source: &'a str) -> &'a str {
    let l = n.location();
    &source[l.start_offset()..l.end_offset()]
}

/// If node is a plain `Hash.new(0)` or `::Hash.new(0)` call, return true.
fn is_hash_new_zero<'a>(n: &Node<'a>, source: &str) -> bool {
    let c = match n.as_call_node() { Some(c) => c, None => return false };
    if &*node_name!(c) != "new" { return false; }
    let recv = match c.receiver() { Some(r) => r, None => return false };
    // receiver must be `Hash` or `::Hash` constant
    let rsrc = src(&recv, source);
    if rsrc != "Hash" && rsrc != "::Hash" { return false; }
    let args = match c.arguments() { Some(a) => a, None => return false };
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 { return false; }
    // arg must be integer literal 0
    let a = &list[0];
    if a.as_integer_node().is_none() { return false; }
    let al = a.location();
    &source[al.start_offset()..al.end_offset()] == "0"
}

/// Get block body statements from a call's attached block.
fn block_body<'a>(c: &ruby_prism::CallNode<'a>) -> Option<(Vec<Node<'a>>, Option<ruby_prism::BlockParametersNode<'a>>)> {
    let blk = c.block()?;
    let b = blk.as_block_node()?;
    let body = b.body()?;
    let stmts = body.as_statements_node()?;
    let items: Vec<_> = stmts.body().iter().collect();
    let params = b.parameters().and_then(|p| p.as_block_parameters_node());
    Some((items, params))
}

/// Get the 2 regular parameters of a block, as source names.
fn block_two_params<'a>(p: &ruby_prism::BlockParametersNode<'a>, source: &'a str) -> Option<(String, String)> {
    let inner = p.parameters()?;
    let reqs: Vec<_> = inner.requireds().iter().collect();
    if reqs.len() != 2 { return None; }
    let a = src(&reqs[0], source).to_string();
    let b = src(&reqs[1], source).to_string();
    Some((a, b))
}

/// Check if block body is `counts[item] += 1` (or numbered `_2[_1] += 1`).
/// item_name/counts_name may be None → require numbered params.
fn body_is_counts_plusplus<'a>(items: &[Node<'a>], item_name: Option<&str>, counts_name: Option<&str>, source: &str) -> bool {
    if items.len() != 1 { return false; }
    let stmt = &items[0];
    // IndexOperatorWriteNode: `counts[item] += 1`
    // Also can be CallOperatorWriteNode in older Prism. Check via source match for simplicity.
    let s = src(stmt, source).trim();
    match (item_name, counts_name) {
        (Some(item), Some(counts)) => {
            let expected = format!("{}[{}] += 1", counts, item);
            s == expected
        }
        (None, None) => s == "_2[_1] += 1",
        _ => false,
    }
}

impl Cop for TallyMethod {
    fn name(&self) -> &'static str { "Style/TallyMethod" }
    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 7) { return vec![]; }
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> { ctx: &'a CheckContext<'a>, offenses: Vec<Offense> }

impl<'a> V<'a> {
    fn check_each_with_object(&mut self, c: &ruby_prism::CallNode<'a>) -> bool {
        let source = self.ctx.source;
        if &*node_name!(c) != "each_with_object" { return false; }
        let args = match c.arguments() { Some(a) => a, None => return false };
        let list: Vec<_> = args.arguments().iter().collect();
        if list.len() != 1 { return false; }
        if !is_hash_new_zero(&list[0], source) { return false; }
        // block must have 2 params OR numbered
        let blk = match c.block() { Some(b) => b, None => return false };
        let b = match blk.as_block_node() { Some(b) => b, None => return false };
        let body = match b.body() { Some(b) => b, None => return false };
        let stmts = match body.as_statements_node() { Some(s) => s, None => return false };
        let items: Vec<_> = stmts.body().iter().collect();
        let params = b.parameters();
        let ok = if let Some(p) = params.as_ref() {
            if let Some(bp) = p.as_block_parameters_node() {
                // 2 regular params
                if let Some((item, counts)) = block_two_params(&bp, source) {
                    body_is_counts_plusplus(&items, Some(&item), Some(&counts), source)
                } else { false }
            } else if p.as_numbered_parameters_node().is_some() {
                body_is_counts_plusplus(&items, None, None, source)
            } else { false }
        } else { false };
        if !ok { return false; }
        // Emit offense on the method name location
        let msg_loc = c.message_loc().expect("each_with_object has message");
        let start = msg_loc.start_offset();
        let end = msg_loc.end_offset();
        // Correction: replace from message start through end of block with `tally`
        let block_end = blk.location().end_offset();
        let msg = "Use `tally` instead of `each_with_object`.";
        let correction = Correction::replace(start, block_end, "tally".to_string());
        self.offenses.push(
            self.ctx.offense_with_range("Style/TallyMethod", msg, Severity::Convention, start, end)
                .with_correction(correction),
        );
        true
    }

    /// Check: `array.group_by(...).transform_values(...)` → tally.
    fn check_group_by_transform(&mut self, c: &ruby_prism::CallNode<'a>) -> bool {
        let source = self.ctx.source;
        if &*node_name!(c) != "transform_values" { return false; }
        // Inner call receiver = group_by
        let recv = match c.receiver() { Some(r) => r, None => return false };
        let gb = match recv.as_call_node() { Some(r) => r, None => return false };
        if &*node_name!(&gb) != "group_by" { return false; }
        // group_by must be identity:
        //   (&:itself) OR { |x| x } OR { _1 } OR { it }
        if !is_identity_group_by(&gb, source) { return false; }
        // transform_values must be &:count/&:size/&:length OR { |v| v.count/size/length } / numblock / itblock
        if !is_count_transform_values(c, source) { return false; }
        // Emit offense on group_by's method name
        let msg_loc = gb.message_loc().expect("group_by has message");
        let start = msg_loc.start_offset();
        let end = msg_loc.end_offset();
        // Correction: replace from group_by message start through end of entire transform_values call with `tally`
        let end_all = c.location().end_offset();
        let msg = "Use `tally` instead of `group_by` and `transform_values`.";
        let correction = Correction::replace(start, end_all, "tally".to_string());
        self.offenses.push(
            self.ctx.offense_with_range("Style/TallyMethod", msg, Severity::Convention, start, end)
                .with_correction(correction),
        );
        true
    }
}

fn is_identity_group_by<'a>(gb: &ruby_prism::CallNode<'a>, source: &str) -> bool {
    // Two shapes: &:itself block arg, or block returning identity.
    // Case 1: block argument (&:itself) — goes into CallNode.block(), not arguments().
    if gb.arguments().is_none() {
        if let Some(blk) = gb.block() {
            if let Some(ba) = blk.as_block_argument_node() {
                if let Some(exp) = ba.expression() {
                    if let Some(sym) = exp.as_symbol_node() {
                        if let Some(vloc) = sym.value_loc() {
                            if &source[vloc.start_offset()..vloc.end_offset()] == "itself" {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    // Case 2: actual block `{ |x| x }` or `{ _1 }` or `{ it }`.
    if let Some(blk) = gb.block() {
        if let Some(b) = blk.as_block_node() {
            if gb.arguments().is_some() { return false; }
            let body = match b.body() { Some(b) => b, None => return false };
            let stmts = match body.as_statements_node() { Some(s) => s, None => return false };
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 { return false; }
            let stmt = &items[0];
            let s_src = src(stmt, source).trim();
            let params = b.parameters();
            if let Some(p) = params.as_ref() {
                if let Some(bp) = p.as_block_parameters_node() {
                    if let Some(inner) = bp.parameters() {
                        let reqs: Vec<_> = inner.requireds().iter().collect();
                        if reqs.len() == 1 {
                            let name = src(&reqs[0], source);
                            if s_src == name { return true; }
                        }
                    }
                } else if p.as_numbered_parameters_node().is_some() {
                    if s_src == "_1" { return true; }
                } else if p.as_it_parameters_node().is_some() {
                    if s_src == "it" { return true; }
                }
            } else {
                // No explicit params → check body is `it`
                if s_src == "it" { return true; }
            }
        }
    }
    false
}

fn is_count_transform_values<'a>(tv: &ruby_prism::CallNode<'a>, source: &str) -> bool {
    // Shape 1: block arg (&:count/&:size/&:length) — goes into block(), not arguments().
    if tv.arguments().is_none() {
        if let Some(blk) = tv.block() {
            if let Some(ba) = blk.as_block_argument_node() {
                if let Some(exp) = ba.expression() {
                    if let Some(sym) = exp.as_symbol_node() {
                        if let Some(vloc) = sym.value_loc() {
                            let v = &source[vloc.start_offset()..vloc.end_offset()];
                            if matches!(v, "count" | "size" | "length") { return true; }
                        }
                    }
                }
            }
        }
    }
    // Shape 2: block { |v| v.count/size/length } or numblock { _1.count } or itblock { it.count }
    if let Some(blk) = tv.block() {
        if let Some(b) = blk.as_block_node() {
            let body = match b.body() { Some(b) => b, None => return false };
            let stmts = match body.as_statements_node() { Some(s) => s, None => return false };
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 { return false; }
            let stmt = &items[0];
            // Stmt must be a call: <v>.count/size/length with no args/block.
            let call = match stmt.as_call_node() { Some(c) => c, None => return false };
            let m = node_name!(&call);
            if !matches!(&*m, "count" | "size" | "length") { return false; }
            if call.arguments().is_some() { return false; }
            if call.block().is_some() { return false; }
            let recv = match call.receiver() { Some(r) => r, None => return false };
            let recv_src = src(&recv, source);
            let params = b.parameters();
            if let Some(p) = params.as_ref() {
                if let Some(bp) = p.as_block_parameters_node() {
                    if let Some(inner) = bp.parameters() {
                        let reqs: Vec<_> = inner.requireds().iter().collect();
                        if reqs.len() == 1 && src(&reqs[0], source) == recv_src {
                            return true;
                        }
                    }
                } else if p.as_numbered_parameters_node().is_some() {
                    if recv_src == "_1" { return true; }
                } else if p.as_it_parameters_node().is_some() {
                    if recv_src == "it" { return true; }
                }
            } else if recv_src == "it" {
                return true;
            }
        }
    }
    false
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        if !self.check_each_with_object(node) {
            self.check_group_by_transform(node);
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/TallyMethod", |_cfg| Some(Box::new(TallyMethod::new())));
