//! Style/MultilineIfThen cop
//!
//! Checks for uses of the `then` keyword in multi-line `if`/`unless`/`elsif` statements.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Style/MultilineIfThen";

#[derive(Default)]
pub struct MultilineIfThen;

impl MultilineIfThen {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for MultilineIfThen {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MultilineIfThenVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct MultilineIfThenVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> MultilineIfThenVisitor<'a> {
    fn check_if(&mut self, node: &ruby_prism::IfNode) {
        let then_loc = match node.then_keyword_loc() {
            Some(t) => t,
            None => return,
        };

        let keyword_src = self.ctx.src(then_loc.start_offset(), then_loc.end_offset());
        if keyword_src != "then" {
            return;
        }

        // Check if this is multi-line: body (if_branch) must be on a different line than `then`
        let then_line = self.ctx.line_of(then_loc.start_offset());

        // Get the if_branch (statements)
        let branch_line = match node.statements() {
            Some(stmts) => {
                let parts: Vec<_> = stmts.body().iter().collect();
                match parts.first() {
                    Some(first) => self.ctx.line_of(first.location().start_offset()),
                    None => {
                        // Empty body — check if end is on different line
                        match node.end_keyword_loc() {
                            Some(end_loc) => self.ctx.line_of(end_loc.start_offset()),
                            None => return,
                        }
                    }
                }
            }
            None => {
                // No body — check end keyword
                match node.end_keyword_loc() {
                    Some(end_loc) => self.ctx.line_of(end_loc.start_offset()),
                    None => return,
                }
            }
        };

        if branch_line <= then_line {
            // Single-line use of `then` (same line) — allowed
            return;
        }

        // Determine keyword name (if/elsif/unless)
        let kw = self.keyword_of(node);
        let msg = format!("Do not use `then` for multi-line `{kw}`.");

        let offense = self.ctx.offense_with_range(
            COP_NAME,
            &msg,
            Severity::Convention,
            then_loc.start_offset(),
            then_loc.end_offset(),
        );
        self.offenses.push(offense);
    }

    fn check_unless(&mut self, node: &ruby_prism::UnlessNode) {
        let then_loc = match node.then_keyword_loc() {
            Some(t) => t,
            None => return,
        };

        let keyword_src = self.ctx.src(then_loc.start_offset(), then_loc.end_offset());
        if keyword_src != "then" {
            return;
        }

        let then_line = self.ctx.line_of(then_loc.start_offset());

        let branch_line = match node.statements() {
            Some(stmts) => {
                let parts: Vec<_> = stmts.body().iter().collect();
                match parts.first() {
                    Some(first) => self.ctx.line_of(first.location().start_offset()),
                    None => match node.end_keyword_loc() {
                        Some(end_loc) => self.ctx.line_of(end_loc.start_offset()),
                        None => return,
                    },
                }
            }
            None => match node.end_keyword_loc() {
                Some(end_loc) => self.ctx.line_of(end_loc.start_offset()),
                None => return,
            },
        };

        if branch_line <= then_line {
            return;
        }

        let msg = "Do not use `then` for multi-line `unless`.";
        let offense = self.ctx.offense_with_range(
            COP_NAME,
            msg,
            Severity::Convention,
            then_loc.start_offset(),
            then_loc.end_offset(),
        );
        self.offenses.push(offense);
    }

    fn keyword_of(&self, node: &ruby_prism::IfNode) -> &'static str {
        // Check if starts with "elsif"
        let start = node.location().start_offset();
        if self.ctx.source[start..].starts_with("elsif") {
            "elsif"
        } else {
            "if"
        }
    }
}

impl Visit<'_> for MultilineIfThenVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless(node);
        ruby_prism::visit_unless_node(self, node);
    }
}

crate::register_cop!("Style/MultilineIfThen", |_cfg| {
    Some(Box::new(MultilineIfThen::new()))
});
