//! Style/HashLikeCase cop
//!
//! Checks for case/when that maps literals 1:1 and could be a hash lookup.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{CaseNode, Node, Visit};

pub struct HashLikeCase {
    min_branches_count: usize,
}

impl HashLikeCase {
    pub fn new(min_branches_count: usize) -> Self {
        Self { min_branches_count }
    }
}

impl Default for HashLikeCase {
    fn default() -> Self {
        Self::new(3)
    }
}

impl Cop for HashLikeCase {
    fn name(&self) -> &'static str {
        "Style/HashLikeCase"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = HashLikeCaseVisitor {
            ctx,
            min_branches_count: self.min_branches_count,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct HashLikeCaseVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    min_branches_count: usize,
    offenses: Vec<Offense>,
}

fn is_basic_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::StringNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::SymbolNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
    )
}

fn literal_kind(node: &Node) -> u8 {
    match node {
        Node::StringNode { .. } => 1,
        Node::SymbolNode { .. } => 2,
        Node::IntegerNode { .. } => 3,
        Node::FloatNode { .. } => 4,
        Node::TrueNode { .. } | Node::FalseNode { .. } => 5,
        Node::NilNode { .. } => 6,
        _ => 0,
    }
}

impl<'a> HashLikeCaseVisitor<'a> {
    fn check_case(&mut self, node: &CaseNode) {
        // Must have a subject (case x)
        if node.predicate().is_none() {
            return;
        }

        // Must have no else clause
        if node.else_clause().is_some() {
            return;
        }

        let conditions = node.conditions();
        let when_nodes: Vec<_> = conditions.iter().collect();

        // Min branches
        if when_nodes.len() < self.min_branches_count {
            return;
        }

        let mut cond_kinds: Vec<u8> = Vec::new();
        let mut body_kinds: Vec<u8> = Vec::new();

        for when_node in &when_nodes {
            let w = match when_node.as_when_node() {
                Some(w) => w,
                None => return,
            };

            // Each when must have exactly one condition that is a str or sym
            let conds: Vec<_> = w.conditions().iter().collect();
            if conds.len() != 1 {
                return;
            }
            let cond = &conds[0];
            if !matches!(cond, Node::StringNode { .. } | Node::SymbolNode { .. }) {
                return;
            }
            cond_kinds.push(literal_kind(cond));

            // Body must be a single basic literal
            let body = match w.statements() {
                Some(s) => s,
                None => return,
            };
            let stmts: Vec<_> = body.body().iter().collect();
            if stmts.len() != 1 {
                return;
            }
            let stmt = &stmts[0];
            if !is_basic_literal(stmt) {
                return;
            }
            body_kinds.push(literal_kind(stmt));
        }

        // All conditions must be same type, all bodies same type
        if cond_kinds.windows(2).any(|w| w[0] != w[1]) {
            return;
        }
        if body_kinds.windows(2).any(|w| w[0] != w[1]) {
            return;
        }

        // RuboCop flags `case` keyword + predicate: from start of `case` to end of predicate
        let case_start = node.location().start_offset();
        let case_end = node.predicate()
            .map(|p| p.location().end_offset())
            .unwrap_or(case_start + 4);
        self.offenses.push(self.ctx.offense_with_range(
            "Style/HashLikeCase",
            "Consider replacing `case-when` with a hash lookup.",
            Severity::Convention,
            case_start,
            case_end,
        ));
    }
}

impl Visit<'_> for HashLikeCaseVisitor<'_> {
    fn visit_case_node(&mut self, node: &CaseNode) {
        self.check_case(node);
        ruby_prism::visit_case_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    min_branches_count: Option<usize>,
}

crate::register_cop!("Style/HashLikeCase", |cfg| {
    let c: Cfg = cfg.typed("Style/HashLikeCase");
    let min = c.min_branches_count.unwrap_or(3);
    Some(Box::new(HashLikeCase::new(min)))
});
