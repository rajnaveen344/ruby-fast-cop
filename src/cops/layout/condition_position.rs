//! Layout/ConditionPosition - Checks that conditions are on the same line as if/while/until.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/condition_position.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_at_offset;
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

pub struct ConditionPosition;

impl Default for ConditionPosition {
    fn default() -> Self {
        Self
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Check that keyword and condition are on the same line.
    /// keyword_end: byte offset right after the keyword (e.g. after "if")
    /// condition: the condition node
    fn check_condition(
        &mut self,
        keyword_loc: ruby_prism::Location<'a>,
        condition: ruby_prism::Node<'a>,
    ) {
        let source = self.ctx.source;
        let kw_line = line_at_offset(source, keyword_loc.start_offset());
        let cond_line = line_at_offset(source, condition.location().start_offset());

        if kw_line == cond_line {
            return; // same line, OK
        }

        // Get keyword name from source
        let kw_text = &source[keyword_loc.start_offset()..keyword_loc.end_offset()];

        let msg = format!("Place the condition on the same line as `{kw_text}`.");
        let cond_start = condition.location().start_offset();
        let cond_end = condition.location().end_offset();

        // Correction: move condition to end of keyword line
        // Remove from condition's current position to end of its line
        // Insert " <condition_text>" after keyword
        let correction = build_correction(source, keyword_loc.end_offset(), cond_start, cond_end);

        self.offenses.push(
            Offense::new(
                "Layout/ConditionPosition",
                &msg,
                Severity::Convention,
                Location::from_offsets(source, cond_start, cond_end),
                self.ctx.filename,
            ).with_correction(correction)
        );
    }
}

/// Build correction: delete the whitespace/newline between keyword and condition,
/// and insert the condition right after keyword with a space.
fn build_correction(
    source: &str,
    kw_end: usize,
    cond_start: usize,
    cond_end: usize,
) -> Correction {
    let cond_text = source[cond_start..cond_end].to_string();
    // Delete from kw_end to cond_end (removes newline + possible indent + condition)
    // Then insert " <cond_text>" at kw_end
    use crate::offense::Edit;
    Correction {
        edits: vec![
            Edit { start_offset: kw_end, end_offset: cond_end, replacement: format!(" {cond_text}") },
        ],
    }
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'a>) {
        // Skip modifier form (no keyword loc = ternary; or predicate keyword)
        // IfNode: if_keyword_loc() is Option — None for ternary
        if let Some(kw_loc) = node.if_keyword_loc() {
            let kw_text = &self.ctx.source[kw_loc.start_offset()..kw_loc.end_offset()];
            // Modifier form: `x if condition` — the keyword comes AFTER the body
            // For modifiers, keyword line > condition line... actually in modifier,
            // the whole thing is on one line anyway. Skip modifier check via:
            // if keyword_loc and condition are both on same line → OK already checked above.
            // The issue: "do_something if\n  cond" → modifier at kw_line, cond on next line
            // RuboCop DOES NOT flag modifiers. How to detect: IfNode with then_body = nil?
            // Actually: modifier `if` in Prism appears as an IfNode where predicate comes first.
            // Check: in `x if cond`, the 'if' keyword comes after x. For modifier check:
            // the statement is `do_something if cond`. In `if\ncond\nend` the keyword has no then.
            // We can detect modifier by checking if the keyword `if` or `unless` starts the line.
            // A cleaner way: if predicate (cond) comes BEFORE the if keyword offset → modifier.
            if kw_text == "if" || kw_text == "elsif" {
                let cond = node.predicate();
                let cond_start = cond.location().start_offset();
                // Modifier: condition is AFTER keyword in source? No: `x if cond` → cond after if.
                // But the issue is: for modifier `do_something if\n  cond` — should NOT flag.
                // RuboCop skips modifier forms explicitly. In Prism, modifier if:
                // the keyword_loc appears in the middle of an expression line.
                // Detection: for modifiers, the IfNode has no `end_keyword_loc`.
                // Prism's IfNode has `end_keyword_loc()` returning Option.
                // Modifier `body if cond` has no end_keyword_loc.
                let is_modifier = node.end_keyword_loc().is_none();
                if !is_modifier {
                    self.check_condition(kw_loc, cond);
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'a>) {
        let kw_loc = node.keyword_loc();
        let is_modifier = node.end_keyword_loc().is_none();
        if !is_modifier {
            self.check_condition(kw_loc, node.predicate());
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'a>) {
        // modifier: flags attribute?
        // Prism WhileNode has `keyword_loc()` (not Option)
        // Check modifier: in `do_something while cond`, condition comes after keyword.
        // The begin_modifier flag... use `node.flags()` — WhileNode has a "begin_modifier" flag.
        // Actually let's check: is the keyword the start of the statement?
        let kw_loc = node.keyword_loc();
        // Modifier detection: in Prism, while/until have a BEGIN_MODIFIER flag
        // We check: does keyword appear before the body?
        let body_start = node.statements().map(|s| s.location().start_offset());
        let cond_start = node.predicate().location().start_offset();
        let kw_end = kw_loc.end_offset();
        // Non-modifier: keyword comes first (kw_start < cond_start and kw_start < body)
        // Modifier: body comes first
        let is_modifier = body_start.map(|bs| bs < kw_loc.start_offset()).unwrap_or(false)
            || cond_start < kw_loc.start_offset();
        // Actually for modifier `x while y`, we have x before `while` before y.
        // For normal `while y\nx\nend`, we have `while` before y before x.
        // Simplest: check if cond comes after kw_end
        let is_modifier = cond_start > kw_end;
        // wait, both forms have cond after keyword. Let me use source structure:
        // modifier: the keyword is NOT at start of line (has code before it)
        let kw_line_start = {
            let src = self.ctx.source;
            src[..kw_loc.start_offset()].rfind('\n').map_or(0, |p| p + 1)
        };
        let code_before_kw = self.ctx.source[kw_line_start..kw_loc.start_offset()].trim();
        let is_modifier = !code_before_kw.is_empty();
        if !is_modifier {
            self.check_condition(kw_loc, node.predicate());
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'a>) {
        let kw_loc = node.keyword_loc();
        let kw_line_start = {
            let src = self.ctx.source;
            src[..kw_loc.start_offset()].rfind('\n').map_or(0, |p| p + 1)
        };
        let code_before_kw = self.ctx.source[kw_line_start..kw_loc.start_offset()].trim();
        let is_modifier = !code_before_kw.is_empty();
        if !is_modifier {
            self.check_condition(kw_loc, node.predicate());
        }
        ruby_prism::visit_until_node(self, node);
    }
}

impl Cop for ConditionPosition {
    fn name(&self) -> &'static str {
        "Layout/ConditionPosition"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

crate::register_cop!("Layout/ConditionPosition", |_cfg| {
    Some(Box::new(ConditionPosition))
});
