//! Lint/DuplicateMatchPattern - Checks for repeated patterns in `in` keywords.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_match_pattern.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct DuplicateMatchPattern;

impl DuplicateMatchPattern {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateMatchPattern {
    fn name(&self) -> &'static str { "Lint/DuplicateMatchPattern" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn src_of(&self, node: &Node) -> &str {
        let loc = node.location();
        &self.ctx.source[loc.start_offset()..loc.end_offset()]
    }

    /// Splits `in_node.pattern()` into (actual pattern, optional guard source).
    /// In Prism, `in foo if cond` → pattern() == IfNode { predicate = cond, statements = foo }.
    fn split_guard<'b>(&self, pattern: Node<'b>) -> (Node<'b>, Option<String>) {
        if let Some(if_node) = pattern.as_if_node() {
            // Guarded: statements has the real pattern, predicate is the guard expr
            if let Some(stmts) = if_node.statements() {
                if let Some(inner) = stmts.body().iter().next() {
                    let guard_src = format!("if {}", self.src_of(&if_node.predicate()));
                    return (inner, Some(guard_src));
                }
            }
        }
        if let Some(unless_node) = pattern.as_unless_node() {
            if let Some(stmts) = unless_node.statements() {
                if let Some(inner) = stmts.body().iter().next() {
                    let guard_src = format!("unless {}", self.src_of(&unless_node.predicate()));
                    return (inner, Some(guard_src));
                }
            }
        }
        (pattern, None)
    }

    fn pattern_identity(&self, pattern: &Node, guard: &Option<String>) -> String {
        let mut id = match pattern {
            Node::HashPatternNode { .. } => {
                let hp = pattern.as_hash_pattern_node().unwrap();
                let mut parts: Vec<String> = hp.elements().iter().map(|e| self.src_of(&e).to_string()).collect();
                if let Some(rest) = hp.rest() {
                    parts.push(self.src_of(&rest).to_string());
                }
                parts.sort();
                parts.join(",")
            }
            Node::AlternationPatternNode { .. } => {
                // Flatten left-associative alt: (((a | b) | c) | d) → [a,b,c,d], sort
                let mut parts: Vec<String> = Vec::new();
                self.collect_alt(pattern, &mut parts);
                parts.sort();
                format!("ALT|{}", parts.join("|"))
            }
            _ => self.src_of(pattern).to_string(),
        };
        if let Some(g) = guard {
            id.push_str("##");
            id.push_str(g);
        }
        id
    }

    fn collect_alt(&self, node: &Node, out: &mut Vec<String>) {
        if let Some(alt) = node.as_alternation_pattern_node() {
            let left = alt.left();
            let right = alt.right();
            self.collect_alt(&left, out);
            self.collect_alt(&right, out);
        } else {
            out.push(self.src_of(node).to_string());
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        let mut seen: Vec<String> = Vec::new();
        for cond in node.conditions().iter() {
            let Some(in_node) = cond.as_in_node() else { continue };
            let (inner_pattern, guard) = self.split_guard(in_node.pattern());
            let id = self.pattern_identity(&inner_pattern, &guard);
            if seen.contains(&id) {
                let loc = inner_pattern.location();
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/DuplicateMatchPattern",
                    "Duplicate `in` pattern detected.",
                    Severity::Warning,
                    loc.start_offset(),
                    loc.end_offset(),
                ));
            } else {
                seen.push(id);
            }
        }
        ruby_prism::visit_case_match_node(self, node);
    }
}

crate::register_cop!("Lint/DuplicateMatchPattern", |_cfg| {
    Some(Box::new(DuplicateMatchPattern::new()))
});
