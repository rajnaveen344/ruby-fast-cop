//! Style/SlicingWithRange — Prefer endless ranges for slicing.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/slicing_with_range.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct SlicingWithRange;

impl SlicingWithRange {
    pub fn new() -> Self { Self }
}

fn integer_value_text(node: &Node, source: &str) -> Option<i64> {
    node.as_integer_node()?;
    let loc = node.location();
    let text = &source[loc.start_offset()..loc.end_offset()];
    text.parse::<i64>().ok()
}

fn is_nil(node: &Node) -> bool {
    matches!(node, Node::NilNode { .. })
}

fn is_negative_one(node: &Node, source: &str) -> bool {
    // Check for literal `-1` — in Prism this appears as a call node `-` on IntegerNode `1`,
    // or as an IntegerNode with value -1 (depends on how Prism parses it).
    // Actually for `-1` inside a range, Prism may parse as `(call IntegerNode(1), "-@")`
    // Let's check source text directly.
    let loc = node.location();
    let text = &source[loc.start_offset()..loc.end_offset()];
    text == "-1"
}

fn is_zero(node: &Node, source: &str) -> bool {
    integer_value_text(node, source) == Some(0)
}

impl Cop for SlicingWithRange {
    fn name(&self) -> &'static str {
        "Style/SlicingWithRange"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only apply if ruby >= 2.6
        if ctx.target_ruby_version < 2.6 { return vec![]; }
        let mut visitor = SlicingVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct SlicingVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> SlicingVisitor<'a> {
    /// Check `ary[range]` style (index access without `.`)
    fn check_index_call(&mut self, node: &ruby_prism::CallNode) {
        // Must be `[]` method call (index access without dot notation OR with dot `[]`)
        let method = node_name!(node);
        if method != "[]" { return; }

        // Check if it uses parentheses-less style (bracket notation `ary[range]`)
        // vs dot style `ary.[](range)` — we check by looking at call_operator_loc
        let has_dot = node.call_operator_loc().is_some();
        // If has_dot, then it's `ary.[](range)` — parentheses required for offense to apply
        // Actually we check arguments for range
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 { return; }

        let arg = &arg_list[0];
        let range = match arg.as_range_node() {
            Some(r) => r,
            None => return,
        };

        let op_loc = range.operator_loc();
        let op_src = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
        let exclusive = op_src == "...";

        let from = range.left();
        let to = range.right();

        // For dot-style `ary.[](range)`, only flag if args are parenthesized
        if has_dot {
            self.check_dot_style_range(node, &range, from, to, exclusive);
            return;
        }

        // Bracket style: `ary[range]`
        self.check_bracket_style_range(node, &range, from, to, exclusive);
    }

    fn check_bracket_style_range(
        &mut self,
        call_node: &ruby_prism::CallNode,
        range: &ruby_prism::RangeNode,
        from: Option<Node>,
        to: Option<Node>,
        exclusive: bool,
    ) {
        // Get the receiver source
        let recv = match call_node.receiver() {
            Some(r) => r,
            None => return,
        };
        let recv_src = &self.ctx.source[recv.location().start_offset()..recv.location().end_offset()];

        // Full range source
        let range_start = range.location().start_offset();
        let range_end = range.location().end_offset();
        // The bracket region: from `[` to `]`
        // opening_loc is `[`
        let bracket_start = match call_node.opening_loc() {
            Some(loc) => loc.start_offset(),
            None => return,
        };
        let bracket_end = match call_node.closing_loc() {
            Some(loc) => loc.end_offset(),
            None => return,
        };

        // Pattern 1: ary[0..-1] or ary[0..nil] — remove entire slice
        if let Some(ref f) = from {
            if is_zero(f, self.ctx.source) {
                let should_flag = match &to {
                    Some(t) => is_negative_one(t, self.ctx.source) && !exclusive
                                || is_nil(t),
                    None => false,
                };
                if should_flag {
                    let range_src_display = &self.ctx.source[bracket_start..bracket_end];
                    let msg = format!("Remove the useless `{}`.", range_src_display);
                    let call_start = call_node.location().start_offset();
                    let call_end = call_node.location().end_offset();
                    let offense = self.ctx.offense_with_range(
                        "Style/SlicingWithRange", &msg, Severity::Convention,
                        bracket_start, bracket_end,
                    ).with_correction(Correction::replace(call_start, call_end, recv_src.to_string()));
                    self.offenses.push(offense);
                    return;
                }
            }
        }

        // Pattern 2: nil..n at Ruby 2.7+ — `ary[nil..n]` → `ary[..n]`
        if self.ctx.target_ruby_version >= 2.7 {
            if let Some(ref f) = from {
                if is_nil(f) {
                    if let Some(ref t) = to {
                        if !is_nil(t) {
                            let to_src = &self.ctx.source[t.location().start_offset()..t.location().end_offset()];
                            let op = if exclusive { "..." } else { ".." };
                            let msg = format!("Prefer `[{op}{to_src}]` over `[nil{op}{to_src}]`.");
                            let new_range = format!("{op}{to_src}");
                            let offense = self.ctx.offense_with_range(
                                "Style/SlicingWithRange", &msg, Severity::Convention,
                                bracket_start, bracket_end,
                            ).with_correction(Correction::replace(bracket_start, bracket_end,
                                format!("[{new_range}]")));
                            self.offenses.push(offense);
                            return;
                        }
                    }
                }
            }
        }

        // Pattern 3: ary[n..-1] or ary[n..nil] — use endless range
        // Only if from is NOT nil and to is -1 or nil
        if let Some(ref f) = from {
            if !is_nil(f) && !is_zero(f, self.ctx.source) {
                let from_src = &self.ctx.source[f.location().start_offset()..f.location().end_offset()];
                match &to {
                    Some(t) if is_negative_one(t, self.ctx.source) && !exclusive => {
                        let msg = format!("Prefer `[{from_src}..]` over `[{from_src}..-1]`.");
                        let offense = self.ctx.offense_with_range(
                            "Style/SlicingWithRange", &msg, Severity::Convention,
                            bracket_start, bracket_end,
                        ).with_correction(Correction::replace(bracket_start, bracket_end,
                            format!("[{from_src}..]")));
                        self.offenses.push(offense);
                    }
                    Some(t) if is_nil(t) => {
                        let op = if exclusive { "..." } else { ".." };
                        let msg = format!("Prefer `[{from_src}{op}]` over `[{from_src}{op}nil]`.");
                        let offense = self.ctx.offense_with_range(
                            "Style/SlicingWithRange", &msg, Severity::Convention,
                            bracket_start, bracket_end,
                        ).with_correction(Correction::replace(bracket_start, bracket_end,
                            format!("[{from_src}{op}]")));
                        self.offenses.push(offense);
                    }
                    None => {
                        // startless range without end
                    }
                    _ => {}
                }
            } else if is_zero(f, self.ctx.source) {
                // Handle ary[0..n] — no offense (non-slicing)
            }
        }
    }

    fn check_dot_style_range(
        &mut self,
        call_node: &ruby_prism::CallNode,
        range: &ruby_prism::RangeNode,
        from: Option<Node>,
        to: Option<Node>,
        exclusive: bool,
    ) {
        let recv = match call_node.receiver() {
            Some(r) => r,
            None => return,
        };
        let recv_src = &self.ctx.source[recv.location().start_offset()..recv.location().end_offset()];

        let has_paren = call_node.opening_loc().is_some();
        let call_start = call_node.location().start_offset();
        let call_end = call_node.location().end_offset();

        // The dot+method span: from `.` to end of call
        let dot_start = call_node.call_operator_loc()
            .map(|l| l.start_offset())
            .unwrap_or(recv.location().end_offset());

        // Pattern 1: .[](0..-1) or `.[] 0..-1` — remove entire method call (works with or without parens)
        if let Some(ref f) = from {
            if is_zero(f, self.ctx.source) {
                let should_flag = match &to {
                    Some(t) => is_negative_one(t, self.ctx.source) && !exclusive || is_nil(t),
                    None => false,
                };
                if should_flag {
                    let method_span_src = &self.ctx.source[dot_start..call_end];
                    let msg = format!("Remove the useless `{}`.", method_span_src);
                    let offense = self.ctx.offense_with_range(
                        "Style/SlicingWithRange", &msg, Severity::Convention,
                        dot_start, call_end,
                    ).with_correction(Correction::replace(call_start, call_end, recv_src.to_string()));
                    self.offenses.push(offense);
                    return;
                }
            }
        }

        // Patterns 2 and 3 require parentheses for correction
        if !has_paren { return; }

        // Pattern 2: nil..n at Ruby 2.7+
        if self.ctx.target_ruby_version >= 2.7 {
            if let Some(ref f) = from {
                if is_nil(f) {
                    if let Some(ref t) = to {
                        if !is_nil(t) {
                            let to_src = &self.ctx.source[t.location().start_offset()..t.location().end_offset()];
                            let op = if exclusive { "..." } else { ".." };
                            let msg = format!("Prefer `{op}{to_src}` over `nil{op}{to_src}`.");
                            let paren_start = call_node.opening_loc().unwrap().start_offset();
                            let paren_end = call_node.closing_loc().unwrap_or(call_node.location()).end_offset();
                            let offense = self.ctx.offense_with_range(
                                "Style/SlicingWithRange", &msg, Severity::Convention,
                                dot_start, call_end,
                            ).with_correction(Correction::replace(paren_start, paren_end,
                                format!("({op}{to_src})")));
                            self.offenses.push(offense);
                            return;
                        }
                    }
                }
            }
        }

        // Pattern 3: .[](n..-1) → .[](n..)
        if let Some(ref f) = from {
            if !is_nil(f) {
                let from_src = &self.ctx.source[f.location().start_offset()..f.location().end_offset()];
                match &to {
                    Some(t) if is_negative_one(t, self.ctx.source) && !exclusive => {
                        let msg = format!("Prefer `{from_src}..` over `{from_src}..-1`.");
                        let paren_start = call_node.opening_loc().unwrap().start_offset();
                        let paren_end = call_node.closing_loc().unwrap_or(call_node.location()).end_offset();
                        let offense = self.ctx.offense_with_range(
                            "Style/SlicingWithRange", &msg, Severity::Convention,
                            dot_start, call_end,
                        ).with_correction(Correction::replace(paren_start, paren_end,
                            format!("({from_src}..)")));
                        self.offenses.push(offense);
                    }
                    _ => {}
                }
            }
        }
    }
}

impl<'a> Visit<'_> for SlicingVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_index_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/SlicingWithRange", |_cfg| {
    Some(Box::new(SlicingWithRange::new()))
});
