//! Lint/DuplicateElsifCondition cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_elsif_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct DuplicateElsifCondition;

impl DuplicateElsifCondition {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateElsifCondition {
    fn name(&self) -> &'static str { "Lint/DuplicateElsifCondition" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ElsifVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct ElsifVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> ElsifVisitor<'a> {
    /// Collect (src, start, end) for all conditions in an if-elsif chain.
    /// Returns the list (first element is the `if` condition, rest are `elsif`s).
    fn collect_chain_conditions(node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<(String, usize, usize)> {
        let mut result = Vec::new();
        let cond = node.predicate();
        let loc = cond.location();
        result.push((ctx.src(loc.start_offset(), loc.end_offset()).to_string(), loc.start_offset(), loc.end_offset()));

        if let Some(sub) = node.subsequent() {
            if let Some(next_if) = sub.as_if_node() {
                // Recurse into the elsif chain
                // We can't hold &IfNode from sub because sub is a temporary Node.
                // Use the node's source location to get its offset, then re-parse isn't possible.
                // Instead, collect via the source: the sub Node is the elsif IfNode.
                // We collect via a helper that takes a Node and extracts the if-chain.
                collect_from_node(sub, ctx, &mut result);
            }
        }
        result
    }

    fn is_elsif(node: &ruby_prism::IfNode, ctx: &CheckContext) -> bool {
        if let Some(kl) = node.if_keyword_loc() {
            ctx.src(kl.start_offset(), kl.end_offset()) == "elsif"
        } else {
            false
        }
    }
}

fn collect_from_node(node: Node, ctx: &CheckContext, result: &mut Vec<(String, usize, usize)>) {
    if let Some(if_node) = node.as_if_node() {
        let cond = if_node.predicate();
        let loc = cond.location();
        result.push((ctx.src(loc.start_offset(), loc.end_offset()).to_string(), loc.start_offset(), loc.end_offset()));
        if let Some(sub) = if_node.subsequent() {
            collect_from_node(sub, ctx, result);
        }
    }
}

impl<'a> Visit<'_> for ElsifVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if !ElsifVisitor::is_elsif(node, self.ctx) {
            // Root of a chain — check for duplicates
            let conditions = ElsifVisitor::collect_chain_conditions(node, self.ctx);
            let mut seen: Vec<String> = Vec::new();
            for (src, start, end) in conditions {
                if seen.contains(&src) {
                    self.offenses.push(self.ctx.offense_with_range(
                        "Lint/DuplicateElsifCondition",
                        "Duplicate `elsif` condition detected.",
                        Severity::Warning,
                        start,
                        end,
                    ));
                }
                seen.push(src);
            }
        }

        // Visit then-branch
        if let Some(stmts) = node.statements() {
            for child in stmts.body().iter() {
                self.visit(&child);
            }
        }
        // Visit else branch — if it's an elsif chain, the root visit will handle it.
        // If it's a plain else node, visit it.
        if let Some(sub) = node.subsequent() {
            if sub.as_if_node().is_none() {
                self.visit(&sub);
            }
        }
    }
}

crate::register_cop!("Lint/DuplicateElsifCondition", |_cfg| {
    Some(Box::new(DuplicateElsifCondition::new()))
});
