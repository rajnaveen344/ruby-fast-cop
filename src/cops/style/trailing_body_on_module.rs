//! Style/TrailingBodyOnModule cop
//!
//! Checks for trailing code after the module definition line.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{ModuleNode, Node, Visit};

const COP_NAME: &str = "Style/TrailingBodyOnModule";
const MSG: &str = "Place the first line of module body on its own line.";

#[derive(Default)]
pub struct TrailingBodyOnModule;

impl TrailingBodyOnModule {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for TrailingBodyOnModule {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = TrailingBodyOnModuleVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct TrailingBodyOnModuleVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> TrailingBodyOnModuleVisitor<'a> {
    fn check_body(
        &mut self,
        def_start: usize,
        body: &Option<Node>,
        end_keyword_start: usize,
    ) {
        let body = match body {
            Some(b) => b,
            None => return,
        };

        let def_line = self.ctx.line_of(def_start);
        let end_line = self.ctx.line_of(end_keyword_start);
        if def_line == end_line {
            return;
        }

        let first = self.first_statement(body);
        let (first_start, first_end) = match first {
            Some(f) => f,
            None => return,
        };

        if self.ctx.line_of(first_start) != def_line {
            return;
        }

        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG,
            Severity::Convention,
            first_start,
            first_end,
        ));
    }

    fn first_statement(&self, body: &Node) -> Option<(usize, usize)> {
        match body {
            Node::StatementsNode { .. } => {
                let stmts = body.as_statements_node()?;
                let parts: Vec<_> = stmts.body().iter().collect();
                parts.first().map(|n| (n.location().start_offset(), n.location().end_offset()))
            }
            Node::BeginNode { .. } => {
                let begin = body.as_begin_node()?;
                let stmts = begin.statements()?;
                let parts: Vec<_> = stmts.body().iter().collect();
                parts.first().map(|n| (n.location().start_offset(), n.location().end_offset()))
            }
            _ => {
                let loc = body.location();
                Some((loc.start_offset(), loc.end_offset()))
            }
        }
    }
}

impl Visit<'_> for TrailingBodyOnModuleVisitor<'_> {
    fn visit_module_node(&mut self, node: &ModuleNode) {
        let end_start = node.end_keyword_loc().start_offset();
        self.check_body(node.location().start_offset(), &node.body(), end_start);
        ruby_prism::visit_module_node(self, node);
    }
}

crate::register_cop!("Style/TrailingBodyOnModule", |_cfg| {
    Some(Box::new(TrailingBodyOnModule::new()))
});
