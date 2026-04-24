//! Style/MultilineInPatternThen - Checks uses of `then` keyword in multi-line `in` statement.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/multiline_in_pattern_then.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct MultilineInPatternThen;

impl MultilineInPatternThen {
    pub fn new() -> Self { Self }
}

impl Cop for MultilineInPatternThen {
    fn name(&self) -> &'static str { "Style/MultilineInPatternThen" }

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
        let Some(then_loc) = node.then_loc() else {
            ruby_prism::visit_in_node(self, node);
            return;
        };

        // require_then? logic from RuboCop:
        //   true if pattern spans multiple lines
        //   false if no body
        //   otherwise: `in` and body on same line
        let pattern = node.pattern();
        let pat_start = pattern.location().start_offset();
        let pat_end = pattern.location().end_offset();
        let pattern_multiline = self.ctx.line_of(pat_start) != self.ctx.line_of(pat_end);

        let require_then = if pattern_multiline {
            true
        } else if let Some(body) = node.statements() {
            let in_line = self.ctx.line_of(node.in_loc().start_offset());
            let body_line = self.ctx.line_of(body.location().start_offset());
            in_line == body_line
        } else {
            false
        };

        if require_then {
            ruby_prism::visit_in_node(self, node);
            return;
        }

        // Offense: `then` keyword range
        let then_start = then_loc.start_offset();
        let then_end = then_loc.end_offset();

        // Correction: remove `then` with preceding whitespace (but not newlines)
        let src = self.ctx.source.as_bytes();
        let mut del_start = then_start;
        while del_start > 0 && (src[del_start - 1] == b' ' || src[del_start - 1] == b'\t') {
            del_start -= 1;
        }

        let off = self.ctx.offense_with_range(
            "Style/MultilineInPatternThen",
            "Do not use `then` for multiline `in` statement.",
            Severity::Convention,
            then_start,
            then_end,
        ).with_correction(Correction::delete(del_start, then_end));
        self.offenses.push(off);

        ruby_prism::visit_in_node(self, node);
    }
}

crate::register_cop!("Style/MultilineInPatternThen", |_cfg| {
    Some(Box::new(MultilineInPatternThen::new()))
});
