//! Style/TrailingBodyOnMethodDefinition cop
//!
//! Checks for trailing code after the method definition line.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/TrailingBodyOnMethodDefinition";
const MSG: &str = "Place the first line of a multi-line method definition's body on its own line.";

#[derive(Default)]
pub struct TrailingBodyOnMethodDefinition;

impl TrailingBodyOnMethodDefinition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for TrailingBodyOnMethodDefinition {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = TrailingBodyVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct TrailingBodyVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> TrailingBodyVisitor<'a> {
    fn check_def(&mut self, def_start: usize, body: Option<Node>, end_loc: Option<ruby_prism::Location>) {
        let body = match body {
            Some(b) => b,
            None => return,
        };

        let end_keyword_loc = match end_loc {
            Some(e) => e,
            None => {
                // Endless method — skip
                return;
            }
        };

        // Get the first statement of the body
        let (first_start, first_end) = match self.first_statement_offsets(&body) {
            Some(s) => s,
            None => return,
        };

        // The def is multi-line if end keyword is on a different line from def
        let def_line = self.ctx.line_of(def_start);
        let end_line = self.ctx.line_of(end_keyword_loc.start_offset());

        if def_line == end_line {
            // Single-line method — no offense
            return;
        }

        // Check if the first statement is on the same line as def
        let first_stmt_line = self.ctx.line_of(first_start);
        if first_stmt_line != def_line {
            // Body already on its own line — no offense
            return;
        }

        // Trailing body on def line — flag the first statement
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention, first_start, first_end,
        ));
    }

    fn first_statement_offsets(&self, body: &Node) -> Option<(usize, usize)> {
        match body {
            Node::StatementsNode { .. } => {
                let stmts = body.as_statements_node().unwrap();
                let parts: Vec<_> = stmts.body().iter().collect();
                parts.first().map(|n| (n.location().start_offset(), n.location().end_offset()))
            }
            Node::BeginNode { .. } => {
                let begin = body.as_begin_node().unwrap();
                if let Some(stmts) = begin.statements() {
                    let parts: Vec<_> = stmts.body().iter().collect();
                    parts.first().map(|n| (n.location().start_offset(), n.location().end_offset()))
                } else {
                    None
                }
            }
            _ => {
                // Single expression body
                let loc = body.location();
                Some((loc.start_offset(), loc.end_offset()))
            }
        }
    }
}

impl Visit<'_> for TrailingBodyVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let def_start = node.location().start_offset();
        let body = node.body();
        let end_loc = node.end_keyword_loc();
        self.check_def(def_start, body, end_loc);
        ruby_prism::visit_def_node(self, node);
    }

}

crate::register_cop!("Style/TrailingBodyOnMethodDefinition", |_cfg| {
    Some(Box::new(TrailingBodyOnMethodDefinition::new()))
});
