//! Style/RandomWithOffset — Prefer ranges with rand instead of offsets.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/random_with_offset.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Prefer ranges when generating random numbers instead of integers with offsets.";

#[derive(Default)]
pub struct RandomWithOffset;

impl RandomWithOffset {
    pub fn new() -> Self { Self }
}

fn integer_value(node: &Node, source: &str) -> Option<i64> {
    node.as_integer_node()?;
    let loc = node.location();
    let text = &source[loc.start_offset()..loc.end_offset()];
    text.parse::<i64>().ok()
}

/// Returns (prefix, lo, hi) — inclusive boundaries of the generated range.
/// rand(n) → [0, n-1]; rand(0..n) → [0, n]; rand(0...n) → [0, n-1].
fn extract_rand_boundaries(node: &ruby_prism::CallNode, source: &str) -> Option<(String, i64, i64)> {
    let method = node_name!(node);
    if method != "rand" { return None; }

    let prefix = if let Some(recv) = node.receiver() {
        let s = &source[recv.location().start_offset()..recv.location().end_offset()];
        match s {
            "Kernel" | "::Kernel" | "Random" | "::Random" => s.to_string(),
            _ => return None,
        }
    } else {
        String::new()
    };

    let args = node.arguments()?;
    let arg_list: Vec<_> = args.arguments().iter().collect();
    if arg_list.len() != 1 { return None; }
    let arg = &arg_list[0];

    // Integer arg: rand(n) → [0, n-1]
    if let Some(n) = integer_value(arg, source) {
        return Some((prefix, 0, n - 1));
    }

    // Range arg
    if let Some(range) = arg.as_range_node() {
        let from = range.left()?;
        let to = range.right()?;
        let lo = integer_value(&from, source)?;
        let hi_val = integer_value(&to, source)?;
        let op_src = &source[range.operator_loc().start_offset()..range.operator_loc().end_offset()];
        let exclusive = op_src == "...";
        let hi = if exclusive { hi_val - 1 } else { hi_val };
        return Some((prefix, lo, hi));
    }

    None
}

fn rand_src(prefix: &str, lo: i64, hi: i64) -> String {
    if prefix.is_empty() {
        format!("rand({lo}..{hi})")
    } else {
        format!("{prefix}.rand({lo}..{hi})")
    }
}

struct RandomWithOffsetVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RandomWithOffsetVisitor<'a> {
    fn try_check(&mut self, node: &ruby_prism::CallNode) -> Option<()> {
        let op = node_name!(node);

        // Pattern 1: rand(n).succ / rand(n).next / rand(n).pred
        if op == "succ" || op == "next" || op == "pred" {
            let recv = node.receiver()?;
            let rand_call = recv.as_call_node()?;
            let (prefix, lo, hi) = extract_rand_boundaries(&rand_call, self.ctx.source)?;
            let (new_lo, new_hi) = if op == "succ" || op == "next" {
                (lo + 1, hi + 1)
            } else {
                (lo - 1, hi - 1)
            };
            let correction = rand_src(&prefix, new_lo, new_hi);
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            let offense = self.ctx.offense_with_range(
                "Style/RandomWithOffset", MSG, Severity::Convention, start, end,
            ).with_correction(Correction::replace(start, end, correction));
            self.offenses.push(offense);
            return Some(());
        }

        // Pattern 2: rand(n) + k or rand(n) - k
        if op == "+" || op == "-" {
            let recv = node.receiver()?;
            let args = node.arguments()?;
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() != 1 { return None; }

            // rand(n) +/- k
            if let Some(rand_call) = recv.as_call_node() {
                if let Some((prefix, lo, hi)) = extract_rand_boundaries(&rand_call, self.ctx.source) {
                    if let Some(k) = integer_value(&arg_list[0], self.ctx.source) {
                        let (new_lo, new_hi) = if op == "+" {
                            (lo + k, hi + k)
                        } else {
                            (lo - k, hi - k)
                        };
                        let correction = rand_src(&prefix, new_lo, new_hi);
                        let start = node.location().start_offset();
                        let end = node.location().end_offset();
                        let offense = self.ctx.offense_with_range(
                            "Style/RandomWithOffset", MSG, Severity::Convention, start, end,
                        ).with_correction(Correction::replace(start, end, correction));
                        self.offenses.push(offense);
                        return Some(());
                    }
                }
            }

            // k + rand(n) or k - rand(n)
            if let Some(k) = integer_value(&recv, self.ctx.source) {
                if let Some(rand_call) = arg_list[0].as_call_node() {
                    if let Some((prefix, lo, hi)) = extract_rand_boundaries(&rand_call, self.ctx.source) {
                        let (new_lo, new_hi) = if op == "+" {
                            (k + lo, k + hi)
                        } else {
                            // k - [lo, hi] → [k-hi, k-lo]
                            (k - hi, k - lo)
                        };
                        let correction = rand_src(&prefix, new_lo, new_hi);
                        let start = node.location().start_offset();
                        let end = node.location().end_offset();
                        let offense = self.ctx.offense_with_range(
                            "Style/RandomWithOffset", MSG, Severity::Convention, start, end,
                        ).with_correction(Correction::replace(start, end, correction));
                        self.offenses.push(offense);
                        return Some(());
                    }
                }
            }
        }

        None
    }
}

impl<'a> Visit<'_> for RandomWithOffsetVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.try_check(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for RandomWithOffset {
    fn name(&self) -> &'static str { "Style/RandomWithOffset" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RandomWithOffsetVisitor { ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

crate::register_cop!("Style/RandomWithOffset", |_cfg| {
    Some(Box::new(RandomWithOffset::new()))
});
