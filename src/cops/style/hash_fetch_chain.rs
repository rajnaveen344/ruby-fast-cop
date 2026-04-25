//! Style/HashFetchChain cop
//!
//! Replaces chained `fetch(_, nil)` calls (with `nil`/`{}`/`Hash.new` defaults)
//! with a single call to `dig`.
//!
//! Mirrors RuboCop's `lib/rubocop/cop/style/hash_fetch_chain.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

#[derive(Default)]
pub struct HashFetchChain;
impl HashFetchChain { pub fn new() -> Self { Self } }

impl Cop for HashFetchChain {
    fn name(&self) -> &'static str { "Style/HashFetchChain" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 3) { return vec![]; }
        let mut v = V { ctx, offenses: Vec::new(), ignored: HashSet::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    ignored: HashSet<usize>,
}

/// Match `fetch(arg, default)` where default ∈ {nil, {}, Hash.new, ::Hash.new}.
/// Returns the first argument's source range if matches.
fn diggable_arg<'a>(call: &ruby_prism::CallNode<'a>) -> Option<(usize, usize)> {
    if node_name!(call) != "fetch" { return None; }
    let args = call.arguments()?;
    let list: Vec<Node> = args.arguments().iter().collect();
    if list.len() != 2 { return None; }
    // Block to fetch makes it non-diggable (RuboCop pattern excludes blocks via `(call ...)` form).
    if call.block().is_some() { return None; }
    let default = &list[1];
    let ok = match default {
        Node::NilNode { .. } => true,
        Node::HashNode { .. } => {
            // Only literal `{}` (empty hash). Pattern `(hash)` matches all hashes; RuboCop pattern allows any hash literal.
            // Actually pattern is `(hash)` w/ no capture → any hash node. Allow.
            true
        }
        Node::CallNode { .. } => {
            let c = default.as_call_node().unwrap();
            if node_name!(c) != "new" { return None; }
            if c.arguments().map_or(false, |a| a.arguments().iter().count() > 0) { return None; }
            let recv = c.receiver()?;
            match &recv {
                Node::ConstantReadNode { .. } => {
                    let cr = recv.as_constant_read_node().unwrap();
                    std::str::from_utf8(cr.name().as_slice()).unwrap_or("") == "Hash"
                }
                Node::ConstantPathNode { .. } => {
                    let cp = recv.as_constant_path_node().unwrap();
                    // ::Hash → parent is None (cbase), name is Hash
                    if cp.parent().is_some() { return None; }
                    let n = cp.name()?;
                    std::str::from_utf8(n.as_slice()).unwrap_or("") == "Hash"
                }
                _ => return None,
            }
        }
        _ => false,
    };
    if !ok { return None; }
    let loc = list[0].location();
    Some((loc.start_offset(), loc.end_offset()))
}

fn last_arg_is_nil(call: &ruby_prism::CallNode) -> bool {
    let args = match call.arguments() { Some(a) => a, None => return false };
    let list: Vec<Node> = args.arguments().iter().collect();
    matches!(list.last(), Some(Node::NilNode { .. }))
}

impl<'a> V<'a> {
    fn check(&mut self, node: &ruby_prism::CallNode<'a>) {
        if node_name!(node) != "fetch" { return; }
        let id = node.location().start_offset();
        if self.ignored.contains(&id) { return; }
        if !last_arg_is_nil(node) { return; }

        // Walk chain: collect first-args while diggable.
        let mut args_src: Vec<String> = Vec::new();
        let mut last_replaceable_start: Option<usize> = None;

        // Process outermost first
        {
            let arg = match diggable_arg(node) { Some(a) => a, None => return };
            args_src.push(self.ctx.source[arg.0..arg.1].to_string());
            self.ignored.insert(node.location().start_offset());
            let sel = match node.message_loc() { Some(l) => l, None => return };
            last_replaceable_start = Some(sel.start_offset());
        }

        // Descend through receivers
        let mut cur_recv = node.receiver();
        while let Some(r) = cur_recv {
            let c = match r.as_call_node() { Some(c) => c, None => break };
            let arg = match diggable_arg(&c) { Some(a) => a, None => break };
            args_src.push(self.ctx.source[arg.0..arg.1].to_string());
            self.ignored.insert(c.location().start_offset());
            let sel = match c.message_loc() { Some(l) => l, None => break };
            last_replaceable_start = Some(sel.start_offset());
            cur_recv = c.receiver();
        }

        if args_src.len() < 2 { return; }
        let start = match last_replaceable_start { Some(s) => s, None => return };
        let end = node.location().end_offset();

        // args_src is outermost→innermost. Reverse for innermost→outermost (dig order).
        let joined: Vec<String> = args_src.into_iter().rev().collect();
        let replacement = format!("dig({})", joined.join(", "));
        let msg = format!("Use `{}` instead.", replacement);

        self.offenses.push(
            self.ctx.offense_with_range("Style/HashFetchChain", &msg, Severity::Convention, start, end)
                .with_correction(Correction::replace(start, end, replacement)),
        );
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/HashFetchChain", |_cfg| Some(Box::new(HashFetchChain::new())));
