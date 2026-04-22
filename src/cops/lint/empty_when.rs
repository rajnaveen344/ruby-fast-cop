//! Lint/EmptyWhen - Checks for `when` branches without a body.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct EmptyWhen {
    allow_comments: bool,
}

impl EmptyWhen {
    pub fn new(allow_comments: bool) -> Self { Self { allow_comments } }
}

impl Default for EmptyWhen {
    fn default() -> Self { Self::new(true) }
}

impl Cop for EmptyWhen {
    fn name(&self) -> &'static str { "Lint/EmptyWhen" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    cop: &'a EmptyWhen,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Check if there are any comment lines in the source range [start, end)
    fn has_comment_in_range(&self, start: usize, end: usize) -> bool {
        let src = self.ctx.source;
        let bytes = src.as_bytes();
        let mut pos = start;
        // Find newline first — comments must be on their own line or after `then`
        while pos < end && pos < bytes.len() {
            if bytes[pos] == b'#' {
                return true;
            }
            pos += 1;
        }
        false
    }

    fn check_when(&mut self, when_node: &ruby_prism::WhenNode, next_when_or_else_start: usize) {
        let has_body = when_node.statements().is_some();

        if has_body {
            return;
        }

        // Body is absent — check for comments if AllowComments is true
        if self.cop.allow_comments {
            // Check between when keyword end and next when/else/end
            let when_end = when_node.location().end_offset();
            if self.has_comment_in_range(when_end, next_when_or_else_start) {
                return;
            }
            // Also check inline after `then` keyword
            if let Some(then_loc) = when_node.then_keyword_loc() {
                let after_then = then_loc.end_offset();
                // Check to end of same line
                let eol = self.ctx.source[after_then..].find('\n')
                    .map_or(self.ctx.source.len(), |p| after_then + p);
                if self.has_comment_in_range(after_then, eol) {
                    return;
                }
            }
        }

        // Offense: the `when` keyword
        let start = when_node.location().start_offset();
        // Offense covers `when` up to and including the conditions (end of conditions list)
        let conditions: Vec<_> = when_node.conditions().iter().collect();
        let end = conditions.last()
            .map(|c| c.location().end_offset())
            .unwrap_or(start + 4);

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/EmptyWhen",
            "Avoid `when` branches without a body.",
            Severity::Warning,
            start,
            end,
        ));
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        let conditions: Vec<_> = node.conditions().iter().collect();
        let n = conditions.len();
        for (i, when) in conditions.iter().enumerate() {
            if let Some(when_node) = when.as_when_node() {
                // Next boundary: start of next when, or else clause, or case end
                let next_start = if i + 1 < n {
                    conditions[i + 1].location().start_offset()
                } else if let Some(else_clause) = node.else_clause() {
                    else_clause.location().start_offset()
                } else {
                    node.location().end_offset()
                };
                self.check_when(&when_node, next_start);
            }
        }
        ruby_prism::visit_case_node(self, node);
    }
}

crate::register_cop!("Lint/EmptyWhen", |cfg| {
    let allow_comments = cfg
        .get_cop_config("Lint/EmptyWhen")
        .and_then(|c| c.raw.get("AllowComments"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(EmptyWhen::new(allow_comments)))
});
