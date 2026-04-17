//! Lint/LiteralAsCondition cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

pub struct LiteralAsCondition;

impl LiteralAsCondition {
    pub fn new() -> Self { Self }
}

impl Cop for LiteralAsCondition {
    fn name(&self) -> &'static str { "Lint/LiteralAsCondition" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = LiteralConditionVisitor { ctx, offenses: Vec::new(), reported: HashSet::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct LiteralConditionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    reported: HashSet<usize>,
}

impl<'a> LiteralConditionVisitor<'a> {
    fn check_condition(&mut self, condition: &Node) {
        match condition {
            Node::AndNode { .. } => self.check_condition(&condition.as_and_node().unwrap().left()),
            Node::OrNode { .. } => self.check_condition(&condition.as_or_node().unwrap().left()),
            Node::CallNode { .. } => {
                let call = condition.as_call_node().unwrap();
                if node_name!(call) == "!" {
                    if let Some(recv) = call.receiver() { self.check_condition(&recv); }
                } else if is_literal(condition) {
                    self.add_offense(condition);
                }
            }
            Node::ParenthesesNode { .. } => {
                if let Some(body) = condition.as_parentheses_node().unwrap().body() {
                    if let Node::StatementsNode { .. } = &body {
                        let stmts: Vec<_> = body.as_statements_node().unwrap().body().iter().collect();
                        if stmts.len() == 1 { self.check_condition(&stmts[0]); }
                    }
                }
            }
            _ => { if is_literal(condition) { self.add_offense(condition); } }
        }
    }

    fn add_offense(&mut self, node: &Node) {
        let loc = node.location();
        if !self.reported.insert(loc.start_offset()) { return; }
        self.offenses.push(self.ctx.offense_with_range(
            "Lint/LiteralAsCondition",
            &format!("Literal `{}` appeared as a condition.", &self.ctx.source[loc.start_offset()..loc.end_offset()]),
            Severity::Warning, loc.start_offset(), loc.end_offset(),
        ));
    }
}

impl Visit<'_> for LiteralConditionVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_condition(&node.predicate());
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_condition(&node.predicate());
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let pred = node.predicate();
        if !matches!(&pred, Node::TrueNode { .. }) { self.check_condition(&pred); }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let pred = node.predicate();
        if !matches!(&pred, Node::FalseNode { .. }) { self.check_condition(&pred); }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if let Some(predicate) = node.predicate() {
            if is_literal(&predicate) { self.add_offense(&predicate); }
        } else {
            for cond in node.conditions().iter() {
                if let Node::WhenNode { .. } = &cond {
                    for wc in cond.as_when_node().unwrap().conditions().iter() {
                        if is_literal(&wc) { self.add_offense(&wc); }
                    }
                }
            }
        }
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        if let Some(predicate) = node.predicate() {
            if is_literal(&predicate) && !has_match_var_pattern(node) {
                self.add_offense(&predicate);
            }
        }
        ruby_prism::visit_case_match_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if node_name!(node) == "!" {
            if let Some(recv) = node.receiver() {
                if is_literal(&recv) { self.add_offense(&recv); }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let left = node.left();
        if is_literal(&left) { self.add_offense(&left); }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let left = node.left();
        if is_literal(&left) { self.add_offense(&left); }
        ruby_prism::visit_or_node(self, node);
    }
}

fn is_literal(node: &Node) -> bool {
    match node {
        Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. } | Node::StringNode { .. } | Node::SymbolNode { .. }
        | Node::RegularExpressionNode { .. } | Node::TrueNode { .. } | Node::FalseNode { .. }
        | Node::NilNode { .. } | Node::SourceLineNode { .. } | Node::SourceFileNode { .. }
        | Node::SourceEncodingNode { .. } | Node::RangeNode { .. }
        | Node::InterpolatedSymbolNode { .. } => true,
        Node::InterpolatedStringNode { .. } | Node::InterpolatedRegularExpressionNode { .. } => false,
        Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter().all(|e| is_literal(&e)),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().all(|e| {
            e.as_assoc_node().map_or(false, |a| is_literal(&a.key()) && is_literal(&a.value()))
        }),
        _ => false,
    }
}

fn has_match_var_pattern(case_match: &ruby_prism::CaseMatchNode) -> bool {
    case_match.conditions().iter().any(|cond| {
        matches!(&cond, Node::InNode { .. }) && pattern_has_match_var(&cond.as_in_node().unwrap().pattern())
    })
}

fn pattern_has_match_var(pattern: &Node) -> bool {
    match pattern {
        Node::LocalVariableTargetNode { .. } | Node::CapturePatternNode { .. } => true,
        Node::PinnedVariableNode { .. } | Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => false,
        Node::ArrayPatternNode { .. } => {
            let arr = pattern.as_array_pattern_node().unwrap();
            arr.requireds().iter().any(|r| pattern_has_match_var(&r))
                || arr.posts().iter().any(|r| pattern_has_match_var(&r))
                || arr.rest().map_or(false, |r| pattern_has_match_var(&r))
        }
        Node::FindPatternNode { .. } => {
            let find = pattern.as_find_pattern_node().unwrap();
            find.requireds().iter().any(|r| pattern_has_match_var(&r)) || true
        }
        Node::HashPatternNode { .. } => {
            let hash = pattern.as_hash_pattern_node().unwrap();
            hash.elements().iter().any(|e|
                e.as_assoc_node().map_or(false, |a| pattern_has_match_var(&a.value())))
                || hash.rest().is_some()
        }
        Node::AlternationPatternNode { .. } => {
            let alt = pattern.as_alternation_pattern_node().unwrap();
            pattern_has_match_var(&alt.left()) || pattern_has_match_var(&alt.right())
        }
        Node::SplatNode { .. } => pattern.as_splat_node().unwrap().expression()
            .map_or(true, |e| pattern_has_match_var(&e)),
        Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::StringNode { .. }
        | Node::SymbolNode { .. } | Node::NilNode { .. } | Node::TrueNode { .. }
        | Node::FalseNode { .. } | Node::RangeNode { .. } | Node::RegularExpressionNode { .. }
        | Node::InterpolatedStringNode { .. } | Node::InterpolatedSymbolNode { .. }
        | Node::LambdaNode { .. } | Node::ImaginaryNode { .. } | Node::RationalNode { .. }
        | Node::ArrayNode { .. } | Node::HashNode { .. } | Node::SourceFileNode { .. }
        | Node::SourceLineNode { .. } | Node::SourceEncodingNode { .. } => false,
        _ => true,
    }
}

crate::register_cop!("Lint/LiteralAsCondition", |_cfg| {
    Some(Box::new(LiteralAsCondition::new()))
});
