//! Style/NegatedIf — flag `if !x; ...; end` / `x if !y` in favor of `unless`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/negated_if.rb
//! Mixin: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/negative_conditional.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::negative_conditional::{match_negative_condition, MSG};
use crate::offense::{Correction, Edit, Offense, Severity};

const COP_NAME: &str = "Style/NegatedIf";

/// EnforcedStyle for Style/NegatedIf (and Style/NegatedUnless).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    /// Flag both prefix (`if !x ... end`) and postfix (`x if !y`) forms.
    Both,
    /// Flag only prefix form.
    Prefix,
    /// Flag only postfix form.
    Postfix,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::Both
    }
}

#[derive(Default)]
pub struct NegatedIf {
    style: EnforcedStyle,
}

impl NegatedIf {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Cop for NegatedIf {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip elsif (an elsif is an IfNode with if_keyword == "elsif").
        let Some(kw_loc) = node.if_keyword_loc() else {
            // Ternary: `a ? b : c` — no keyword.
            return vec![];
        };
        let kw = ctx.src(kw_loc.start_offset(), kw_loc.end_offset());
        if kw == "elsif" {
            return vec![];
        }

        // Skip if there is any `subsequent` (else or elsif chain).
        if node.subsequent().is_some() {
            return vec![];
        }

        // Distinguish prefix vs postfix form: prefix has an `end` keyword.
        let is_modifier = node.end_keyword_loc().is_none();

        if !style_applies(self.style, is_modifier) {
            return vec![];
        }

        let Some(m) = match_negative_condition(node.predicate(), ctx.source) else {
            return vec![];
        };

        let message = MSG
            .replace("%<inverse>s", "unless")
            .replace("%<current>s", "if");

        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let mut offense =
            ctx.offense_with_range(COP_NAME, &message, Severity::Convention, node_start, node_end);

        // Autocorrect: swap keyword, drop the `!` / `not` wrapper.
        offense = offense.with_correction(build_correction(
            kw_loc.start_offset(),
            kw_loc.end_offset(),
            "unless",
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

/// Build the corrector edits for `ConditionCorrector.correct_negative_condition`:
/// replace the leading keyword with its inverse, and replace the negated call
/// (`!x` / `not x`) with just its receiver's source.
pub(crate) fn build_correction(
    kw_start: usize,
    kw_end: usize,
    inverse_keyword: &str,
    negated_call: &ruby_prism::Node,
    source: &str,
) -> Correction {
    let call = negated_call
        .as_call_node()
        .expect("match_negative_condition guarantees a CallNode");
    let receiver = call
        .receiver()
        .expect("single_negative? implies a receiver");
    let recv_start = receiver.location().start_offset();
    let recv_end = receiver.location().end_offset();
    let call_start = call.location().start_offset();
    let call_end = call.location().end_offset();

    Correction {
        edits: vec![
            Edit {
                start_offset: kw_start,
                end_offset: kw_end,
                replacement: inverse_keyword.to_string(),
            },
            Edit {
                start_offset: call_start,
                end_offset: call_end,
                replacement: source[recv_start..recv_end].to_string(),
            },
        ],
    }
}
