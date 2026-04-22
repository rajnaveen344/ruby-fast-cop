//! Lint/Loop - Checks for `begin...end while/until` constructs.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct Loop;

impl Loop {
    pub fn new() -> Self { Self }
}

impl Cop for Loop {
    fn name(&self) -> &'static str { "Lint/Loop" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

const MSG: &str = "Use `Kernel#loop` with `break` rather than `begin/end/until`(or `while`).";

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn register_offense(&mut self, keyword_start: usize, keyword_end: usize,
                        is_while: bool, condition_src: &str,
                        begin_start: usize, begin_end: usize,
                        body_begin_start: usize, body_end_start: usize) {
        // `do` → `loop do`, remove `end while/until condition\n`
        // body_begin_start = offset of `begin` keyword, body_end_start = offset of `end` before while/until
        let src = self.ctx.source;

        // Indentation: find the indent before `begin`
        let line_start = src[..begin_start].rfind('\n').map_or(0, |p| p + 1);
        let indent = &src[line_start..begin_start];

        // Correction:
        // 1. Replace `begin` with `loop do`
        // 2. Insert break line before `end` (body_end_start)
        // 3. Remove `end while/until condition` part (from body_end_start+3 to end of line/newline after condition)
        let break_keyword = if is_while { "unless" } else { "if" };
        let break_line = format!("break {} {}\n{}", break_keyword, condition_src, indent);

        // Find the end of `end while condition` — from body_end_start to end of that line
        let after_end = body_end_start + 3; // len("end")
        // Find end of condition line
        let cond_line_end = src[keyword_end..].find('\n')
            .map_or(src.len(), |p| keyword_end + p + 1);

        let correction = Correction {
            edits: vec![
                // Replace `begin` with `loop do`
                Edit { start_offset: begin_start, end_offset: begin_start + 5, replacement: "loop do".to_string() },
                // Insert break line before `end` in ensure_body
                Edit { start_offset: body_end_start, end_offset: body_end_start, replacement: break_line },
                // Remove ` while/until condition\n` from after `end` to end of line
                Edit { start_offset: after_end, end_offset: cond_line_end, replacement: "\n".to_string() },
            ],
        };

        let mut offense = self.ctx.offense_with_range(
            "Lint/Loop",
            MSG,
            Severity::Warning,
            keyword_start,
            keyword_end,
        );
        offense.correction = Some(correction);
        self.offenses.push(offense);
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        // Only PostConditionWhileNode (begin...end while) is flagged
        if !node.is_begin_modifier() {
            ruby_prism::visit_while_node(self, node);
            return;
        }
        let kw = node.keyword_loc();
        let cond = node.predicate();
        let cond_src = &self.ctx.source[cond.location().start_offset()..cond.location().end_offset()];
        let body = node.statements();
        // body is the BeginNode
        let begin_start = node.location().start_offset();
        let begin_end = node.location().end_offset();
        // The `begin` keyword is at begin_start, the `end` before `while` is at keyword_start - spaces/newline
        // body_end_start: the offset of `end` in `end while ...`
        // In source: `end while test` — find `end` before keyword
        let src = self.ctx.source;
        let kw_start = kw.start_offset();
        // Find `end` right before `while` by scanning backwards
        let end_kw_start = {
            let before = &src[..kw_start];
            let trimmed = before.trim_end();
            trimmed.len().saturating_sub(3)
        };

        self.register_offense(
            kw_start, kw.end_offset(),
            true, cond_src,
            begin_start, begin_end,
            end_kw_start, end_kw_start,
        );
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if !node.is_begin_modifier() {
            ruby_prism::visit_until_node(self, node);
            return;
        }
        let kw = node.keyword_loc();
        let cond = node.predicate();
        let cond_src = &self.ctx.source[cond.location().start_offset()..cond.location().end_offset()];
        let src = self.ctx.source;
        let kw_start = kw.start_offset();
        let begin_start = node.location().start_offset();
        let begin_end = node.location().end_offset();
        let end_kw_start = {
            let before = &src[..kw_start];
            let trimmed = before.trim_end();
            trimmed.len().saturating_sub(3)
        };

        self.register_offense(
            kw_start, kw.end_offset(),
            false, cond_src,
            begin_start, begin_end,
            end_kw_start, end_kw_start,
        );
        ruby_prism::visit_until_node(self, node);
    }
}

crate::register_cop!("Lint/Loop", |_cfg| Some(Box::new(Loop::new())));
