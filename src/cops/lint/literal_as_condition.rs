//! Lint/LiteralAsCondition - Checks for literals used as conditions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/literal_as_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

pub struct LiteralAsCondition;

impl LiteralAsCondition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for LiteralAsCondition {
    fn name(&self) -> &'static str {
        "Lint/LiteralAsCondition"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = LiteralConditionVisitor {
            ctx,
            offenses: Vec::new(),
            reported: HashSet::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct LiteralConditionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Track reported offense positions to avoid duplicates (start_offset)
    reported: HashSet<usize>,
}

impl<'a> LiteralConditionVisitor<'a> {
    /// Check a condition expression for literals. Handles &&/|| chains by checking LHS only.
    fn check_condition(&mut self, condition: &Node) {
        match condition {
            Node::AndNode { .. } => {
                let and_node = condition.as_and_node().unwrap();
                self.check_condition(&and_node.left());
            }
            Node::OrNode { .. } => {
                let or_node = condition.as_or_node().unwrap();
                self.check_condition(&or_node.left());
            }
            // `!expr` or `not(expr)` - check recursively
            Node::CallNode { .. } => {
                let call = condition.as_call_node().unwrap();
                let method_name = String::from_utf8_lossy(call.name().as_slice());
                if method_name.as_ref() == "!" {
                    if let Some(recv) = call.receiver() {
                        self.check_condition(&recv);
                    }
                } else if is_literal(condition) {
                    self.add_offense(condition);
                }
            }
            Node::ParenthesesNode { .. } => {
                let paren = condition.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if let Node::StatementsNode { .. } = &body {
                        let stmts = body.as_statements_node().unwrap();
                        let stmts_body: Vec<_> = stmts.body().iter().collect();
                        if stmts_body.len() == 1 {
                            self.check_condition(&stmts_body[0]);
                        }
                    }
                }
            }
            _ => {
                if is_literal(condition) {
                    self.add_offense(condition);
                }
            }
        }
    }

    fn add_offense(&mut self, node: &Node) {
        let loc = node.location();
        let start = loc.start_offset();
        if !self.reported.insert(start) {
            return;
        }
        let source_text = &self.ctx.source[start..loc.end_offset()];
        let message = format!("Literal `{}` appeared as a condition.", source_text);
        self.offenses.push(self.ctx.offense_with_range(
            "Lint/LiteralAsCondition",
            &message,
            Severity::Warning,
            start,
            loc.end_offset(),
        ));
    }
}

impl Visit<'_> for LiteralConditionVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let predicate = node.predicate();
        self.check_condition(&predicate);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let predicate = node.predicate();
        self.check_condition(&predicate);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let predicate = node.predicate();
        if !matches!(&predicate, Node::TrueNode { .. }) {
            self.check_condition(&predicate);
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let predicate = node.predicate();
        if !matches!(&predicate, Node::FalseNode { .. }) {
            self.check_condition(&predicate);
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if let Some(predicate) = node.predicate() {
            if is_literal(&predicate) {
                self.add_offense(&predicate);
            }
        } else {
            for cond in node.conditions().iter() {
                if let Node::WhenNode { .. } = &cond {
                    let when = cond.as_when_node().unwrap();
                    for when_cond in when.conditions().iter() {
                        if is_literal(&when_cond) {
                            self.add_offense(&when_cond);
                        }
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

    // Handle standalone `!literal` and `not(literal)` expressions
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        if method_name.as_ref() == "!" {
            if let Some(recv) = node.receiver() {
                // Recursively check for literals inside ! chains
                self.check_not_condition(&recv);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    // Handle &&/|| as standalone conditions (not just inside if/while predicates)
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        // Check LHS for literal
        let left = node.left();
        if is_literal(&left) {
            self.add_offense(&left);
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        // Check LHS for literal
        let left = node.left();
        if is_literal(&left) {
            self.add_offense(&left);
        }
        ruby_prism::visit_or_node(self, node);
    }
}

impl<'a> LiteralConditionVisitor<'a> {
    /// Check inside a `!` call for literal operands
    fn check_not_condition(&mut self, node: &Node) {
        if is_literal(node) {
            self.add_offense(node);
        }
        // Don't recurse into &&/|| here - they will be visited separately
    }
}

/// Check if a node is a literal value.
/// For this cop, interpolated strings with non-literal interpolation are NOT literals.
fn is_literal(node: &Node) -> bool {
    match node {
        Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. }
        | Node::StringNode { .. }
        | Node::SymbolNode { .. }
        | Node::RegularExpressionNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::NilNode { .. }
        | Node::SourceLineNode { .. }
        | Node::SourceFileNode { .. }
        | Node::SourceEncodingNode { .. }
        | Node::RangeNode { .. } => true,

        // Interpolated symbol: always truthy (symbol even with interpolation)
        Node::InterpolatedSymbolNode { .. } => true,

        // Interpolated string: only literal if all parts are literal
        // `"hello"` with no interpolation => literal
        // `"#{x}"` => NOT literal (depends on runtime value)
        // However, RuboCop considers interpolated strings as literals for condition checking
        // unless they contain non-literal interpolation.
        // Actually, looking at the tests: `case "#{x}"` is NOT flagged (offenses = [])
        // while `:"#{a}"` IS flagged. So interpolated strings are not literals here,
        // but interpolated symbols are.
        Node::InterpolatedStringNode { .. } => false,
        Node::InterpolatedRegularExpressionNode { .. } => false,

        // Array literals: only if ALL elements are literal
        Node::ArrayNode { .. } => {
            let arr = node.as_array_node().unwrap();
            arr.elements().iter().all(|e| is_literal(&e))
        }

        // Hash literals: only if ALL key-value pairs are literal
        Node::HashNode { .. } => {
            let hash = node.as_hash_node().unwrap();
            hash.elements().iter().all(|e| {
                if let Node::AssocNode { .. } = &e {
                    let assoc = e.as_assoc_node().unwrap();
                    is_literal(&assoc.key()) && is_literal(&assoc.value())
                } else {
                    false
                }
            })
        }

        _ => false,
    }
}

/// Check if a CaseMatchNode has any match variable patterns in its in-clauses.
fn has_match_var_pattern(case_match: &ruby_prism::CaseMatchNode) -> bool {
    for cond in case_match.conditions().iter() {
        if let Node::InNode { .. } = &cond {
            let in_node = cond.as_in_node().unwrap();
            if pattern_has_match_var(&in_node.pattern()) {
                return true;
            }
        }
    }
    false
}

/// Recursively check if a pattern contains a match variable (local variable capture).
fn pattern_has_match_var(pattern: &Node) -> bool {
    match pattern {
        Node::LocalVariableTargetNode { .. } => true,
        Node::PinnedVariableNode { .. } => false,
        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => false,
        Node::ArrayPatternNode { .. } => {
            let arr = pattern.as_array_pattern_node().unwrap();
            for req in arr.requireds().iter() {
                if pattern_has_match_var(&req) {
                    return true;
                }
            }
            for rest in arr.posts().iter() {
                if pattern_has_match_var(&rest) {
                    return true;
                }
            }
            if let Some(rest) = arr.rest() {
                if pattern_has_match_var(&rest) {
                    return true;
                }
            }
            false
        }
        Node::FindPatternNode { .. } => {
            let find = pattern.as_find_pattern_node().unwrap();
            for req in find.requireds().iter() {
                if pattern_has_match_var(&req) {
                    return true;
                }
            }
            true
        }
        Node::HashPatternNode { .. } => {
            let hash = pattern.as_hash_pattern_node().unwrap();
            for elem in hash.elements().iter() {
                if let Node::AssocNode { .. } = &elem {
                    let assoc = elem.as_assoc_node().unwrap();
                    if pattern_has_match_var(&assoc.value()) {
                        return true;
                    }
                }
            }
            if hash.rest().is_some() {
                return true;
            }
            false
        }
        Node::CapturePatternNode { .. } => true,
        Node::AlternationPatternNode { .. } => {
            let alt = pattern.as_alternation_pattern_node().unwrap();
            pattern_has_match_var(&alt.left()) || pattern_has_match_var(&alt.right())
        }
        Node::SplatNode { .. } => {
            let splat = pattern.as_splat_node().unwrap();
            if let Some(expr) = splat.expression() {
                pattern_has_match_var(&expr)
            } else {
                true
            }
        }
        // Literals in patterns are not match vars
        Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::StringNode { .. }
        | Node::SymbolNode { .. }
        | Node::NilNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::RangeNode { .. }
        | Node::RegularExpressionNode { .. }
        | Node::InterpolatedStringNode { .. }
        | Node::InterpolatedSymbolNode { .. }
        | Node::LambdaNode { .. }
        | Node::ImaginaryNode { .. }
        | Node::RationalNode { .. }
        | Node::ArrayNode { .. }
        | Node::HashNode { .. }
        | Node::SourceFileNode { .. }
        | Node::SourceLineNode { .. }
        | Node::SourceEncodingNode { .. } => false,

        // Default: assume it might be a match var to be safe
        _ => true,
    }
}
