//! Style/WhileUntilDo cop
//!
//! Checks for uses of `do` in multi-line `while/until` statements.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Visit, WhileNode, UntilNode};

#[derive(Default)]
pub struct WhileUntilDo;

impl WhileUntilDo {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for WhileUntilDo {
    fn name(&self) -> &'static str {
        "Style/WhileUntilDo"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = WhileUntilDoVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct WhileUntilDoVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> WhileUntilDoVisitor<'a> {
    fn check_while_like(&mut self, node_start: usize, cond_end: usize, do_loc: Option<ruby_prism::Location>, body_end: usize, keyword: &str) {
        let do_loc = match do_loc {
            Some(d) => d,
            None => return,
        };

        // Single-line: body ends on same line as keyword
        let kw_line = self.ctx.line_of(node_start);
        let body_end_line = self.ctx.line_of(body_end);
        if kw_line == body_end_line {
            // single-line while — skip
            return;
        }

        let msg = format!("Do not use `do` with multi-line `{}`.", keyword);
        self.offenses.push(self.ctx.offense_with_range(
            "Style/WhileUntilDo",
            &msg,
            Severity::Convention,
            do_loc.start_offset(),
            do_loc.end_offset(),
        ));
    }
}

impl Visit<'_> for WhileUntilDoVisitor<'_> {
    fn visit_while_node(&mut self, node: &WhileNode) {
        let do_loc = node.do_keyword_loc();
        let body_end = node.closing_loc()
            .map(|l| l.start_offset())
            .unwrap_or_else(|| node.location().end_offset());
        self.check_while_like(
            node.location().start_offset(),
            node.predicate().location().end_offset(),
            do_loc,
            body_end,
            "while",
        );
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &UntilNode) {
        let do_loc = node.do_keyword_loc();
        let body_end = node.closing_loc()
            .map(|l| l.start_offset())
            .unwrap_or_else(|| node.location().end_offset());
        self.check_while_like(
            node.location().start_offset(),
            node.predicate().location().end_offset(),
            do_loc,
            body_end,
            "until",
        );
        ruby_prism::visit_until_node(self, node);
    }
}

crate::register_cop!("Style/WhileUntilDo", |_cfg| {
    Some(Box::new(WhileUntilDo::new()))
});
