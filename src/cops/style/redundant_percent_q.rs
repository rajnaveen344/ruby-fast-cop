//! Style/RedundantPercentQ — Flag redundant %q/%Q string literals.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_percent_q.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::percent_literal;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG_Q: &str = "Use `%q` only for strings that contain both single quotes and double quotes.";
const MSG_BIG_Q: &str = "Use `%Q` only for strings that contain both single quotes and double quotes, or for dynamic strings that contain double quotes.";

#[derive(Default)]
pub struct RedundantPercentQ;

impl RedundantPercentQ {
    pub fn new() -> Self { Self }

    /// Check whether string body contains both ' and "
    fn has_both_quotes(body: &str) -> bool {
        body.contains('\'') && body.contains('"')
    }

    /// Convert %q body to single-quoted string (escape single quotes).
    fn to_single_quote(body: &str) -> String {
        format!("'{}'", body.replace('\'', "\\'"))
    }

    /// Convert %q body to double-quoted string (escape double quotes).
    fn to_double_quote(body: &str) -> String {
        format!("\"{}\"", body.replace('"', "\\\""))
    }

    /// Decide correction for %q string:
    /// - Only double quotes → use single quotes: `%q("hi")` → `'"hi"'`
    /// - Only single quotes → use double quotes: `%q('hi')` → `"'hi'"`
    /// - No quotes → use single quotes: `%q(hi)` → `'hi'`
    fn q_correction(body: &str) -> Option<String> {
        let has_single = body.contains('\'');
        let has_double = body.contains('"');
        if has_single && has_double { return None; } // keep %q
        if has_single {
            // Has single quotes, no double → use double quotes
            Some(format!("\"{}\"", body))
        } else {
            // Has double quotes or neither → use single quotes
            Some(format!("'{}'", body))
        }
    }

    /// Decide correction for %Q string (static).
    fn big_q_correction_static(body: &str) -> Option<String> {
        let has_single = body.contains('\'');
        let has_double = body.contains('"');
        if has_single && has_double { return None; }
        if has_double {
            // Use single quotes
            Some(format!("'{}'", body))
        } else {
            Some(format!("\"{}\"", body))
        }
    }
}

impl Cop for RedundantPercentQ {
    fn name(&self) -> &'static str {
        "Style/RedundantPercentQ"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RedundantPercentQVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct RedundantPercentQVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RedundantPercentQVisitor<'a> {
    fn check_static_string(&mut self, open_start: usize, open_end: usize, node_end: usize) {
        let open_src = &self.ctx.source[open_start..open_end];
        let Some(ty) = percent_literal::percent_type(open_src) else { return };
        if ty != "%q" && ty != "%Q" { return; }

        // Get body: between opening delimiter end and closing delimiter (last char)
        if node_end < open_end + 1 { return; }
        let body = &self.ctx.source[open_end..node_end - 1];

        // Skip if body has string interpolation start (for %q only — %Q is dynamic)
        if ty == "%q" {
            // %q: only flag if no escaped non-backslash chars (those need %q to work)
            // Escaped backslash `\\` is fine — flag those. Only skip if \x (x != \).
            if has_non_backslash_escape(body) { return; }

            // Skip if body has interpolation literal `#{`
            if body.contains("#{") { return; }

            let has_both = body.contains('\'') && body.contains('"');
            if has_both { return; }

            // Flag
            let correction = RedundantPercentQ::q_correction(body);
            let offense = self.ctx.offense_with_range(
                "Style/RedundantPercentQ", MSG_Q, Severity::Convention, open_start, node_end,
            );
            let offense = if let Some(corr) = correction {
                offense.with_correction(Correction::replace(open_start, node_end, corr))
            } else {
                offense
            };
            self.offenses.push(offense);
        } else {
            // %Q static (no interpolation)
            // Has both quotes → keep
            let has_both = body.contains('\'') && body.contains('"');
            if has_both { return; }

            // Has escaped special char (not backslash) → keep
            // e.g. \t, \n → keep
            let has_escape_seq = has_special_escape(body);
            if has_escape_seq { return; }

            // Has double quotes? → use single-quote wrapping
            let has_double = body.contains('"');

            let correction = RedundantPercentQ::big_q_correction_static(body);
            let offense = self.ctx.offense_with_range(
                "Style/RedundantPercentQ", MSG_BIG_Q, Severity::Convention, open_start, node_end,
            );
            let offense = if let Some(corr) = correction {
                offense.with_correction(Correction::replace(open_start, node_end, corr))
            } else {
                offense
            };
            self.offenses.push(offense);
        }
    }

    fn check_dynamic_string(&mut self, open_start: usize, open_end: usize, node_end: usize) {
        let open_src = &self.ctx.source[open_start..open_end];
        let Some(ty) = percent_literal::percent_type(open_src) else { return };
        if ty != "%Q" { return; }

        let body = &self.ctx.source[open_end..node_end - 1];

        // Has both quotes → keep
        if body.contains('\'') && body.contains('"') { return; }

        // Has double quotes → keep
        if body.contains('"') { return; }

        // Flag: use "..."
        let correction = format!("\"{}\"", body);
        let offense = self.ctx.offense_with_range(
            "Style/RedundantPercentQ", MSG_BIG_Q, Severity::Convention, open_start, node_end,
        ).with_correction(Correction::replace(open_start, node_end, correction));
        self.offenses.push(offense);
    }
}

/// Returns true if string body contains escape sequences that are not just `\\` (escaped backslash).
/// e.g. `\t`, `\n`, `\'`, `\"` → true; `\\` → false; `\\\\` → false.
fn has_non_backslash_escape(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            if bytes[i + 1] == b'\\' {
                i += 2; // skip \\, it's an escaped backslash — OK
            } else {
                return true; // \x where x != \ → non-backslash escape
            }
        } else {
            i += 1;
        }
    }
    false
}

/// Returns true if string body contains escape sequences like \t, \n (but not \\)
fn has_special_escape(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            let next = bytes[i + 1];
            if next != b'\\' {
                return true;
            }
            i += 2; // skip \\
        } else {
            i += 1;
        }
    }
    false
}

impl<'a> Visit<'_> for RedundantPercentQVisitor<'a> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if let Some(open_loc) = node.opening_loc() {
            let open_start = open_loc.start_offset();
            let open_end = open_loc.end_offset();
            let node_end = node.location().end_offset();
            self.check_static_string(open_start, open_end, node_end);
        }
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if let Some(open_loc) = node.opening_loc() {
            let open_start = open_loc.start_offset();
            let open_end = open_loc.end_offset();
            let node_end = node.location().end_offset();
            self.check_dynamic_string(open_start, open_end, node_end);
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

crate::register_cop!("Style/RedundantPercentQ", |_cfg| {
    Some(Box::new(RedundantPercentQ::new()))
});
