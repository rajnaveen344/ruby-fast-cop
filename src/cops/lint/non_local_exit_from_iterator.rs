//! Lint/NonLocalExitFromIterator - Checks for `return` without value inside iterator blocks.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Non-local exit from iterator, without return value. \
    `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.";

#[derive(Default)]
pub struct NonLocalExitFromIterator;

impl NonLocalExitFromIterator {
    pub fn new() -> Self { Self }
}

impl Cop for NonLocalExitFromIterator {
    fn name(&self) -> &'static str { "Lint/NonLocalExitFromIterator" }
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
    fn check_call_with_block(&mut self, call: &ruby_prism::CallNode) {
        let block = match call.block() {
            Some(b) => b,
            None => return,
        };
        let block_node = match block.as_block_node() {
            Some(b) => b,
            None => return,
        };

        // Skip if define_method / define_singleton_method
        let method = node_name!(call);
        if matches!(method.as_ref(), "define_method" | "define_singleton_method") {
            return;
        }

        // Must be chained (call has receiver)
        if call.receiver().is_none() {
            return;
        }

        // Check for explicit params or implicit (numbered/it) params
        let has_params = block_node.parameters().is_some();
        let has_implicit = if !has_params {
            block_node.body().map_or(false, |body| self.has_implicit_params(&body))
        } else {
            false
        };

        if !has_params && !has_implicit {
            return;
        }

        // Find bare returns in block body (not into nested defs/lambdas/blocks)
        if let Some(body) = block_node.body() {
            self.find_bare_returns(&body);
        }
    }

    fn has_implicit_params(&self, node: &Node) -> bool {
        match node {
            Node::ItLocalVariableReadNode { .. } => true,
            Node::LocalVariableReadNode { .. } => {
                let n = node.as_local_variable_read_node().unwrap();
                let name = String::from_utf8_lossy(n.name().as_slice());
                (name.starts_with('_') && name.len() > 1 && name[1..].parse::<u32>().is_ok())
                    || name.as_ref() == "it"
            }
            // Stop at scope barriers
            Node::DefNode { .. } | Node::LambdaNode { .. } | Node::SingletonClassNode { .. } |
            Node::BlockNode { .. } => false,
            _ => self.has_implicit_params_in_stmts(node),
        }
    }

    fn has_implicit_params_in_stmts(&self, node: &Node) -> bool {
        // Check statements body
        if let Some(stmts) = node.as_statements_node() {
            return stmts.body().iter().any(|c| self.has_implicit_params(&c));
        }
        false
    }

    fn find_bare_returns(&mut self, node: &Node) {
        match node {
            Node::ReturnNode { .. } => {
                let ret = node.as_return_node().unwrap();
                // Only flag return without value
                let has_value = ret.arguments().map_or(false, |a| a.arguments().iter().count() > 0);
                if !has_value {
                    let start = ret.keyword_loc().start_offset();
                    let end = ret.keyword_loc().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Lint/NonLocalExitFromIterator",
                        MSG,
                        Severity::Warning,
                        start,
                        end,
                    ));
                }
            }
            // Stop at scope barriers
            Node::DefNode { .. } |
            Node::SingletonClassNode { .. } |
            Node::LambdaNode { .. } => {}
            Node::BlockNode { .. } => {
                // Recurse into nested blocks — they may contain `return` that scopes
                // to an outer qualifying block (RuboCop walks up through ancestors)
                let block = node.as_block_node().unwrap();
                if let Some(body) = block.body() {
                    self.find_bare_returns(&body);
                }
            }
            Node::CallNode { .. } => {
                // Recurse into the body of any nested method call with a block.
                // The `return` inside `item.with_lock do...end` is attributed to the
                // outermost qualifying iterator block.
                // Exception: define_method / define_singleton_method create a new method scope.
                let call = node.as_call_node().unwrap();
                let method = node_name!(call);
                if matches!(method.as_ref(), "define_method" | "define_singleton_method") {
                    return;
                }
                if let Some(block) = call.block() {
                    if let Some(block_node) = block.as_block_node() {
                        if let Some(body) = block_node.body() {
                            self.find_bare_returns(&body);
                        }
                    }
                }
            }
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for child in stmts.body().iter() {
                    self.find_bare_returns(&child);
                }
            }
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                if let Some(s) = n.statements() {
                    for c in s.body().iter() { self.find_bare_returns(&c); }
                }
                if let Some(sub) = n.subsequent() {
                    self.find_bare_returns(&sub);
                }
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                if let Some(s) = n.statements() {
                    for c in s.body().iter() { self.find_bare_returns(&c); }
                }
                if let Some(else_branch) = n.else_clause() {
                    if let Some(s) = else_branch.statements() {
                        for c in s.body().iter() { self.find_bare_returns(&c); }
                    }
                }
            }
            Node::ElseNode { .. } => {
                let n = node.as_else_node().unwrap();
                if let Some(s) = n.statements() {
                    for c in s.body().iter() { self.find_bare_returns(&c); }
                }
            }
            Node::BeginNode { .. } => {
                let n = node.as_begin_node().unwrap();
                if let Some(s) = n.statements() {
                    for c in s.body().iter() { self.find_bare_returns(&c); }
                }
            }
            Node::WhileNode { .. } => {
                let n = node.as_while_node().unwrap();
                if let Some(s) = n.statements() {
                    for c in s.body().iter() { self.find_bare_returns(&c); }
                }
            }
            Node::UntilNode { .. } => {
                let n = node.as_until_node().unwrap();
                if let Some(s) = n.statements() {
                    for c in s.body().iter() { self.find_bare_returns(&c); }
                }
            }
            _ => {}
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call_with_block(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/NonLocalExitFromIterator", |_cfg| Some(Box::new(NonLocalExitFromIterator::new())));
