//! Style/NegatedWhile — flag `while !x; ...; end` / `x while !y` (and the same
//! for `until`) in favor of the inverse keyword.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/negated_while.rb
//! Mixin: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/negative_conditional.rb

use crate::cops::{CheckContext, Cop};
use crate::cops::style::negated_if::build_correction;
use crate::helpers::negative_conditional::{match_negative_condition, MSG};
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Style/NegatedWhile";

#[derive(Default)]
pub struct NegatedWhile;

impl NegatedWhile {
    pub fn new() -> Self {
        Self
    }

    fn emit(
        &self,
        keyword: &str,
        inverse: &str,
        kw_start: usize,
        kw_end: usize,
        node_start: usize,
        node_end: usize,
        predicate: ruby_prism::Node<'_>,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        let m = match_negative_condition(predicate, ctx.source)?;

        let message = MSG
            .replace("%<inverse>s", inverse)
            .replace("%<current>s", keyword);

        let offense = ctx
            .offense_with_range(COP_NAME, &message, Severity::Convention, node_start, node_end)
            .with_correction(build_correction(
                kw_start,
                kw_end,
                inverse,
                &m.negated_call,
                ctx.source,
            ));

        Some(offense)
    }
}

impl Cop for NegatedWhile {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_while(&self, node: &ruby_prism::WhileNode, ctx: &CheckContext) -> Vec<Offense> {
        let kw_loc = node.keyword_loc();
        self.emit(
            "while",
            "until",
            kw_loc.start_offset(),
            kw_loc.end_offset(),
            node.location().start_offset(),
            node.location().end_offset(),
            node.predicate(),
            ctx,
        )
        .into_iter()
        .collect()
    }

    fn check_until(&self, node: &ruby_prism::UntilNode, ctx: &CheckContext) -> Vec<Offense> {
        let kw_loc = node.keyword_loc();
        self.emit(
            "until",
            "while",
            kw_loc.start_offset(),
            kw_loc.end_offset(),
            node.location().start_offset(),
            node.location().end_offset(),
            node.predicate(),
            ctx,
        )
        .into_iter()
        .collect()
    }
}
