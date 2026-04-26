//! Layout/EmptyLineAfterMultilineCondition - empty line required after multiline condition.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/empty_line_after_multiline_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Use empty line after multiline condition.";
const COP_NAME: &str = "Layout/EmptyLineAfterMultilineCondition";

#[derive(Default)]
pub struct EmptyLineAfterMultilineCondition;

impl EmptyLineAfterMultilineCondition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EmptyLineAfterMultilineCondition {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
            sibling_stack: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    sibling_stack: Vec<bool>,
}

impl<'a> Visitor<'a> {
    fn line_of(&self, offset: usize) -> usize {
        self.ctx.line_of(offset)
    }

    fn check_condition(&mut self, cond_start: usize, cond_end: usize) {
        let first_line = self.line_of(cond_start);
        let last_line = self.line_of(cond_end.saturating_sub(1));
        if first_line >= last_line {
            return;
        }
        if self.next_line_blank(last_line) {
            return;
        }
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG,
            Severity::Convention,
            cond_start,
            cond_end,
        ));
    }

    fn next_line_blank(&self, line_1based: usize) -> bool {
        let bytes = self.ctx.source.as_bytes();
        let mut current_line = 1usize;
        let mut i = 0;
        while i < bytes.len() && current_line < line_1based {
            if bytes[i] == b'\n' {
                current_line += 1;
            }
            i += 1;
        }
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        if i >= bytes.len() {
            return true;
        }
        i += 1;
        while i < bytes.len() && bytes[i] != b'\n' {
            if bytes[i] != b' ' && bytes[i] != b'\t' && bytes[i] != b'\r' {
                return false;
            }
            i += 1;
        }
        true
    }

    fn current_has_right_sibling(&self) -> bool {
        *self.sibling_stack.last().unwrap_or(&false)
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode) {
        let body: Vec<_> = node.body().iter().collect();
        let n = body.len();
        for (i, child) in body.iter().enumerate() {
            self.sibling_stack.push(i + 1 < n);
            self.visit(child);
            self.sibling_stack.pop();
        }
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if node.if_keyword_loc().is_none() {
            ruby_prism::visit_if_node(self, node);
            return;
        }
        let is_modifier = node.end_keyword_loc().is_none();
        let cond = node.predicate();
        let should_check = if is_modifier {
            self.current_has_right_sibling()
        } else {
            true
        };
        if should_check {
            let s = cond.location().start_offset();
            let e = cond.location().end_offset();
            self.check_condition(s, e);
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let is_modifier = node.end_keyword_loc().is_none();
        let cond = node.predicate();
        let should_check = if is_modifier {
            self.current_has_right_sibling()
        } else {
            true
        };
        if should_check {
            let s = cond.location().start_offset();
            let e = cond.location().end_offset();
            self.check_condition(s, e);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let body_start = node.statements().map(|s| s.location().start_offset());
        let kw = node.keyword_loc();
        let cond = node.predicate();
        let body_before_kw = body_start.map(|s| s < kw.start_offset()).unwrap_or(false);
        let should_check = if body_before_kw {
            self.current_has_right_sibling()
        } else {
            true
        };
        if should_check {
            self.check_condition(cond.location().start_offset(), cond.location().end_offset());
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let body_start = node.statements().map(|s| s.location().start_offset());
        let kw = node.keyword_loc();
        let cond = node.predicate();
        let body_before_kw = body_start.map(|s| s < kw.start_offset()).unwrap_or(false);
        let should_check = if body_before_kw {
            self.current_has_right_sibling()
        } else {
            true
        };
        if should_check {
            self.check_condition(cond.location().start_offset(), cond.location().end_offset());
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        for cond in node.conditions().iter() {
            if let Some(when_node) = cond.as_when_node() {
                let conds: Vec<_> = when_node.conditions().iter().collect();
                if conds.is_empty() {
                    continue;
                }
                let first = &conds[0];
                let last = &conds[conds.len() - 1];
                let first_line = self.line_of(first.location().start_offset());
                let last_line = self.line_of(last.location().end_offset().saturating_sub(1));
                if first_line >= last_line {
                    continue;
                }
                if self.next_line_blank(last_line) {
                    continue;
                }
                let when_kw = when_node.keyword_loc();
                let start = when_kw.start_offset();
                let end = last.location().end_offset();
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        let exceptions: Vec<Node> = node.exceptions().iter().collect();
        if exceptions.len() > 1 {
            let first = &exceptions[0];
            let last = &exceptions[exceptions.len() - 1];
            let first_line = self.line_of(first.location().start_offset());
            let last_line = self.line_of(last.location().end_offset().saturating_sub(1));
            if first_line < last_line && !self.next_line_blank(last_line) {
                let kw = node.keyword_loc();
                let start = kw.start_offset();
                let end = last.location().end_offset();
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }
        ruby_prism::visit_rescue_node(self, node);
    }
}

crate::register_cop!("Layout/EmptyLineAfterMultilineCondition", |_cfg| Some(
    Box::new(EmptyLineAfterMultilineCondition::new())
));
