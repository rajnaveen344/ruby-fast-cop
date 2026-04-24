//! Lint/UnreachablePatternBranch - Checks for unreachable `in` pattern branches.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/unreachable_pattern_branch.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct UnreachablePatternBranch;

impl UnreachablePatternBranch {
    pub fn new() -> Self { Self }
}

impl Cop for UnreachablePatternBranch {
    fn name(&self) -> &'static str { "Lint/UnreachablePatternBranch" }
    fn severity(&self) -> Severity { Severity::Warning }

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

impl<'a> Visitor<'a> {
    /// If pattern is wrapped in a guard (IfNode/UnlessNode), returns true.
    fn has_guard(&self, pattern: &Node) -> bool {
        matches!(pattern, Node::IfNode { .. } | Node::UnlessNode { .. })
    }

    /// Unwrap guard wrapper to inner pattern.
    fn unwrap_guard<'b>(&self, pattern: Node<'b>) -> Node<'b> {
        if let Some(if_node) = pattern.as_if_node() {
            if let Some(stmts) = if_node.statements() {
                if let Some(inner) = stmts.body().iter().next() {
                    return inner;
                }
            }
        }
        if let Some(unless_node) = pattern.as_unless_node() {
            if let Some(stmts) = unless_node.statements() {
                if let Some(inner) = stmts.body().iter().next() {
                    return inner;
                }
            }
        }
        pattern
    }

    fn is_catch_all(&self, pattern: &Node) -> bool {
        match pattern {
            // `in x` - bare variable capture
            Node::LocalVariableTargetNode { .. } => true,
            // `in pattern => y` - capture pattern, catch-all if inner is catch-all
            Node::CapturePatternNode { .. } => {
                let cap = pattern.as_capture_pattern_node().unwrap();
                self.is_catch_all(&cap.value())
            }
            // `in (...)` - parentheses wrapper
            Node::ParenthesesNode { .. } => {
                let paren = pattern.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if let Some(stmts) = body.as_statements_node() {
                        if let Some(inner) = stmts.body().iter().next() {
                            return self.is_catch_all(&inner);
                        }
                    }
                    return self.is_catch_all(&body);
                }
                false
            }
            // `in a | b` - alternation, catch-all if any alternative is
            Node::AlternationPatternNode { .. } => {
                let alt = pattern.as_alternation_pattern_node().unwrap();
                let l = alt.left();
                let r = alt.right();
                self.is_catch_all(&l) || self.is_catch_all(&r)
            }
            _ => false,
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        let mut catch_all_found = false;
        let conds: Vec<_> = node.conditions().iter().collect();

        for cond in conds.iter() {
            let Some(in_node) = cond.as_in_node() else { continue };

            if catch_all_found {
                // Offense span: `in` kw .. pattern end
                let start = in_node.in_loc().start_offset();
                let end = in_node.pattern().location().end_offset();
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/UnreachablePatternBranch",
                    "Unreachable `in` pattern branch detected.",
                    Severity::Warning,
                    start,
                    end,
                ));
                continue;
            }

            let raw_pattern = in_node.pattern();
            let has_guard = self.has_guard(&raw_pattern);
            let inner = self.unwrap_guard(raw_pattern);
            if !has_guard && self.is_catch_all(&inner) {
                catch_all_found = true;
            }
        }

        if catch_all_found {
            if let Some(else_clause) = node.else_clause() {
                let kw = else_clause.else_keyword_loc();
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/UnreachablePatternBranch",
                    "Unreachable `else` branch detected.",
                    Severity::Warning,
                    kw.start_offset(),
                    kw.end_offset(),
                ));
            }
        }

        ruby_prism::visit_case_match_node(self, node);
    }
}

crate::register_cop!("Lint/UnreachablePatternBranch", |_cfg| {
    Some(Box::new(UnreachablePatternBranch::new()))
});
