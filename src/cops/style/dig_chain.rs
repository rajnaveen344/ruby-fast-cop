//! Style/DigChain cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const MSG_TMPL: &str = "Use `{}` instead of chaining.";

#[derive(Default)]
pub struct DigChain;
impl DigChain { pub fn new() -> Self { Self } }

impl Cop for DigChain {
    fn name(&self) -> &'static str { "Style/DigChain" }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
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

/// Single-hash-arg or kwargs-only call to `dig` is not safe to chain.
fn dig_args_ok(call: &ruby_prism::CallNode) -> bool {
    let args = match call.arguments() { Some(a) => a, None => return false };
    let list: Vec<Node> = args.arguments().iter().collect();
    if list.is_empty() { return false; }
    // Reject hash arg (single-hash or kwargs-only)
    // Single arg that's a HashNode or KeywordHashNode → bad
    if list.len() == 1 {
        if matches!(list[0], Node::HashNode { .. } | Node::KeywordHashNode { .. }) {
            return false;
        }
    }
    // Any kwargs-only arg in middle? For safety reject any KeywordHashNode.
    if list.iter().any(|n| matches!(n, Node::KeywordHashNode { .. })) {
        return false;
    }
    // Reject any BlockArgumentNode whose child is nil (anonymous `&`) — or simply reject `&`/`**` forwarding shapes other than `...` and `*`.
    // Prism: anonymous block arg = BlockArgumentNode with no expression.
    for a in &list {
        if let Some(ba) = a.as_block_argument_node() {
            if ba.expression().is_none() { return false; }
        }
        // Anonymous **: Prism has KeywordHashNode? Actually `**` alone is KeywordHashNode with AssocSplatNode(no value)
        if let Some(kh) = a.as_keyword_hash_node() {
            // Already rejected above.
            let _ = kh;
        }
    }
    true
}

fn is_dig_call(n: &Node) -> bool {
    n.as_call_node().map(|c| node_name!(c) == "dig").unwrap_or(false)
}

/// Any ForwardingArgumentsNode (`...`) as argument?
fn forwarded_args_index(call: &ruby_prism::CallNode) -> Option<usize> {
    let args = call.arguments()?;
    for (i, a) in args.arguments().iter().enumerate() {
        if a.as_forwarding_arguments_node().is_some() { return Some(i); }
    }
    None
}

impl<'a> V<'a> {
    fn check(&mut self, node: &ruby_prism::CallNode<'a>) {
        let id = node.location().start_offset();
        if self.ignored.contains(&id) { return; }
        if node_name!(node) != "dig" { return; }
        // require explicit dot/:: (skip bare `dig` at outer level).
        if node.call_operator_loc().is_none() { return; }
        // Must have at least one valid arg
        if !dig_args_ok(node) { return; }
        // Walk receivers gathering dig chain.
        // Collect args-per-dig (outermost first, we'll reverse before joining)
        // and innermost selector range.
        let mut args_per_dig: Vec<Vec<String>> = Vec::new();
        // outermost args
        {
            let args = node.arguments().unwrap();
            args_per_dig.push(args.arguments().iter().map(|a| {
                let loc = a.location();
                self.ctx.source[loc.start_offset()..loc.end_offset()].to_string()
            }).collect());
        }
        let mut innermost_selector_start: Option<usize> = None;
        let mut inner_start_offsets: Vec<usize> = Vec::new();
        let mut cur_recv = node.receiver();
        while let Some(r) = cur_recv {
            let rc = match r.as_call_node() { Some(c) => c, None => break };
            if node_name!(rc) != "dig" { break; }
            // If receiver has its own receiver, require operator ≠ `::`.
            // Innermost bare `dig` has no receiver and no operator → allowed.
            if rc.receiver().is_some() {
                let rop = match rc.call_operator_loc() { Some(l) => l, None => break };
                let rop_src = &self.ctx.source[rop.start_offset()..rop.end_offset()];
                if rop_src == "::" { break; }
            }
            if !dig_args_ok(&rc) { break; }
            let sel = match rc.message_loc() { Some(l) => l, None => break };
            innermost_selector_start = Some(sel.start_offset());
            inner_start_offsets.push(rc.location().start_offset());
            let args = rc.arguments().unwrap();
            args_per_dig.push(args.arguments().iter().map(|a| {
                let loc = a.location();
                self.ctx.source[loc.start_offset()..loc.end_offset()].to_string()
            }).collect());
            cur_recv = rc.receiver();
        }
        if args_per_dig.len() < 2 { return; }
        // Mark inner digs as ignored
        for off in &inner_start_offsets {
            self.ignored.insert(*off);
        }
        // Build args list: innermost first. args_per_dig is outermost→innermost; reverse.
        let mut all_args_src: Vec<String> = Vec::new();
        for group in args_per_dig.iter().rev() {
            for a in group { all_args_src.push(a.clone()); }
        }
        let fwd_positions: Vec<usize> = all_args_src.iter().enumerate()
            .filter(|(_, s)| s.as_str() == "...").map(|(i, _)| i).collect();
        if fwd_positions.iter().any(|&i| i < all_args_src.len() - 1) { return; }
        let replacement = format!("dig({})", all_args_src.join(", "));
        let msg_text = replacement.clone();
        let start = match innermost_selector_start { Some(s) => s, None => return };
        let end = node.location().end_offset();
        // Handle comments between call beginning and end: lift to before entire expression.
        let call_full_start = {
            // Walk down receivers to the leftmost start.
            let mut leftmost = node.location().start_offset();
            let mut r = node.receiver();
            while let Some(rr) = r {
                let rc = match rr.as_call_node() { Some(c) => c, None => break };
                if node_name!(rc) != "dig" { break; }
                leftmost = rc.location().start_offset();
                r = rc.receiver();
            }
            leftmost
        };
        // Collect line-internal `#...` comments between call_full_start and `end`, except final trailing comment on outer call's line.
        let src = self.ctx.source;
        let bytes = src.as_bytes();
        let outer_end_line_end = {
            let mut i = end;
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            i
        };
        let mut intermediate_comments: Vec<String> = Vec::new();
        // Scan source from call_full_start to outer_end_line_end for `#` starts.
        let mut i = call_full_start;
        while i < outer_end_line_end {
            if bytes[i] == b'#' {
                // Find end-of-line
                let mut j = i;
                while j < bytes.len() && bytes[j] != b'\n' { j += 1; }
                let comment = &src[i..j];
                // If comment is on the same line as outer `end` → skip (trailing, preserved).
                let comment_line_end = j;
                if comment_line_end != outer_end_line_end {
                    intermediate_comments.push(comment.to_string());
                }
                i = j;
            } else {
                i += 1;
            }
        }
        let (final_start, final_end, final_replacement) = if intermediate_comments.is_empty() {
            (start, end, replacement)
        } else {
            // Build replacement that lifts comments to before the call, preserves indentation of call_full_start line.
            // Column of call_full_start
            let line_start = src[..call_full_start].rfind('\n').map_or(0, |p| p + 1);
            let indent = &src[line_start..call_full_start];
            let mut s = String::new();
            for c in &intermediate_comments {
                s.push_str(indent);
                s.push_str(c);
                s.push('\n');
            }
            // Then the original leftmost up to innermost selector, then replacement
            s.push_str(&src[call_full_start..start]);
            s.push_str(&replacement);
            (call_full_start, end, s)
        };
        let (correction_start, correction_end, correction_text) = (final_start, final_end, final_replacement);
        // Offense range remains (start..end) = innermost_selector..outer_end
        let msg = MSG_TMPL.replace("{}", &msg_text);
        self.offenses.push(
            self.ctx.offense_with_range("Style/DigChain", &msg, Severity::Convention, start, end)
                .with_correction(Correction::replace(correction_start, correction_end, correction_text)),
        );
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/DigChain", |_cfg| Some(Box::new(DigChain::new())));
