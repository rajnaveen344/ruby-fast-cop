//! Style/SelectByKind - prefer `grep`/`grep_v` with class check.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/select_by_kind.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Node};

#[derive(Default)]
pub struct SelectByKind;

impl SelectByKind {
    pub fn new() -> Self { Self }
}

fn is_select_family(m: &str) -> bool { matches!(m, "select" | "filter" | "find_all") }
fn is_reject(m: &str) -> bool { m == "reject" }

fn block_param_name(block: &BlockNode) -> Option<String> {
    let params = block.parameters()?;
    match &params {
        Node::BlockParametersNode { .. } => {
            let bp = params.as_block_parameters_node()?;
            let inner = bp.parameters()?;
            let reqs: Vec<_> = inner.requireds().iter().collect();
            if reqs.len() != 1 { return None; }
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

fn is_param_lvar(node: &Node, name: &str) -> bool {
    match node {
        Node::LocalVariableReadNode { .. } => {
            node_name!(node.as_local_variable_read_node().unwrap()) == name
        }
        Node::ItLocalVariableReadNode { .. } => name == "it",
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            node_name!(c) == name && c.receiver().is_none() && c.arguments().is_none()
        }
        _ => false,
    }
}

fn extract_class_check(call: &CallNode, pname: &str, source: &str) -> Option<(bool, String)> {
    let n = node_name!(call);
    if n != "is_a?" && n != "kind_of?" { return None; }
    let recv = call.receiver()?;
    if !is_param_lvar(&recv, pname) { return None; }
    let args = call.arguments()?;
    let arg_list: Vec<_> = args.arguments().iter().collect();
    if arg_list.len() != 1 { return None; }
    let a = &arg_list[0];
    let loc = a.location();
    let src = source.get(loc.start_offset()..loc.end_offset())?.to_string();
    Some((true, src))
}

fn analyze_body(body: &Node, pname: &str, source: &str) -> Option<(bool, String)> {
    let call = body.as_call_node()?;
    if node_name!(call) == "!" {
        let inner = call.receiver()?;
        let inner_call = inner.as_call_node()?;
        let (ok, class_src) = extract_class_check(&inner_call, pname, source)?;
        if !ok { return None; }
        return Some((true, class_src));
    }
    let (ok, class_src) = extract_class_check(&call, pname, source)?;
    if !ok { return None; }
    Some((false, class_src))
}

fn receiver_is_hashlike(recv: &Node) -> bool {
    match recv {
        Node::HashNode { .. } => true,
        Node::CallNode { .. } => {
            let c = recv.as_call_node().unwrap();
            let name = node_name!(c).into_owned();
            if matches!(name.as_str(), "to_h" | "to_hash") { return true; }
            if let Some(r) = c.receiver() {
                if let Some(cr) = r.as_constant_read_node() {
                    let nn = String::from_utf8_lossy(cr.name().as_slice()).into_owned();
                    if nn == "Hash" && matches!(name.as_str(), "new" | "[]") { return true; }
                }
                if let Some(cp) = r.as_constant_path_node() {
                    if cp.parent().is_none() {
                        let nn = cp.name().map(|n| String::from_utf8_lossy(n.as_slice()).into_owned());
                        if nn.as_deref() == Some("Hash") && matches!(name.as_str(), "new" | "[]") { return true; }
                    }
                }
            }
            false
        }
        Node::ConstantReadNode { .. } => {
            node_name!(recv.as_constant_read_node().unwrap()) == "ENV"
        }
        Node::ConstantPathNode { .. } => {
            let cp = recv.as_constant_path_node().unwrap();
            cp.parent().is_none() &&
              cp.name().map_or(false, |n| String::from_utf8_lossy(n.as_slice()) == "ENV")
        }
        _ => false,
    }
}

fn block_offense_end(blk_any: &Node, block: &BlockNode, source: &str) -> usize {
    let open = block.opening_loc();
    let is_do = source.get(open.start_offset()..open.end_offset()) == Some("do");
    if is_do {
        block.parameters().map_or(open.end_offset(), |p| p.location().end_offset())
    } else {
        blk_any.location().end_offset()
    }
}

fn uses_safe_nav(call: &CallNode, source: &str) -> bool {
    call.call_operator_loc().map_or(false, |op| {
        source.get(op.start_offset()..op.end_offset()) == Some("&.")
    })
}

/// Extract the single body expression, handling StatementsNode and BeginNode.
fn single_body_expr<'a>(body: &Node<'a>) -> Option<Node<'a>> {
    // BeginNode (multi-statement) → skip per RuboCop.
    if let Node::BeginNode { .. } = body { return None; }
    if let Some(s) = body.as_statements_node() {
        let list: Vec<_> = s.body().iter().collect();
        if list.len() != 1 { return None; }
        return Some(list.into_iter().next().unwrap());
    }
    None
}

impl Cop for SelectByKind {
    fn name(&self) -> &'static str { "Style/SelectByKind" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let m: &str = &method;
        if !is_select_family(m) && !is_reject(m) { return vec![]; }
        if m == "filter" && !ctx.ruby_version_at_least(2, 6) { return vec![]; }

        let blk_any = match node.block() { Some(b) => b, None => return vec![] };
        let block = match blk_any.as_block_node() { Some(b) => b, None => return vec![] };

        if let Some(r) = node.receiver() {
            if receiver_is_hashlike(&r) { return vec![]; }
        }

        let pname = match block_param_name(&block) { Some(n) => n, None => return vec![] };
        let body = match block.body() { Some(b) => b, None => return vec![] };
        let expr = match single_body_expr(&body) { Some(e) => e, None => return vec![] };

        let (negated, class_src) = match analyze_body(&expr, &pname, ctx.source) {
            Some(v) => v,
            None => return vec![],
        };

        let replacement = if is_select_family(m) {
            if negated { "grep_v" } else { "grep" }
        } else if negated { "grep" } else { "grep_v" };

        let message = format!("Prefer `{}` to `{}` with a kind check.", replacement, m);

        let start = node.receiver().map_or(node.location().start_offset(), |r| r.location().start_offset());
        let off_end = block_offense_end(&blk_any, &block, ctx.source);
        let full_end = blk_any.location().end_offset();

        let mut off = ctx.offense_with_range(self.name(), &message, self.severity(), start, off_end);

        let nav = if uses_safe_nav(node, ctx.source) { "&." } else { "." };
        let recv_src = node.receiver().and_then(|r| {
            ctx.source.get(r.location().start_offset()..r.location().end_offset()).map(|s| s.to_string())
        });
        let corrected = match recv_src {
            Some(r) => format!("{}{}{}({})", r, nav, replacement, class_src),
            None => format!("{}({})", replacement, class_src),
        };
        off = off.with_correction(Correction::replace(start, full_end, corrected));
        vec![off]
    }
}

crate::register_cop!("Style/SelectByKind", |_cfg| Some(Box::new(SelectByKind::new())));
