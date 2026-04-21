//! Style/NegatedUnless — flag `unless !x; ...; end` / `x unless !y` in favor of `if`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/negated_unless.rb
//! Mixin: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/negative_conditional.rb

use crate::cops::{CheckContext, Cop};
use crate::cops::style::negated_if::{build_correction, EnforcedStyle};
use crate::helpers::negative_conditional::{match_negative_condition, MSG};
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Style/NegatedUnless";

#[derive(Default)]
pub struct NegatedUnless {
    style: EnforcedStyle,
}

impl NegatedUnless {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Cop for NegatedUnless {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip unless with an else clause.
        if node.else_clause().is_some() {
            return vec![];
        }

        let kw_loc = node.keyword_loc();
        let is_modifier = node.end_keyword_loc().is_none();

        if !style_applies(self.style, is_modifier) {
            return vec![];
        }

        let Some(m) = match_negative_condition(node.predicate(), ctx.source) else {
            return vec![];
        };

        let message = MSG
            .replace("%<inverse>s", "if")
            .replace("%<current>s", "unless");

        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let mut offense =
            ctx.offense_with_range(COP_NAME, &message, Severity::Convention, node_start, node_end);

        offense = offense.with_correction(build_correction(
            kw_loc.start_offset(),
            kw_loc.end_offset(),
            "if",
            &m.negated_call,
            ctx.source,
        ));

        vec![offense]
    }
}

fn style_applies(style: EnforcedStyle, is_modifier: bool) -> bool {
    match style {
        EnforcedStyle::Both => true,
        EnforcedStyle::Prefix => !is_modifier,
        EnforcedStyle::Postfix => is_modifier,
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/NegatedUnless", |cfg| {
    let c: Cfg = cfg.typed("Style/NegatedUnless");
    let style = match c.enforced_style.as_str() {
        "prefix" => EnforcedStyle::Prefix,
        "postfix" => EnforcedStyle::Postfix,
        _ => EnforcedStyle::Both,
    };
    Some(Box::new(NegatedUnless::with_style(style)))
});
