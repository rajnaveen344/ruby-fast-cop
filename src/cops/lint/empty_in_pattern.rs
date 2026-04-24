//! Lint/EmptyInPattern - Checks for `in` pattern branches without a body.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/empty_in_pattern.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct EmptyInPattern {
    allow_comments: bool,
}

impl EmptyInPattern {
    pub fn new(allow_comments: bool) -> Self { Self { allow_comments } }
}

impl Default for EmptyInPattern {
    fn default() -> Self { Self::new(true) }
}

impl Cop for EmptyInPattern {
    fn name(&self) -> &'static str { "Lint/EmptyInPattern" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { cop: self, ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    cop: &'a EmptyInPattern,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn has_comment_in_range(&self, start: usize, end: usize) -> bool {
        let bytes = self.ctx.source.as_bytes();
        let mut pos = start;
        while pos < end && pos < bytes.len() {
            if bytes[pos] == b'#' { return true; }
            pos += 1;
        }
        false
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        let conditions: Vec<_> = node.conditions().iter().collect();
        let n = conditions.len();
        for (i, cond) in conditions.iter().enumerate() {
            let Some(in_node) = cond.as_in_node() else { continue };
            if in_node.statements().is_some() { continue; }

            // Allow comments check
            if self.cop.allow_comments {
                let in_end = in_node.location().end_offset();
                let next_start = if i + 1 < n {
                    conditions[i + 1].location().start_offset()
                } else if let Some(e) = node.else_clause() {
                    e.location().start_offset()
                } else {
                    node.location().end_offset()
                };
                if self.has_comment_in_range(in_end, next_start) { continue; }
                // Also inline after `then`
                if let Some(then_loc) = in_node.then_loc() {
                    let after_then = then_loc.end_offset();
                    let eol = self.ctx.source[after_then..].find('\n')
                        .map_or(self.ctx.source.len(), |p| after_then + p);
                    if self.has_comment_in_range(after_then, eol) { continue; }
                }
            }

            // Offense range: `in` kw .. pattern end
            let start = in_node.in_loc().start_offset();
            let end = in_node.pattern().location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/EmptyInPattern",
                "Avoid `in` branches without a body.",
                Severity::Warning,
                start,
                end,
            ));
        }
        ruby_prism::visit_case_match_node(self, node);
    }
}

crate::register_cop!("Lint/EmptyInPattern", |cfg| {
    let allow_comments = cfg
        .get_cop_config("Lint/EmptyInPattern")
        .and_then(|c| c.raw.get("AllowComments"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(EmptyInPattern::new(allow_comments)))
});
