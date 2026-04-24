//! Style/BitwisePredicate cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct BitwisePredicate;
impl BitwisePredicate { pub fn new() -> Self { Self } }

fn src<'a>(n: &Node<'a>, source: &'a str) -> &'a str {
    let l = n.location();
    &source[l.start_offset()..l.end_offset()]
}

/// If node is `(lhs & rhs)` — parentheses around a single `&` call — return (lhs, rhs) source strings.
fn as_bit_and<'a>(n: &Node<'a>, source: &'a str) -> Option<(String, String)> {
    let p = n.as_parentheses_node()?;
    let body = p.body()?;
    let inner = if let Some(s) = body.as_statements_node() {
        let items: Vec<_> = s.body().iter().collect();
        if items.len() != 1 { return None; }
        items.into_iter().next().unwrap()
    } else {
        body
    };
    let c = inner.as_call_node()?;
    let n = node_name!(c);
    if &*n != "&" { return None; }
    let recv = c.receiver()?;
    let args = c.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 { return None; }
    let rhs = list.into_iter().next().unwrap();
    Some((src(&recv, source).to_string(), src(&rhs, source).to_string()))
}

/// If node is an integer literal equal to `val`, return true.
fn is_int_lit(n: &Node, val: i64, source: &str) -> bool {
    if n.as_integer_node().is_none() { return false; }
    let l = n.location();
    let s = &source[l.start_offset()..l.end_offset()];
    s.trim() == val.to_string()
}

impl Cop for BitwisePredicate {
    fn name(&self) -> &'static str { "Style/BitwisePredicate" }
    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 5) { return vec![]; }
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> { ctx: &'a CheckContext<'a>, offenses: Vec<Offense> }

impl<'a> V<'a> {
    fn check_call(&mut self, c: &ruby_prism::CallNode<'a>) {
        let m_cow = node_name!(c);
        let m: &str = &m_cow;
        let source = self.ctx.source;
        // Case A: predicate method (no args): .positive? / .zero?
        if matches!(m, "positive?" | "zero?") {
            let recv = match c.receiver() { Some(r) => r, None => return };
            if c.arguments().is_some() { return; }
            let (lhs, rhs) = match as_bit_and(&recv, source) { Some(v) => v, None => return };
            let method = if m == "positive?" { "anybits?" } else { "nobits?" };
            self.emit(c, &lhs, &rhs, method);
            return;
        }
        // Case B: binary comparison with integer literal: >, >=, !=, ==
        if matches!(m, ">" | ">=" | "!=" | "==") {
            let recv = match c.receiver() { Some(r) => r, None => return };
            let args = match c.arguments() { Some(a) => a, None => return };
            let list: Vec<_> = args.arguments().iter().collect();
            if list.len() != 1 { return; }
            let rhs_arg = list.into_iter().next().unwrap();
            let (a_lhs, a_rhs) = match as_bit_and(&recv, source) { Some(v) => v, None => return };
            // anybits? cases
            match m {
                ">" if is_int_lit(&rhs_arg, 0, source) => { self.emit(c, &a_lhs, &a_rhs, "anybits?"); return; }
                ">=" if is_int_lit(&rhs_arg, 1, source) => { self.emit(c, &a_lhs, &a_rhs, "anybits?"); return; }
                "!=" if is_int_lit(&rhs_arg, 0, source) => { self.emit(c, &a_lhs, &a_rhs, "anybits?"); return; }
                "==" if is_int_lit(&rhs_arg, 0, source) => { self.emit(c, &a_lhs, &a_rhs, "nobits?"); return; }
                "==" => {
                    // allbits? — (x & f) == f OR (f & x) == f
                    let r = src(&rhs_arg, source);
                    if a_rhs == r { self.emit(c, &a_lhs, &a_rhs, "allbits?"); return; }
                    if a_lhs == r { self.emit(c, &a_rhs, &a_lhs, "allbits?"); return; }
                }
                _ => {}
            }
        }
    }

    fn emit(&mut self, c: &ruby_prism::CallNode<'a>, variable: &str, flags: &str, method: &str) {
        let loc = c.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        let replacement = format!("{}.{}({})", variable, method, flags);
        let msg = format!("Replace with `{}` for comparison with bit flags.", replacement);
        self.offenses.push(
            self.ctx.offense_with_range("Style/BitwisePredicate", &msg, Severity::Convention, start, end)
                .with_correction(Correction::replace(start, end, replacement)),
        );
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/BitwisePredicate", |_cfg| Some(Box::new(BitwisePredicate::new())));
