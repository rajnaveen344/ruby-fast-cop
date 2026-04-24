//! Style/MinMaxComparison cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct MinMaxComparison;
impl MinMaxComparison { pub fn new() -> Self { Self } }

const GREATER: &[&str] = &[">", ">="];
const LESS: &[&str] = &["<", "<="];

fn unwrap_parens<'a>(n: Node<'a>) -> Node<'a> {
    if let Some(p) = n.as_parentheses_node() {
        if let Some(b) = p.body() {
            if let Some(s) = b.as_statements_node() {
                let items: Vec<_> = s.body().iter().collect();
                if items.len() == 1 { return items.into_iter().next().unwrap(); }
            }
            return b;
        }
    }
    n
}

fn as_comparison<'a>(n: Node<'a>) -> Option<(Node<'a>, String, Node<'a>)> {
    let n = unwrap_parens(n);
    let c = n.as_call_node()?;
    let m = node_name!(c).to_string();
    if !GREATER.contains(&m.as_str()) && !LESS.contains(&m.as_str()) { return None; }
    let recv = c.receiver()?;
    let args = c.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 { return None; }
    Some((recv, m, list.into_iter().next().unwrap()))
}

fn same_src(a: &Node, b: &Node, source: &str) -> bool {
    let la = a.location(); let lb = b.location();
    source[la.start_offset()..la.end_offset()] == source[lb.start_offset()..lb.end_offset()]
}

fn src<'a>(n: &Node<'a>, source: &str) -> &'a str {
    // Can't return &'a — use &str from same lifetime as source.
    let loc = n.location();
    // The node belongs to the same arena-like source; borrow via unsafe pointer trickery not needed; return via caller.
    // Instead do: static lifetime won't work. Re-borrow:
    unsafe { std::mem::transmute::<&str, &'a str>(&source[loc.start_offset()..loc.end_offset()]) }
}

impl Cop for MinMaxComparison {
    fn name(&self) -> &'static str { "Style/MinMaxComparison" }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        let cond = node.predicate();
        let (lhs, op, rhs) = match as_comparison(cond) { Some(v) => v, None => return vec![] };
        // if branch is statements inside node.statements().body()
        let if_stmts = match node.statements() { Some(s) => s, None => return vec![] };
        let if_items: Vec<_> = if_stmts.body().iter().collect();
        if if_items.len() != 1 { return vec![]; }
        let if_branch = if_items.into_iter().next().unwrap();
        // else branch via consequent / subsequent
        let else_node = match node.subsequent() { Some(e) => e, None => return vec![] };
        let else_elsenode = match else_node.as_else_node() { Some(e) => e, None => return vec![] };
        let else_stmts = match else_elsenode.statements() { Some(s) => s, None => return vec![] };
        let else_items: Vec<_> = else_stmts.body().iter().collect();
        if else_items.len() != 1 { return vec![]; }
        let else_branch = else_items.into_iter().next().unwrap();

        let is_greater = GREATER.contains(&op.as_str());
        // Pair matching:
        let preferred = if same_src(&lhs, &if_branch, ctx.source) && same_src(&rhs, &else_branch, ctx.source) {
            if is_greater { "max" } else { "min" }
        } else if same_src(&lhs, &else_branch, ctx.source) && same_src(&rhs, &if_branch, ctx.source) {
            if !is_greater { "max" } else { "min" }
        } else {
            return vec![];
        };

        let lhs_s = src(&lhs, ctx.source);
        let rhs_s = src(&rhs, ctx.source);
        let replacement = format!("[{}, {}].{}", lhs_s, rhs_s, preferred);
        let msg = format!("Use `{}` instead.", replacement);
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        // Check if this is `elsif` (has `elsif` keyword)
        let is_elsif = node.if_keyword_loc()
            .map(|l| &ctx.source[l.start_offset()..l.end_offset()] == "elsif")
            .unwrap_or(false);
        let correction = if is_elsif {
            // Remove "elsif <cond>\n  <if_branch>\nelse\n  " (up to else body), keep else body replaced with replacement.
            // Simpler: rewrite whole IfNode -> "else\n  <replacement>\nend" ? No — outer `end` is shared.
            // Range: elsif keyword through end-keyword of inner if (= shared end of outer).
            // Actually inner IfNode's location covers "elsif cond ...end" (or may share `end`).
            // Replace "elsif <cond>\n  <if_branch>" part and the existing "else\n  <else_branch>" with "else\n  <replacement>".
            let elsif_start = match node.if_keyword_loc() { Some(l) => l.start_offset(), None => start };
            let new_text = {
                // Preserve indentation of else_branch.
                let b_loc = else_branch.location();
                let line_start = ctx.source[..b_loc.start_offset()].rfind('\n').map_or(0, |p| p + 1);
                let indent = &ctx.source[line_start..b_loc.start_offset()];
                format!("else\n{}{}", indent, replacement)
            };
            let end_of_else_branch = else_branch.location().end_offset();
            Correction::replace(elsif_start, end_of_else_branch, new_text)
        } else {
            Correction::replace(start, end, replacement)
        };
        vec![ctx.offense_with_range("Style/MinMaxComparison", &msg, Severity::Convention, start, end)
            .with_correction(correction)]
    }
}

crate::register_cop!("Style/MinMaxComparison", |_cfg| Some(Box::new(MinMaxComparison::new())));
