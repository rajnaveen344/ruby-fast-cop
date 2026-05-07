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
        let mut visitor = LiteralConditionVisitor {
            ctx,
            offenses: Vec::new(),
            reported: HashSet::new(),
            if_level_corrections: std::collections::HashMap::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct LiteralConditionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    reported: HashSet<usize>,
    /// For `if LITERAL && RHS` / `if LITERAL || RHS` cases: stores the if-level correction
    /// keyed by the literal's start offset. Used in visit_and_node / visit_or_node to
    /// emit the full if-correction instead of the and/or-level one.
    if_level_corrections: std::collections::HashMap<usize, Correction>,
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
            // Check if predicate is AndNode/OrNode with literal LHS — if so, precompute
            // the if-level correction and store it so visit_and_node/visit_or_node can
            // attach it to the literal offense instead of the and/or-level replacement.
            if let Node::AndNode { .. } = &pred {
                let an = pred.as_and_node().unwrap();
                let left = an.left();
                let right = an.right();
                if is_truthy_literal(&left) && is_truthy_literal(&right) {
                    // Both sides truthy literal → `if LITERAL && LITERAL; body; end` → `body`
                    // Single-pass: replace whole if with then_src.
                    let then_src = statements_source(&node.statements(), self.ctx.source).to_string();
                    let c = Correction::replace(
                        node.location().start_offset(),
                        node.location().end_offset(),
                        &then_src,
                    );
                    self.if_level_corrections.insert(left.location().start_offset(), c);
                }
                // For `1 && non_literal`: and_or_replacement handles it (replace and-node with rhs).
            }
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
            // Check if we have a precomputed if-level correction for this literal.
            let correction = if let Some(if_corr) = self.if_level_corrections.remove(&left.location().start_offset()) {
                Some(if_corr)
            } else {
                and_or_replacement(&node.right(), node.location().start_offset(), node.location().end_offset(), self.ctx.source)
            };
            self.add_offense_with_correction(&left, correction);
        } else if is_literal(&left) {
            self.add_offense(&left);
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let left = node.left();
        if is_falsey_literal(&left) {
            // Check if we have a precomputed if-level correction for this literal.
            let correction = if let Some(if_corr) = self.if_level_corrections.remove(&left.location().start_offset()) {
                Some(if_corr)
            } else {
                and_or_replacement(&node.right(), node.location().start_offset(), node.location().end_offset(), self.ctx.source)
            };
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

/// Like `statements_source` but extends the range through a trailing `# ...`
/// line comment if one is present on the same line as the last statement.
fn statements_source_with_trailing_comment(
    stmts: &Option<ruby_prism::StatementsNode>,
    source: &str,
) -> String {
    let Some(s) = stmts else { return String::new() };
    let bytes = source.as_bytes();
    let start = s.location().start_offset();
    let mut end = s.location().end_offset();
    let mut i = end;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'#' {
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        end = i;
    }
    source[start..end].to_string()
}

fn compute_if_correction(
    node: &ruby_prism::IfNode,
    source: &str,
    truthy: bool,
    _is_unless: bool,
) -> Option<Correction> {
    let if_kw = node.if_keyword_loc();
    let node_start = node.location().start_offset();
    let node_end = node.location().end_offset();

    // Check if this is an `elsif` node.
    let is_elsif = if let Some(kw) = &if_kw {
        &source[kw.start_offset()..kw.end_offset()] == "elsif"
    } else {
        false
    };

    if is_elsif {
        // This IfNode is an `elsif X; body; [else; else_body;] end`.
        // Truthy: replace with `else\n  body\nend` (taking the then-branch as always-true).
        // Falsey: replace with `else\n  else_body\nend` (skipping this branch).
        let then_src = statements_source_with_trailing_comment(&node.statements(), source);
        let replacement = if truthy {
            format!("else\n  {}\nend", then_src)
        } else {
            // falsey: take the else branch if any
            match node.subsequent() {
                Some(sub) => {
                    if let Some(en) = sub.as_else_node() {
                        let else_src = statements_source_with_trailing_comment(&en.statements(), source);
                        format!("else\n  {}\nend", else_src)
                    } else {
                        // subsequent is another elsif — complex chain, skip
                        return None;
                    }
                }
                None => {
                    // No else branch: falsey elsif with no else — just remove the branch.
                    // The `end` is included in this node's range. Replace with just `end`.
                    "end".to_string()
                }
            }
        };
        return Some(Correction::replace(node_start, node_end, &replacement));
    }

    // Not an elsif: handle regular if/modifier-if/ternary.
    // Determine if_branch / else_branch sources.
    let then_src = statements_source(&node.statements(), source).to_string();
    match node.subsequent() {
        Some(sub) => {
            if let Some(en) = sub.as_else_node() {
                // Has explicit `else` branch.
                let else_src = statements_source(&en.statements(), source).to_string();
                let replacement = if truthy { then_src } else { else_src };
                Some(Correction::replace(node_start, node_end, &replacement))
            } else {
                // subsequent is an elsif IfNode — `if LITERAL; body; elsif ...; end`
                // When truthy: take the then-branch.
                // When falsey: rewrite subsequent elif back to `if` (node.elsif_conditional? case).
                if truthy {
                    Some(Correction::replace(node_start, node_end, &then_src))
                } else {
                    // Replace whole outer if with subsequent IfNode, renaming `elsif` → `if`.
                    let sub_loc = sub.location();
                    let sub_src = &source[sub_loc.start_offset()..sub_loc.end_offset()];
                    // sub_src starts with `elsif` — replace with `if`.
                    let new_src = format!("if{}", &sub_src["elsif".len()..]);
                    Some(Correction::replace(node_start, node_end, &new_src))
                }
            }
        }
        None => {
            // No subsequent: simple if without else.
            let replacement = if truthy { then_src } else { String::new() };
            Some(Correction::replace(node_start, node_end, &replacement))
        }
    }
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
