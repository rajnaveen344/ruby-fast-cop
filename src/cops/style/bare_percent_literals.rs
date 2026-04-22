//! Style/BarePercentLiterals — Checks `%` vs `%Q` string delimiters.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/bare_percent_literals.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::percent_literal;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    BarePercent,
    PercentQ,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::BarePercent
    }
}

pub struct BarePercentLiterals {
    style: EnforcedStyle,
}

impl Default for BarePercentLiterals {
    fn default() -> Self {
        Self { style: EnforcedStyle::BarePercent }
    }
}

impl BarePercentLiterals {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Cop for BarePercentLiterals {
    fn name(&self) -> &'static str {
        "Style/BarePercentLiterals"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = BarePercentVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct BarePercentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
}

impl<'a> BarePercentVisitor<'a> {
    fn check_open(&mut self, open_src: &str, open_start: usize, open_end: usize) {
        let Some(ty) = percent_literal::percent_type(open_src) else { return };
        match self.style {
            EnforcedStyle::PercentQ => {
                if ty == "%" {
                    // `%(` → replace `%` with `%Q`, offense covers full opening `%(`
                    let offense = self.ctx.offense_with_range(
                        "Style/BarePercentLiterals",
                        "Use `%Q` instead of `%`.",
                        Severity::Convention,
                        open_start,
                        open_end,
                    );
                    let delim = &open_src[1..]; // delimiter char(s) after %
                    let new_open = format!("%Q{}", delim);
                    let correction = Correction::replace(open_start, open_end, new_open);
                    self.offenses.push(offense.with_correction(correction));
                }
            }
            EnforcedStyle::BarePercent => {
                if ty == "%Q" {
                    // `%Q(` → replace `%Q` with `%`, offense covers full opening `%Q(`
                    let offense = self.ctx.offense_with_range(
                        "Style/BarePercentLiterals",
                        "Use `%` instead of `%Q`.",
                        Severity::Convention,
                        open_start,
                        open_end,
                    );
                    let delim = &open_src[2..]; // delimiter char(s) after %Q
                    let new_open = format!("%{}", delim);
                    let correction = Correction::replace(open_start, open_end, new_open);
                    self.offenses.push(offense.with_correction(correction));
                }
            }
        }
    }
}

impl<'a> Visit<'_> for BarePercentVisitor<'a> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if let Some(open_loc) = node.opening_loc() {
            let open_start = open_loc.start_offset();
            let open_end = open_loc.end_offset();
            let open_src = &self.ctx.source[open_start..open_end];
            self.check_open(open_src, open_start, open_end);
        }
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if let Some(open_loc) = node.opening_loc() {
            let open_start = open_loc.start_offset();
            let open_end = open_loc.end_offset();
            let open_src = &self.ctx.source[open_start..open_end];
            self.check_open(open_src, open_start, open_end);
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/BarePercentLiterals", |cfg| {
    let c: Cfg = cfg.typed("Style/BarePercentLiterals");
    let style = match c.enforced_style.as_str() {
        "percent_q" => EnforcedStyle::PercentQ,
        _ => EnforcedStyle::BarePercent,
    };
    Some(Box::new(BarePercentLiterals::new(style)))
});
