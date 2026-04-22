//! Lint/SuppressedException cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct SuppressedException {
    allow_comments: bool,
    allow_nil: bool,
}

impl SuppressedException {
    pub fn new(allow_comments: bool, allow_nil: bool) -> Self {
        Self { allow_comments, allow_nil }
    }
}

impl Default for SuppressedException {
    fn default() -> Self {
        Self::new(true, true)
    }
}

impl Cop for SuppressedException {
    fn name(&self) -> &'static str {
        "Lint/SuppressedException"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = SuppressedExceptionVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SuppressedExceptionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a SuppressedException,
    offenses: Vec<Offense>,
}

impl<'a> SuppressedExceptionVisitor<'a> {
    /// Check if there are any comment lines between rescue keyword and container end.
    fn has_comment_after_rescue(&self, rescue_start: usize, container_end: usize) -> bool {
        let src = self.ctx.source;
        let bytes = src.as_bytes();
        // Skip to end of rescue line
        let mut pos = rescue_start;
        while pos < bytes.len() && bytes[pos] != b'\n' {
            pos += 1;
        }
        // Scan lines until container_end
        while pos < container_end && pos < bytes.len() {
            if bytes[pos] == b'\n' {
                pos += 1;
                // skip leading whitespace
                while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                    pos += 1;
                }
                if pos < bytes.len() && bytes[pos] == b'#' {
                    return true;
                }
            } else {
                pos += 1;
            }
        }
        false
    }

    fn check_rescue_node(&mut self, node: &ruby_prism::RescueNode, container_end: usize) {
        let stmts: Vec<_> = node.statements().map_or_else(Vec::new, |s| s.body().iter().collect());
        let stmts_count = stmts.len();
        let is_nil_body = stmts_count == 1 && stmts[0].as_nil_node().is_some();
        let is_empty = stmts_count == 0;

        // If has real body (not nil, not empty), skip
        if !is_empty && !is_nil_body {
            return;
        }

        // AllowNil: allow if nil body
        if self.cop.allow_nil && is_nil_body {
            return;
        }

        // AllowComments: check for comment between rescue and end
        if self.cop.allow_comments {
            let rescue_start = node.location().start_offset();
            if self.has_comment_after_rescue(rescue_start, container_end) {
                return;
            }
        }

        // Offense range: `rescue` keyword through exception list / reference,
        // and any trailing `;` (matches RuboCop's resbody expression range).
        let start = node.location().start_offset();
        let mut end = start + 6;
        if let Some(r) = node.reference() {
            end = r.location().end_offset();
        } else if let Some(last_exc) = node.exceptions().iter().last() {
            end = last_exc.location().end_offset();
        }
        if self.ctx.source.as_bytes().get(end) == Some(&b';') {
            end += 1;
        }
        self.offenses.push(self.ctx.offense_with_range(
            "Lint/SuppressedException",
            "Do not suppress exceptions.",
            Severity::Warning,
            start,
            end,
        ));
    }

    fn visit_rescue_chain(&mut self, first: &ruby_prism::RescueNode, container_end: usize) {
        self.check_rescue_node(first, container_end);
        let mut next = first.subsequent();
        while let Some(next_rescue) = next {
            self.check_rescue_node(&next_rescue, container_end);
            next = next_rescue.subsequent();
        }
    }
}

impl<'a> Visit<'_> for SuppressedExceptionVisitor<'a> {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(rescue_clause) = node.rescue_clause() {
            let end_offset = node.location().end_offset();
            self.visit_rescue_chain(&rescue_clause, end_offset);
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // DefNode rescue is inside its body as a BeginNode — handled via visit_begin_node
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode) {
        // `expr rescue value` — check if value is nil
        let is_nil = node.rescue_expression().as_nil_node().is_some();

        if self.cop.allow_nil && is_nil {
            // allowed
        } else {
            let start = node.keyword_loc().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/SuppressedException",
                "Do not suppress exceptions.",
                Severity::Warning,
                start,
                end,
            ));
        }

        ruby_prism::visit_rescue_modifier_node(self, node);
    }
}

crate::register_cop!("Lint/SuppressedException", |cfg| {
    let allow_comments = cfg
        .get_cop_config("Lint/SuppressedException")
        .and_then(|c| c.raw.get("AllowComments"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let allow_nil = cfg
        .get_cop_config("Lint/SuppressedException")
        .and_then(|c| c.raw.get("AllowNil"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(SuppressedException::new(allow_comments, allow_nil)))
});
