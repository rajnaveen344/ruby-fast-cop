//! Style/PercentQLiterals cop — enforces `%q` vs `%Q` per `EnforcedStyle`.
//!
//! Translates RuboCop's
//! `lib/rubocop/cop/style/percent_q_literals.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::percent_literal;
use crate::offense::{Correction, Offense, Severity};

const LOWER_CASE_Q_MSG: &str = "Do not use `%Q` unless interpolation is needed. Use `%q`.";
const UPPER_CASE_Q_MSG: &str = "Use `%Q` instead of `%q`.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    LowerCaseQ,
    UpperCaseQ,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::LowerCaseQ
    }
}

#[derive(Default)]
pub struct PercentQLiterals {
    style: EnforcedStyle,
}

impl PercentQLiterals {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(style: EnforcedStyle) -> Self {
        Self { style }
    }

    fn message(&self) -> &'static str {
        match self.style {
            EnforcedStyle::LowerCaseQ => LOWER_CASE_Q_MSG,
            EnforcedStyle::UpperCaseQ => UPPER_CASE_Q_MSG,
        }
    }

    fn correct_literal_style(&self, ty: &str) -> bool {
        match self.style {
            EnforcedStyle::LowerCaseQ => ty == "%q",
            EnforcedStyle::UpperCaseQ => ty == "%Q",
        }
    }
}

/// Heuristic for RuboCop's "does changing case preserve semantics?" check:
/// if the body contains any backslash escape or interpolation start (`#{`),
/// changing `%q` ↔ `%Q` can alter the parsed string, so skip.
fn body_is_convertible(body: &str) -> bool {
    !body.contains('\\') && !body.contains("#{")
}

impl Cop for PercentQLiterals {
    fn name(&self) -> &'static str {
        "Style/PercentQLiterals"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_string(&self, node: &ruby_prism::StringNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(open_loc) = node.opening_loc() else { return vec![] };
        let open_start = open_loc.start_offset();
        let open_end = open_loc.end_offset();
        let open_src = &ctx.source[open_start..open_end];
        let Some(ty) = percent_literal::percent_type(open_src) else { return vec![] };
        if ty != "%q" && ty != "%Q" {
            return vec![];
        }
        if self.correct_literal_style(ty) {
            return vec![];
        }

        let node_end = node.location().end_offset();
        // Body lies between the opening delimiter (`%q(`) and the closing
        // delimiter (single char).
        if node_end < open_end + 1 {
            return vec![];
        }
        let body = &ctx.source[open_end..node_end - 1];
        if !body_is_convertible(body) {
            return vec![];
        }

        // Offense on `loc.begin` — the full opening delimiter (e.g. `%Q(`).
        let off_start = open_start;
        let off_end = open_end;

        // Correction: swap case of the letter after `%`.
        let new_open: String = open_src
            .char_indices()
            .map(|(i, c)| if i == 1 { match c { 'q' => 'Q', 'Q' => 'q', other => other } } else { c })
            .collect();

        let offense = ctx
            .offense_with_range(self.name(), self.message(), self.severity(), off_start, off_end)
            .with_correction(Correction::replace(open_start, open_end, new_open));
        vec![offense]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/PercentQLiterals", |cfg| {
    let c: Cfg = cfg.typed("Style/PercentQLiterals");
    let style = match c.enforced_style.as_str() {
        "upper_case_q" => EnforcedStyle::UpperCaseQ,
        _ => EnforcedStyle::LowerCaseQ,
    };
    Some(Box::new(PercentQLiterals::with_style(style)))
});
