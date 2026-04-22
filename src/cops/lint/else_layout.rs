//! Lint/ElseLayout - Odd else layout detection.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/else_layout.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct ElseLayout;

impl ElseLayout {
    pub fn new() -> Self {
        Self
    }
}

struct ElseLayoutVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl ElseLayoutVisitor<'_> {
    /// Check an if/unless node for improper else layout.
    fn check_if_node(&mut self, node: &ruby_prism::IfNode) {
        let if_start = node.location().start_offset();
        let if_end = node.location().end_offset();

        // Ternary / single-line: entire if on one line — skip
        if self.ctx.line_of(if_start) == self.ctx.line_of(if_end.saturating_sub(1)) {
            return;
        }

        // Get subsequent (else or elsif)
        let subsequent = node.subsequent();
        if let Some(Node::ElseNode { .. }) = &subsequent {
            let else_node = subsequent.as_ref().unwrap().as_else_node().unwrap();
            self.check_else_node(&else_node, if_start);
        }
    }

    fn check_else_node(&mut self, else_node: &ruby_prism::ElseNode, if_start: usize) {
        let stmts = match else_node.statements() {
            Some(s) => s,
            None => return, // empty else — fine
        };

        let body: Vec<Node> = stmts.body().iter().collect();
        if body.is_empty() {
            return;
        }

        let else_kw_loc = else_node.else_keyword_loc();
        let else_kw_start = else_kw_loc.start_offset();
        let else_kw_end = else_kw_loc.end_offset(); // right after "else"

        let else_line = self.ctx.line_of(else_kw_start);

        // First body element
        let first_body = &body[0];
        let body_start = first_body.location().start_offset();
        let body_line = self.ctx.line_of(body_start);

        if body_line != else_line {
            // body on next line — OK
            return;
        }

        // Body is on same line as `else`.
        // Check: is there a `then` keyword? (modifier form: `if cond then expr\nelse single\nend`)
        // We detect `then` by checking if the if-node source before else_kw contains " then "
        let src = self.ctx.source;
        let if_src = &src[if_start..else_kw_start];
        let has_then = if_src.contains(" then ");

        // If has_then AND entire else body fits on one line and body is single expression → OK
        if has_then && body.len() == 1 {
            // Single-line else with `then` is allowed
            let body_end = first_body.location().end_offset();
            // Check nothing follows body on the same line (i.e., body_end is at/near newline)
            let body_end_line = self.ctx.line_of(body_end.max(1) - 1);
            if body_end_line == else_line {
                // Check if there are more statements after this line
                // Actually: if body.len() == 1 and the body ends on else_line → OK
                return;
            }
        }

        // Offense: body starts on same line as `else`
        // RuboCop offense range = body node (starts at body_start)
        let body_end = first_body.location().end_offset();

        // Indentation of the if keyword for computing proper else body indent
        let indent = self.ctx.indentation_of(if_start);
        let indent_str = " ".repeat(indent + 2);
        let replacement = format!("\n{}", indent_str);

        // Replace from right after "else" to right before body (removes the space between them)
        let correction = Correction::replace(else_kw_end, body_start, &replacement);

        // Offense range: from body_start to body_end (the statement on the else line)
        let offense = self.ctx.offense_with_range(
            "Lint/ElseLayout",
            "Odd `else` layout detected. Did you mean to use `elsif`?",
            Severity::Warning,
            body_start,
            body_end,
        );
        self.offenses.push(offense.with_correction(correction));
    }
}

impl Visit<'_> for ElseLayoutVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if_node(node);
        ruby_prism::visit_if_node(self, node);
    }
}

impl Cop for ElseLayout {
    fn name(&self) -> &'static str {
        "Lint/ElseLayout"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ElseLayoutVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/ElseLayout", |_cfg| {
    Some(Box::new(ElseLayout::new()))
});
