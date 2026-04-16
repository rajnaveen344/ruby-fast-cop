//! Style/EmptyCaseCondition — flag `case` without a condition.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/empty_case_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/EmptyCaseCondition";
const MSG: &str = "Do not use empty `case` condition, instead use an `if` expression.";

#[derive(Default)]
pub struct EmptyCaseCondition;

impl EmptyCaseCondition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EmptyCaseCondition {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            parent_skips: false,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// True when the containing node is return/break/next/send,
    /// meaning a case-with-return inside cannot be converted safely.
    parent_skips: bool,
}

fn branch_has_return(stmts_opt: &Option<ruby_prism::StatementsNode>) -> bool {
    if let Some(stmts) = stmts_opt {
        for s in stmts.body().iter() {
            if contains_return(&s) {
                return true;
            }
        }
    }
    false
}

fn contains_return(node: &Node) -> bool {
    struct F {
        found: bool,
    }
    impl<'pr> Visit<'pr> for F {
        fn visit_return_node(&mut self, _n: &ruby_prism::ReturnNode<'pr>) {
            self.found = true;
        }
    }
    let mut f = F { found: false };
    f.visit(node);
    f.found
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_return_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_break_node(&mut self, node: &ruby_prism::BreakNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_break_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_next_node(&mut self, node: &ruby_prism::NextNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_next_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_call_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        if node.predicate().is_none() {
            // Check branches for `return`
            let mut any_return = false;
            for c in node.conditions().iter() {
                if let Some(when) = c.as_when_node() {
                    if branch_has_return(&when.statements()) {
                        any_return = true;
                        break;
                    }
                }
            }
            if !any_return {
                if let Some(else_clause) = node.else_clause() {
                    if branch_has_return(&else_clause.statements()) {
                        any_return = true;
                    }
                }
            }

            if !self.parent_skips && !any_return {
                let loc = node.case_keyword_loc();
                let start = loc.start_offset();
                let end = loc.end_offset();
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }

        // Reset parent_skips for body traversal — nested case without condition
        // inside a branch is independent.
        let saved = self.parent_skips;
        self.parent_skips = false;
        ruby_prism::visit_case_node(self, node);
        self.parent_skips = saved;
    }
}
