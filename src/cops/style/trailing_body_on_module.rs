//! Style/TrailingBodyOnModule cop
//!
//! Checks for trailing code after the module definition line.

use crate::cops::{CheckContext, Cop};
use crate::cops::style::trailing_body_on_method_definition::trailing_body_correction;
use crate::offense::{Offense, Severity};
use ruby_prism::{ModuleNode, Node, Visit};

const COP_NAME: &str = "Style/TrailingBodyOnModule";
const MSG: &str = "Place the first line of module body on its own line.";

pub struct TrailingBodyOnModule {
    indent_width: usize,
}

impl Default for TrailingBodyOnModule {
    fn default() -> Self {
        Self { indent_width: 2 }
    }
}

impl TrailingBodyOnModule {
    pub fn new(indent_width: usize) -> Self {
        Self { indent_width }
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
            indent_width: self.indent_width,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct TrailingBodyOnModuleVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    indent_width: usize,
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

        let correction = trailing_body_correction(self.ctx, def_start, first_start, self.indent_width);
        let offense = self.ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, first_start, first_end);
        self.offenses.push(offense.with_correction(correction));
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

crate::register_cop!("Style/TrailingBodyOnModule", |cfg| {
    let indent_width = cfg
        .get_cop_config("Layout/IndentationWidth")
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_i64())
        .unwrap_or(2) as usize;
    Some(Box::new(TrailingBodyOnModule::new(indent_width)))
});
