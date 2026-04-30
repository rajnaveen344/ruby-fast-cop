//! Lint/LiteralAsCondition cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
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
            // AndNode/OrNode: handled by visit_and_node / visit_or_node (which can attach correction)
            Node::AndNode { .. } | Node::OrNode { .. } => {}
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
        self.add_offense_with_correction(node, None);
    }

    fn add_offense_with_correction(&mut self, node: &Node, correction: Option<Correction>) {
        let loc = node.location();
        if !self.reported.insert(loc.start_offset()) { return; }
        let mut offense = self.ctx.offense_with_range(
            "Lint/LiteralAsCondition",
            &format!("Literal `{}` appeared as a condition.", &self.ctx.source[loc.start_offset()..loc.end_offset()]),
            Severity::Warning, loc.start_offset(), loc.end_offset(),
        );
        if let Some(c) = correction { offense = offense.with_correction(c); }
        self.offenses.push(offense);
    }
}

impl Visit<'_> for LiteralConditionVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let pred = node.predicate();
        let truthy = is_truthy_literal(&pred);
        let falsey = is_falsey_literal(&pred);
        if truthy || falsey {
            let correction = compute_if_correction(node, self.ctx.source, truthy, false);
            self.add_offense_with_correction(&pred, correction);
        } else {
            self.check_condition(&pred);
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let pred = node.predicate();
        let truthy = is_truthy_literal(&pred);
        let falsey = is_falsey_literal(&pred);
        if truthy || falsey {
            let correction = compute_unless_correction(node, self.ctx.source, falsey);
            self.add_offense_with_correction(&pred, correction);
        } else {
            self.check_condition(&pred);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let pred = node.predicate();
        if !matches!(&pred, Node::TrueNode { .. }) {
            let body_src = postloop_body_source(&node.statements(), self.ctx.source);
            let correction = compute_loop_correction(node.location().start_offset(),
                node.location().end_offset(), &pred, /*invert=*/false,
                node.is_begin_modifier(), &body_src);
            if correction.is_some() {
                self.add_offense_with_correction(&pred, correction);
            } else {
                self.check_condition(&pred);
            }
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let pred = node.predicate();
        if !matches!(&pred, Node::FalseNode { .. }) {
            let body_src = postloop_body_source(&node.statements(), self.ctx.source);
            let correction = compute_loop_correction(node.location().start_offset(),
                node.location().end_offset(), &pred, /*invert=*/true,
                node.is_begin_modifier(), &body_src);
            if correction.is_some() {
                self.add_offense_with_correction(&pred, correction);
            } else {
                self.check_condition(&pred);
            }
        }
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
        if is_truthy_literal(&left) {
            let correction = and_or_replacement(&node.right(), node.location().start_offset(), node.location().end_offset(), self.ctx.source);
            self.add_offense_with_correction(&left, correction);
        } else if is_literal(&left) {
            self.add_offense(&left);
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let left = node.left();
        if is_falsey_literal(&left) {
            let correction = and_or_replacement(&node.right(), node.location().start_offset(), node.location().end_offset(), self.ctx.source);
            self.add_offense_with_correction(&left, correction);
        } else if is_literal(&left) {
            self.add_offense(&left);
        }
        ruby_prism::visit_or_node(self, node);
    }
}

fn is_truthy_literal(node: &Node) -> bool {
    match node {
        Node::FalseNode { .. } | Node::NilNode { .. } => false,
        _ => is_literal(node),
    }
}

fn is_falsey_literal(node: &Node) -> bool {
    matches!(node, Node::FalseNode { .. } | Node::NilNode { .. })
}

fn and_or_replacement(rhs: &Node, node_start: usize, node_end: usize, source: &str) -> Option<Correction> {
    // Skip if rhs is return/break/next (can produce void value error)
    if matches!(rhs,
        Node::ReturnNode { .. }
        | Node::BreakNode { .. }
        | Node::NextNode { .. }
    ) {
        return None;
    }
    let rloc = rhs.location();
    let rhs_src = source[rloc.start_offset()..rloc.end_offset()].to_string();
    Some(Correction::replace(node_start, node_end, &rhs_src))
}

/// Extract postloop body source: `begin..end while X` — body is begin block,
/// we want just the inner statements joined with \n (matches RuboCop's
/// `body.child_nodes.map(&:source).join("\n")`).
fn postloop_body_source(stmts: &Option<ruby_prism::StatementsNode>, source: &str) -> String {
    let s = match stmts { Some(s) => s, None => return String::new() };
    // Walk children: if a single BeginNode, drill into its statements.
    let body: Vec<_> = s.body().iter().collect();
    if body.len() == 1 {
        if let Some(begin) = body[0].as_begin_node() {
            if let Some(inner) = begin.statements() {
                let parts: Vec<String> = inner.body().iter().map(|c| {
                    let l = c.location();
                    source[l.start_offset()..l.end_offset()].to_string()
                }).collect();
                return parts.join("\n");
            }
            return String::new();
        }
    }
    let parts: Vec<String> = body.iter().map(|c| {
        let l = c.location();
        source[l.start_offset()..l.end_offset()].to_string()
    }).collect();
    parts.join("\n")
}

/// For while/until loops:
///   - while truthy → replace cond with "true"
///   - while falsey → remove whole node (replace with "")
///   - until falsey → replace cond with "false"
///   - until truthy → remove whole node (replace with "")
/// `invert=true` for until.
fn compute_loop_correction(
    node_start: usize, node_end: usize, pred: &Node, invert: bool,
    is_postloop: bool, body_src: &str,
) -> Option<Correction> {
    let truthy = is_truthy_literal(pred);
    let falsey = is_falsey_literal(pred);
    if !truthy && !falsey { return None; }
    let pred_loc = pred.location();
    // For while: keep when truthy (replace cond with "true"), drop when falsey.
    // For until: keep when falsey (replace cond with "false"), drop when truthy.
    let keep = if invert { falsey } else { truthy };
    if keep {
        let lit = if invert { "false" } else { "true" };
        Some(Correction::replace(pred_loc.start_offset(), pred_loc.end_offset(), lit))
    } else if is_postloop {
        // Postloop drop: replacement = body source (inner stmts of begin..end).
        // Defer if body_src wasn't successfully extracted (statements() returned None).
        if body_src.is_empty() {
            None
        } else {
            Some(Correction::replace(node_start, node_end, body_src))
        }
    } else {
        Some(Correction::replace(node_start, node_end, ""))
    }
}

fn statements_source<'a>(stmts: &Option<ruby_prism::StatementsNode>, source: &'a str) -> &'a str {
    match stmts {
        Some(s) => {
            let loc = s.location();
            &source[loc.start_offset()..loc.end_offset()]
        }
        None => "",
    }
}

fn compute_if_correction(
    node: &ruby_prism::IfNode,
    source: &str,
    truthy: bool,
    _is_unless: bool,
) -> Option<Correction> {
    // Don't autocorrect when this if-node is itself an elsif chain (subsequent IfNode w/ no end_keyword_loc)
    // Detected by: the parent visit chain — but we have no parent ref. Use end_keyword_loc heuristic:
    //   - Block-form if has end_keyword_loc
    //   - Modifier-if has no end_keyword_loc
    //   - Ternary has no end_keyword_loc
    //   - Elsif (subsequent of outer if) — Prism gives the elsif IfNode no end_keyword_loc
    // To avoid mis-correcting elsif as if it were modifier-if, only correct when:
    //   (a) end_keyword_loc present (block-form), OR
    //   (b) no end_keyword AND if_keyword_loc starts at same byte as predicate-1 or earlier
    //       (modifier-if has predicate AFTER if_keyword; ternary has if_keyword at start of cond)
    //   For safety: correct only block-form, modifier-if, ternary; bail on elsif (handled by parent's correction).
    let if_kw = node.if_keyword_loc();
    let node_start = node.location().start_offset();
    let node_end = node.location().end_offset();
    // Bail on elsif (RuboCop has special elsif handling we don't replicate yet).
    if let Some(kw) = &if_kw {
        let s = &source[kw.start_offset()..kw.end_offset()];
        if s == "elsif" {
            return None;
        }
    }
    // Modifier-if: predicate appears after the body, but if_keyword is between body and predicate.
    // Determine if_branch / else_branch sources.
    let then_src = statements_source(&node.statements(), source).to_string();
    let else_src = match node.subsequent() {
        Some(sub) => match sub.as_else_node() {
            Some(en) => statements_source(&en.statements(), source).to_string(),
            None => {
                // subsequent is elsif IfNode — bail (we'd need to rewrite elsif→if)
                return None;
            }
        },
        None => String::new(),
    };
    let replacement = if truthy { then_src } else { else_src };
    Some(Correction::replace(node_start, node_end, &replacement))
}

fn compute_unless_correction(
    node: &ruby_prism::UnlessNode,
    source: &str,
    falsey: bool,
) -> Option<Correction> {
    // For unless: result = falsey_literal? → take then-branch; else → else-branch.
    let then_src = statements_source(&node.statements(), source).to_string();
    let else_src = match node.else_clause() {
        Some(en) => statements_source(&en.statements(), source).to_string(),
        None => String::new(),
    };
    let node_start = node.location().start_offset();
    let node_end = node.location().end_offset();
    let replacement = if falsey { then_src } else { else_src };
    Some(Correction::replace(node_start, node_end, &replacement))
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
