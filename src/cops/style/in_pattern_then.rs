//! Style/InPatternThen - Checks for `in;` uses in `case` pattern expressions.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/in_pattern_then.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct InPatternThen;

impl InPatternThen {
    pub fn new() -> Self { Self }
}

impl Cop for InPatternThen {
    fn name(&self) -> &'static str { "Style/InPatternThen" }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_in_node(&mut self, node: &ruby_prism::InNode) {
        // Skip if it uses `then` (begin token is `then`) or has no body
        if node.then_loc().is_some() {
            ruby_prism::visit_in_node(self, node);
            return;
        }
        let Some(body) = node.statements() else {
            ruby_prism::visit_in_node(self, node);
            return;
        };
        // Skip if multiline: pattern end line != body start line
        let pattern = node.pattern();
        let pat_end = pattern.location().end_offset();
        let body_start = body.location().start_offset();
        if self.ctx.line_of(pat_end) != self.ctx.line_of(body_start) {
            ruby_prism::visit_in_node(self, node);
            return;
        }
        // Find `;` between pattern end and body start
        let src = self.ctx.source.as_bytes();
        let mut semi = None;
        let mut i = pat_end;
        while i < body_start {
            if src[i] == b';' { semi = Some(i); break; }
            i += 1;
        }
        let Some(semi_off) = semi else {
            ruby_prism::visit_in_node(self, node);
            return;
        };
        // Pattern source for message
        let in_kw_end = node.in_loc().end_offset();
        // Skip leading whitespace after `in`
        let mut p_start = in_kw_end;
        while p_start < semi_off && (src[p_start] == b' ' || src[p_start] == b'\t') {
            p_start += 1;
        }
        let pattern_src = &self.ctx.source[p_start..semi_off];
        let msg = format!("Do not use `in {0};`. Use `in {0} then` instead.", pattern_src);

        let off = self.ctx.offense_with_range(
            "Style/InPatternThen",
            &msg,
            Severity::Convention,
            semi_off,
            semi_off + 1,
        ).with_correction(Correction::replace(semi_off, semi_off + 1, " then"));
        self.offenses.push(off);

        ruby_prism::visit_in_node(self, node);
    }
}

crate::register_cop!("Style/InPatternThen", |_cfg| Some(Box::new(InPatternThen::new())));
