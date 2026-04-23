//! Lint/SafeNavigationWithEmpty cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/safe_navigation_with_empty.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;
use ruby_prism::Node;

#[derive(Default)]
pub struct SafeNavigationWithEmpty;

impl SafeNavigationWithEmpty {
    pub fn new() -> Self { Self }
}

const MSG: &str = "Avoid calling `empty?` with the safe navigation operator in conditionals.";

impl Cop for SafeNavigationWithEmpty {
    fn name(&self) -> &'static str { "Lint/SafeNavigationWithEmpty" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_condition(node.predicate(), ctx)
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_condition(node.predicate(), ctx)
    }
}

impl SafeNavigationWithEmpty {
    fn check_condition(&self, cond: Node, ctx: &CheckContext) -> Vec<Offense> {
        // Pattern: (csend ... :empty?) where csend is the condition
        // Also handle negation: (not (csend ... :empty?)) — but RuboCop only flags direct csend
        let csend = match cond.as_call_node() {
            Some(c) if node_name!(c) == "empty?" && c.call_operator_loc().map(|l| {
                ctx.src(l.start_offset(), l.end_offset()) == "&."
            }).unwrap_or(false) => c,
            _ => return vec![],
        };

        // receiver of csend
        let recv = match csend.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let recv_src = ctx.src(recv.location().start_offset(), recv.location().end_offset());

        let loc = csend.location();
        let correction = Correction::replace(
            loc.start_offset(),
            loc.end_offset(),
            format!("{} && {}.empty?", recv_src, recv_src),
        );

        vec![ctx.offense_with_range(
            "Lint/SafeNavigationWithEmpty",
            MSG,
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        ).with_correction(correction)]
    }
}

crate::register_cop!("Lint/SafeNavigationWithEmpty", |_cfg| {
    Some(Box::new(SafeNavigationWithEmpty::new()))
});
