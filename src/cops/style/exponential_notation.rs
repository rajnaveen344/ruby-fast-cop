//! Style/ExponentialNotation — Enforces consistent formatting of exponential notation.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/exponential_notation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Scientific,
    Engineering,
    Integral,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::Scientific
    }
}

pub struct ExponentialNotation {
    style: EnforcedStyle,
}

impl Default for ExponentialNotation {
    fn default() -> Self {
        Self { style: EnforcedStyle::Scientific }
    }
}

impl ExponentialNotation {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }

    /// Parse mantissa/exponent from text like "12.3e4" or "12e4".
    /// Returns (mantissa_str, exponent, exclusive) or None if not exponential.
    fn parse_exponential(text: &str) -> Option<(&str, i64)> {
        // Find 'e' or 'E' (but not at start)
        let e_pos = text.find(['e', 'E'])?;
        if e_pos == 0 { return None; }
        let mantissa = &text[..e_pos];
        let exp_str = &text[e_pos + 1..];
        let exp: i64 = exp_str.parse().ok()?;
        Some((mantissa, exp))
    }

    /// Parse mantissa as f64, allowing negative.
    fn mantissa_f64(s: &str) -> Option<f64> {
        s.parse::<f64>().ok()
    }

    // Returns true if offense (not conforming)
    fn is_offense(&self, mantissa: &str, exp: i64) -> bool {
        match self.style {
            EnforcedStyle::Scientific => {
                let m = match Self::mantissa_f64(mantissa) {
                    Some(v) => v,
                    None => return false,
                };
                let abs = m.abs();
                !(abs >= 1.0 && abs < 10.0)
            }
            EnforcedStyle::Engineering => {
                let m = match Self::mantissa_f64(mantissa) {
                    Some(v) => v,
                    None => return false,
                };
                let abs = m.abs();
                // exponent divisible by 3, mantissa >= 0.1 and < 1000
                exp % 3 != 0 || !(abs >= 0.1 && abs < 1000.0)
            }
            EnforcedStyle::Integral => {
                // mantissa must be integer without trailing zero
                // has decimal point → offense
                if mantissa.contains('.') { return true; }
                // ends with 0 → offense (trailing zero)
                let clean = mantissa.trim_start_matches('-');
                clean.ends_with('0') && clean.len() > 1
            }
        }
    }

    fn message(&self) -> &'static str {
        match self.style {
            EnforcedStyle::Scientific => "Use a mantissa >= 1 and < 10.",
            EnforcedStyle::Engineering => "Use an exponent divisible by 3 and a mantissa >= 0.1 and < 1000.",
            EnforcedStyle::Integral => "Use an integer as mantissa, without trailing zero.",
        }
    }
}

impl Cop for ExponentialNotation {
    fn name(&self) -> &'static str {
        "Style/ExponentialNotation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ExpNotationVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        use ruby_prism::Visit;
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct ExpNotationVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
}

impl<'a> ruby_prism::Visit<'a> for ExpNotationVisitor<'a> {
    fn visit_float_node(&mut self, node: &ruby_prism::FloatNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let text = &self.ctx.source[start..end];
        self.check_text(text, start, end);
    }

    fn visit_integer_node(&mut self, node: &ruby_prism::IntegerNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let text = &self.ctx.source[start..end];
        self.check_text(text, start, end);
    }
}

impl<'a> ExpNotationVisitor<'a> {
    fn check_text(&mut self, text: &str, start: usize, end: usize) {
        // Remove underscores (Ruby allows 1_000e3)
        let clean: String = text.replace('_', "");
        let (mantissa, exp) = match ExponentialNotation::parse_exponential(&clean) {
            Some(x) => x,
            None => return,
        };
        let cop = ExponentialNotation::new(self.style);
        if cop.is_offense(mantissa, exp) {
            self.offenses.push(
                self.ctx.offense_with_range(
                    "Style/ExponentialNotation",
                    cop.message(),
                    Severity::Convention,
                    start,
                    end,
                )
            );
        }
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/ExponentialNotation", |cfg| {
    let c: Cfg = cfg.typed("Style/ExponentialNotation");
    let style = match c.enforced_style.as_str() {
        "engineering" => EnforcedStyle::Engineering,
        "integral" => EnforcedStyle::Integral,
        _ => EnforcedStyle::Scientific,
    };
    Some(Box::new(ExponentialNotation::new(style)))
});
