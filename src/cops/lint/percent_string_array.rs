//! Lint/PercentStringArray cop — flags quotes/commas inside `%w` / `%W`.
//!
//! Translates RuboCop's
//! `lib/rubocop/cop/lint/percent_string_array.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::percent_literal;
use crate::offense::{Correction, Edit, Offense, Severity};

const MSG: &str = "Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.";

#[derive(Default)]
pub struct PercentStringArray;

impl PercentStringArray {
    pub fn new() -> Self {
        Self
    }
}

/// True when `literal` (after scrub) contains at least one alphanumeric byte.
/// Mirrors RuboCop's `next if literal.gsub(/[^[[:alnum:]]]/, '').empty?` guard.
fn has_alnum(literal: &str) -> bool {
    literal.bytes().any(|b| b.is_ascii_alphanumeric())
}

fn is_quote_or_comma_violation(literal: &str) -> bool {
    literal.ends_with(',')
        || (literal.starts_with('\'') && literal.ends_with('\'') && literal.len() >= 2)
        || (literal.starts_with('"') && literal.ends_with('"') && literal.len() >= 2)
}

impl Cop for PercentStringArray {
    fn name(&self) -> &'static str {
        "Lint/PercentStringArray"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(open_loc) = node.opening_loc() else { return vec![] };
        let open_start = open_loc.start_offset();
        let open_end = open_loc.end_offset();
        let open_src = &ctx.source[open_start..open_end];
        let Some(ty) = percent_literal::percent_type(open_src) else { return vec![] };
        if ty != "%w" && ty != "%W" {
            return vec![];
        }

        let values: Vec<_> = node.elements().iter().collect();
        let slices: Vec<&str> = values
            .iter()
            .map(|v| {
                let loc = v.location();
                &ctx.source[loc.start_offset()..loc.end_offset()]
            })
            .collect();

        let flagged = slices
            .iter()
            .any(|s| has_alnum(s) && is_quote_or_comma_violation(s));
        if !flagged {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        // Build autocorrect edits: strip leading/trailing quote + trailing
        // comma from each token (per RuboCop's TRAILING_QUOTE / LEADING_QUOTE).
        let mut edits = Vec::new();
        for (v, s) in values.iter().zip(slices.iter()) {
            let loc = v.location();
            let vs = loc.start_offset();
            let ve = loc.end_offset();
            let bytes = s.as_bytes();

            // TRAILING_QUOTE: /['"]?,?$/ — match length is 0..=2 chars.
            let mut trail_len = 0usize;
            let n = bytes.len();
            if n > 0 && bytes[n - 1] == b',' {
                trail_len += 1;
                if n >= 2 && (bytes[n - 2] == b'\'' || bytes[n - 2] == b'"') {
                    trail_len += 1;
                }
            } else if n > 0 && (bytes[n - 1] == b'\'' || bytes[n - 1] == b'"') {
                trail_len += 1;
            }
            if trail_len > 0 {
                edits.push(Edit {
                    start_offset: ve - trail_len,
                    end_offset: ve,
                    replacement: String::new(),
                });
            }

            // LEADING_QUOTE: /^['"]/
            if !bytes.is_empty() && (bytes[0] == b'\'' || bytes[0] == b'"') {
                edits.push(Edit {
                    start_offset: vs,
                    end_offset: vs + 1,
                    replacement: String::new(),
                });
            }
        }

        let offense = ctx
            .offense_with_range(self.name(), MSG, self.severity(), start, end)
            .with_correction(Correction { edits });
        vec![offense]
    }
}
