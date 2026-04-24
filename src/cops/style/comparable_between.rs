//! Style/ComparableBetween cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct ComparableBetween;
impl ComparableBetween { pub fn new() -> Self { Self } }

/// Extract (left, method, right) for a comparison call.
fn as_comparison<'a>(n: &Node<'a>) -> Option<(Node<'a>, String, Node<'a>)> {
    let c = n.as_call_node()?;
    let m = node_name!(c).to_string();
    if !matches!(m.as_str(), ">=" | "<=") { return None; }
    let recv = c.receiver()?;
    let args = c.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 { return None; }
    let rhs = list.into_iter().next().unwrap();
    Some((recv, m, rhs))
}

fn src<'a>(n: &Node<'a>, source: &str) -> String {
    let loc = n.location();
    source[loc.start_offset()..loc.end_offset()].to_string()
}

impl Cop for ComparableBetween {
    fn name(&self) -> &'static str { "Style/ComparableBetween" }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> { ctx: &'a CheckContext<'a>, offenses: Vec<Offense> }

impl<'a> V<'a> {
    fn analyze_and(&mut self, node: &ruby_prism::AndNode<'a>) {
        let left = node.left();
        let right = node.right();
        let (l_lhs, l_op, l_rhs) = match as_comparison(&left) { Some(v) => v, None => return };
        let (r_lhs, r_op, r_rhs) = match as_comparison(&right) { Some(v) => v, None => return };

        let src_of = |n: &Node| src(n, self.ctx.source);
        // Each side: determine which operand is "value" and which is bound.
        // Possible forms:
        //   >= min   (lhs = value, rhs = min)
        //   <= value (lhs = min,   rhs = value)   — i.e. min <= value
        //   <= max   (lhs = value, rhs = max)
        //   >= value (lhs = max,   rhs = value)
        // For ComparableBetween we need to pair up: find shared source between comparisons — that's the value.
        let l_lhs_src = src_of(&l_lhs); let l_rhs_src = src_of(&l_rhs);
        let r_lhs_src = src_of(&r_lhs); let r_rhs_src = src_of(&r_rhs);
        // Find shared source.
        let candidates_l = [l_lhs_src.clone(), l_rhs_src.clone()];
        let candidates_r = [r_lhs_src.clone(), r_rhs_src.clone()];
        let value = candidates_l.iter().find(|s| candidates_r.contains(*s)).cloned();
        let value = match value { Some(v) => v, None => return };

        // Determine bound from each comparison = the non-value operand.
        let l_bound = if l_lhs_src == value { l_rhs_src.clone() } else { l_lhs_src.clone() };
        let r_bound = if r_lhs_src == value { r_rhs_src.clone() } else { r_lhs_src.clone() };

        // Determine which comparison gives min (value >= min OR min <= value) vs max.
        // For l: value is on lhs AND op=`>=` → it's "value >= min" → bound is min.
        //        value is on lhs AND op=`<=` → bound is max.
        //        value is on rhs AND op=`>=` → lhs op rhs = bound >= value → "max >= value" → bound is max.
        //        value is on rhs AND op=`<=` → "min <= value" → bound is min.
        let l_is_min = match (l_lhs_src == value, l_op.as_str()) {
            (true, ">=") => true,
            (true, "<=") => false,
            (false, ">=") => false,
            (false, "<=") => true,
            _ => return,
        };
        let r_is_min = match (r_lhs_src == value, r_op.as_str()) {
            (true, ">=") => true,
            (true, "<=") => false,
            (false, ">=") => false,
            (false, "<=") => true,
            _ => return,
        };
        if l_is_min == r_is_min { return; } // one must be min, other max
        let (min, max) = if l_is_min { (l_bound, r_bound) } else { (r_bound, l_bound) };

        let prefer = format!("{}.between?({}, {})", value, min, max);
        let msg = format!("Prefer `{}` over logical comparison.", prefer);
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        self.offenses.push(
            self.ctx.offense_with_range("Style/ComparableBetween", &msg, Severity::Convention, start, end)
                .with_correction(Correction::replace(start, end, prefer)),
        );
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'a>) {
        self.analyze_and(node);
        ruby_prism::visit_and_node(self, node);
    }
}

crate::register_cop!("Style/ComparableBetween", |_cfg| Some(Box::new(ComparableBetween::new())));
