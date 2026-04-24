//! Style/CollectionCompact - prefer `compact`/`compact!` over reject/select on nil.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/collection_compact.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node};

pub struct CollectionCompact {
    allowed_receivers: Vec<String>,
}

impl Default for CollectionCompact {
    fn default() -> Self { Self { allowed_receivers: vec![] } }
}

impl CollectionCompact {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(allowed_receivers: Vec<String>) -> Self {
        Self { allowed_receivers }
    }
}

fn bang(name: &str) -> bool { name.ends_with('!') }

fn is_select_filter(m: &str) -> bool {
    matches!(m, "select" | "select!" | "filter" | "filter!")
}

fn block_single_param_name(block: &ruby_prism::BlockNode) -> Option<String> {
    let params = block.parameters()?;
    match &params {
        Node::BlockParametersNode { .. } => {
            let bp = params.as_block_parameters_node()?;
            let inner = bp.parameters()?;
            let reqs: Vec<_> = inner.requireds().iter().collect();
            if reqs.is_empty() { return None; }
            // RuboCop matches `(args ...)` but validates `args.last.source == receiver.source`,
            // so the last required param name must match the local variable used in `.nil?`.
            let rp = reqs.last()?.as_required_parameter_node()?;
            Some(node_name!(rp).into_owned())
        }
        Node::NumberedParametersNode { .. } => {
            let np = params.as_numbered_parameters_node()?;
            Some(format!("_{}", np.maximum()))
        }
        Node::ItParametersNode { .. } => Some("it".into()),
        _ => None,
    }
}

/// Check node is `x.nil?` or `x&.nil?` where x matches `name`.
fn is_nil_check(node: &Node, name: &str) -> bool {
    let c = match node.as_call_node() { Some(c) => c, None => return false };
    if node_name!(c) != "nil?" { return false; }
    if c.arguments().is_some() { return false; }
    let recv = match c.receiver() { Some(r) => r, None => return false };
    match &recv {
        Node::LocalVariableReadNode { .. } => {
            node_name!(recv.as_local_variable_read_node().unwrap()) == name
        }
        Node::ItLocalVariableReadNode { .. } => name == "it",
        Node::CallNode { .. } => {
            let cc = recv.as_call_node().unwrap();
            node_name!(cc) == name && cc.receiver().is_none() && cc.arguments().is_none()
        }
        _ => false,
    }
}

/// Check node is `!x.nil?` or `x.nil?.!` or `x&.nil?&.!` or `!(x.nil?)`.
fn is_not_nil(node: &Node, name: &str) -> bool {
    let c = match node.as_call_node() { Some(c) => c, None => return false };
    if node_name!(c) != "!" { return false; }
    let recv = match c.receiver() { Some(r) => r, None => return false };
    // unwrap parens
    if let Some(p) = recv.as_parentheses_node() {
        if let Some(b) = p.body() {
            if let Some(s) = b.as_statements_node() {
                let stmts: Vec<_> = s.body().iter().collect();
                if stmts.len() == 1 { return is_nil_check(&stmts[0], name); }
            }
        }
        return false;
    }
    is_nil_check(&recv, name)
}

fn receiver_is_to_enum_or_lazy(call: &CallNode) -> bool {
    if let Some(r) = call.receiver() {
        if let Some(c) = r.as_call_node() {
            let n = node_name!(c);
            return n == "to_enum" || n == "lazy";
        }
    }
    false
}

fn receiver_source<'a>(call: &CallNode, source: &'a str) -> Option<&'a str> {
    let r = call.receiver()?;
    source.get(r.location().start_offset()..r.location().end_offset())
}

fn receiver_matches_allowed(call: &CallNode, source: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() { return false; }
    let recv = match call.receiver() { Some(r) => r, None => return false };
    let src = match receiver_source(call, source) { Some(s) => s, None => return false };
    if allowed.iter().any(|a| a == src) { return true; }
    if let Some(c) = recv.as_call_node() {
        let mn = node_name!(c).into_owned();
        if allowed.iter().any(|a| *a == mn) { return true; }
        let mut cur = c;
        loop {
            match cur.receiver() {
                Some(inner) => {
                    if let Some(cc) = inner.as_call_node() {
                        let nn = node_name!(cc).into_owned();
                        if allowed.iter().any(|a| *a == nn) { return true; }
                        cur = cc;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
    }
    if let Some(lv) = recv.as_local_variable_read_node() {
        let n = node_name!(lv).into_owned();
        return allowed.iter().any(|a| *a == n);
    }
    false
}

fn is_nil_or_nilclass_const(node: &Node) -> bool {
    match node {
        Node::NilNode { .. } => true,
        Node::ConstantReadNode { .. } => {
            node_name!(node.as_constant_read_node().unwrap()) == "NilClass"
        }
        Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            if cp.parent().is_some() { return false; }
            cp.name().map_or(false, |n| String::from_utf8_lossy(n.as_slice()) == "NilClass")
        }
        _ => false,
    }
}

fn emit_offense(cop_name: &'static str, ctx: &CheckContext,
                start: usize, end: usize, bad_src: String, is_bang: bool) -> Vec<Offense> {
    let good = if is_bang { "compact!" } else { "compact" };
    let message = format!("Use `{}` instead of `{}`.", good, bad_src);
    let mut off = ctx.offense_with_range(cop_name, &message, Severity::Convention, start, end);
    off = off.with_correction(Correction::replace(start, end, good.to_string()));
    vec![off]
}

impl Cop for CollectionCompact {
    fn name(&self) -> &'static str { "Style/CollectionCompact" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let m: &str = &method;
        if !matches!(m, "reject" | "reject!" | "select" | "select!" | "filter" | "filter!" | "grep_v") {
            return vec![];
        }
        if node.receiver().is_none() { return vec![]; }
        // minimum_target_ruby_version 2.4
        if !ctx.ruby_version_at_least(2, 4) { return vec![]; }
        if matches!(m, "filter" | "filter!") && !ctx.ruby_version_at_least(2, 6) { return vec![]; }
        if receiver_matches_allowed(node, ctx.source, &self.allowed_receivers) { return vec![]; }
        if receiver_is_to_enum_or_lazy(node) && !ctx.ruby_version_at_least(3, 1) { return vec![]; }

        if m == "grep_v" {
            let args = match node.arguments() { Some(a) => a, None => return vec![] };
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() != 1 { return vec![]; }
            if !is_nil_or_nilclass_const(&arg_list[0]) { return vec![]; }
            let sel = node.message_loc().expect("selector");
            let start = sel.start_offset();
            let end = node.location().end_offset();
            let bad_src = ctx.source[start..end].to_string();
            return emit_offense(self.name(), ctx, start, end, bad_src, false);
        }

        if matches!(m, "reject" | "reject!") {
            if let Some(blk_any) = node.block() {
                if let Some(ba) = blk_any.as_block_argument_node() {
                    if let Some(expr) = ba.expression() {
                        if let Some(s) = expr.as_symbol_node() {
                            if let Some(v) = s.value_loc() {
                                let raw = ctx.source.get(v.start_offset()..v.end_offset()).unwrap_or("");
                                if raw != "nil?" { return vec![]; }
                                let sel = node.message_loc().expect("sel");
                                let start = sel.start_offset();
                                let end = node.location().end_offset();
                                let bad_src = ctx.source[start..end].to_string();
                                return emit_offense(self.name(), ctx, start, end, bad_src, bang(m));
                            }
                        }
                    }
                    return vec![];
                }
                if let Some(block) = blk_any.as_block_node() {
                    let pname = match block_single_param_name(&block) { Some(n) => n, None => return vec![] };
                    let body = match block.body() { Some(b) => b, None => return vec![] };
                    let stmts = match body.as_statements_node() { Some(s) => s, None => return vec![] };
                    let lst: Vec<_> = stmts.body().iter().collect();
                    if lst.len() != 1 { return vec![]; }
                    if !is_nil_check(&lst[0], &pname) { return vec![]; }
                    let sel = node.message_loc().expect("sel");
                    let start = sel.start_offset();
                    let end = blk_any.location().end_offset();
                    let bad_src = ctx.source[start..end].to_string();
                    return emit_offense(self.name(), ctx, start, end, bad_src, bang(m));
                }
            }
            return vec![];
        }

        if is_select_filter(m) {
            let blk_any = match node.block() { Some(b) => b, None => return vec![] };
            let block = match blk_any.as_block_node() { Some(b) => b, None => return vec![] };
            let pname = match block_single_param_name(&block) { Some(n) => n, None => return vec![] };
            let body = match block.body() { Some(b) => b, None => return vec![] };
            let stmts = match body.as_statements_node() { Some(s) => s, None => return vec![] };
            let lst: Vec<_> = stmts.body().iter().collect();
            if lst.len() != 1 { return vec![]; }
            if !is_not_nil(&lst[0], &pname) { return vec![]; }
            let sel = node.message_loc().expect("sel");
            let start = sel.start_offset();
            let end = blk_any.location().end_offset();
            let bad_src = ctx.source[start..end].to_string();
            return emit_offense(self.name(), ctx, start, end, bad_src, bang(m));
        }

        vec![]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_receivers: Vec<String>,
}

crate::register_cop!("Style/CollectionCompact", |cfg| {
    let c: Cfg = cfg.typed("Style/CollectionCompact");
    Some(Box::new(CollectionCompact::with_config(c.allowed_receivers)))
});
