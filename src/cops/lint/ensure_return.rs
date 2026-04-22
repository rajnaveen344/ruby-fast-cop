//! Lint/EnsureReturn - Checks for `return` inside `ensure` blocks.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct EnsureReturn;

impl EnsureReturn {
    pub fn new() -> Self { Self }
}

impl Cop for EnsureReturn {
    fn name(&self) -> &'static str { "Lint/EnsureReturn" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn find_bare_returns(&mut self, node: &Node) {
        match node {
            Node::ReturnNode { .. } => {
                let ret = node.as_return_node().unwrap();
                let start = ret.keyword_loc().start_offset();
                let end = ret.location().end_offset();
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/EnsureReturn",
                    "Do not return from an `ensure` block.",
                    Severity::Warning,
                    start,
                    end,
                ));
            }
            // Scope barriers — stop
            Node::DefNode { .. } | Node::SingletonClassNode { .. } | Node::LambdaNode { .. } => {}
            Node::StatementsNode { .. } => {
                let n = node.as_statements_node().unwrap();
                for child in n.body().iter() { self.find_bare_returns(&child); }
            }
            Node::BeginNode { .. } => {
                let n = node.as_begin_node().unwrap();
                if let Some(s) = n.statements() {
                    for child in s.body().iter() { self.find_bare_returns(&child); }
                }
            }
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                if let Some(s) = n.statements() {
                    for child in s.body().iter() { self.find_bare_returns(&child); }
                }
                if let Some(sub) = n.subsequent() {
                    self.find_bare_returns(&sub);
                }
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                if let Some(s) = n.statements() {
                    for child in s.body().iter() { self.find_bare_returns(&child); }
                }
                if let Some(else_branch) = n.else_clause() {
                    if let Some(s) = else_branch.statements() {
                        for child in s.body().iter() { self.find_bare_returns(&child); }
                    }
                }
            }
            Node::ElseNode { .. } => {
                let n = node.as_else_node().unwrap();
                if let Some(s) = n.statements() {
                    for child in s.body().iter() { self.find_bare_returns(&child); }
                }
            }
            _ => {}
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode) {
        if let Some(stmts) = node.statements() {
            for child in stmts.body().iter() {
                self.find_bare_returns(&child);
            }
        }
        ruby_prism::visit_ensure_node(self, node);
    }
}

crate::register_cop!("Lint/EnsureReturn", |_cfg| Some(Box::new(EnsureReturn::new())));
