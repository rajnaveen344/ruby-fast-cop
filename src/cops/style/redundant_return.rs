//! Style/RedundantReturn - Checks for redundant `return` expressions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_return.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantReturn";
const MSG: &str = "Redundant `return` detected.";
const MULTI_SUFFIX: &str = " To return multiple values, use an array.";

pub struct RedundantReturn {
    allow_multiple_return_values: bool,
}

impl Default for RedundantReturn {
    fn default() -> Self {
        Self { allow_multiple_return_values: false }
    }
}

impl RedundantReturn {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_config(allow_multiple_return_values: bool) -> Self {
        Self { allow_multiple_return_values }
    }
}

impl Cop for RedundantReturn {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = RRVisitor { cop: self, ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct RRVisitor<'a> {
    cop: &'a RedundantReturn,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RRVisitor<'a> {
    fn check_def(&mut self, body: Option<Node>) {
        if let Some(b) = body {
            self.check_branch(&b);
        }
    }

    /// Recurse into branches like RuboCop's `check_branch`.
    fn check_branch(&mut self, node: &Node) {
        match node {
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                if let Some(last) = stmts.body().iter().last() {
                    self.check_branch(&last);
                }
            }
            Node::ReturnNode { .. } => {
                let ret = node.as_return_node().unwrap();
                self.check_return_node(&ret);
            }
            Node::CaseNode { .. } => {
                let case = node.as_case_node().unwrap();
                for cond in case.conditions().iter() {
                    if let Some(w) = cond.as_when_node() {
                        if let Some(stmts) = w.statements() {
                            if let Some(last) = stmts.body().iter().last() {
                                self.check_branch(&last);
                            }
                        }
                    }
                }
                if let Some(else_clause) = case.else_clause() {
                    if let Some(stmts) = else_clause.statements() {
                        if let Some(last) = stmts.body().iter().last() {
                            self.check_branch(&last);
                        }
                    }
                }
            }
            Node::CaseMatchNode { .. } => {
                let case = node.as_case_match_node().unwrap();
                for cond in case.conditions().iter() {
                    if let Some(in_n) = cond.as_in_node() {
                        if let Some(stmts) = in_n.statements() {
                            if let Some(last) = stmts.body().iter().last() {
                                self.check_branch(&last);
                            }
                        }
                    }
                }
                if let Some(else_clause) = case.else_clause() {
                    if let Some(stmts) = else_clause.statements() {
                        if let Some(last) = stmts.body().iter().last() {
                            self.check_branch(&last);
                        }
                    }
                }
            }
            Node::IfNode { .. } => {
                let ifn = node.as_if_node().unwrap();
                // Skip ternary only (no if_keyword). Modifier-if is OK.
                if ifn.if_keyword_loc().is_none() {
                    return;
                }
                if let Some(stmts) = ifn.statements() {
                    if let Some(last) = stmts.body().iter().last() {
                        self.check_branch(&last);
                    }
                }
                if let Some(sub) = ifn.subsequent() {
                    self.check_branch(&sub);
                }
            }
            Node::ElseNode { .. } => {
                let else_node = node.as_else_node().unwrap();
                if let Some(stmts) = else_node.statements() {
                    if let Some(last) = stmts.body().iter().last() {
                        self.check_branch(&last);
                    }
                }
            }
            Node::BeginNode { .. } => {
                let begin = node.as_begin_node().unwrap();
                if begin.rescue_clause().is_some() {
                    // check_rescue_node: each branch + else (or body if no else)
                    let mut cur = begin.rescue_clause();
                    while let Some(resc) = cur {
                        if let Some(stmts) = resc.statements() {
                            if let Some(last) = stmts.body().iter().last() {
                                self.check_branch(&last);
                            }
                        }
                        cur = resc.subsequent();
                    }
                    if let Some(else_clause) = begin.else_clause() {
                        if let Some(stmts) = else_clause.statements() {
                            if let Some(last) = stmts.body().iter().last() {
                                self.check_branch(&last);
                            }
                        }
                    } else if let Some(stmts) = begin.statements() {
                        if let Some(last) = stmts.body().iter().last() {
                            self.check_branch(&last);
                        }
                    }
                    return;
                }
                // Plain begin...end or begin with ensure only
                if let Some(stmts) = begin.statements() {
                    if let Some(last) = stmts.body().iter().last() {
                        self.check_branch(&last);
                    }
                }
            }
            _ => {}
        }
    }

    fn check_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        let arg_count = node.arguments().map_or(0, |a| a.arguments().iter().count());

        if self.cop.allow_multiple_return_values && arg_count > 1 {
            return;
        }

        let msg = if !self.cop.allow_multiple_return_values && arg_count > 1 {
            format!("{}{}", MSG, MULTI_SUFFIX)
        } else {
            MSG.to_string()
        };

        let kw = node.keyword_loc();
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &msg, Severity::Convention, kw.start_offset(), kw.end_offset(),
        ));
    }
}

impl<'a> Visit<'_> for RRVisitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.check_def(node.body());
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        // Stabby-lambda: `-> do ... end` / `-> { ... }`
        if let Some(body) = node.body() {
            self.check_branch(&body);
        }
        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // define_method / define_singleton_method / lambda with block literal
        let method = node_name!(node);
        if matches!(method.as_ref(), "define_method" | "define_singleton_method" | "lambda") {
            if let Some(block) = node.block() {
                if let Some(bn) = block.as_block_node() {
                    if let Some(body) = bn.body() {
                        self.check_branch(&body);
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { allow_multiple_return_values: bool }

crate::register_cop!("Style/RedundantReturn", |cfg| {
    let c: Cfg = cfg.typed("Style/RedundantReturn");
    Some(Box::new(RedundantReturn::with_config(c.allow_multiple_return_values)))
});
