//! Lint/PercentSymbolArray cop — flags colons/commas inside `%i` / `%I`.
//!
//! Translates RuboCop's
//! `lib/rubocop/cop/lint/percent_symbol_array.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::percent_literal;
use crate::offense::{Correction, Edit, Offense, Severity};

const MSG: &str = "Within `%i`/`%I`, ':' and ',' are unnecessary and may be unwanted in the resulting symbols.";

#[derive(Default)]
pub struct PercentSymbolArray;

impl PercentSymbolArray {
    pub fn new() -> Self {
        Self
    }
}

/// True when `literal` contains at least one alphanumeric byte (mirrors
/// RuboCop's `non_alphanumeric_literal?` negation).
fn has_alnum(literal: &str) -> bool {
    literal.bytes().any(|b| b.is_ascii_alphanumeric())
}

impl Cop for PercentSymbolArray {
    fn name(&self) -> &'static str {
        "Lint/PercentSymbolArray"
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
        if ty != "%i" && ty != "%I" {
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
            .any(|s| has_alnum(s) && (s.starts_with(':') || s.ends_with(',')));
        if !flagged {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        let mut edits = Vec::new();
        for (v, s) in values.iter().zip(slices.iter()) {
            let loc = v.location();
            let vs = loc.start_offset();
            let ve = loc.end_offset();
            let bytes = s.as_bytes();
            if !bytes.is_empty() && bytes[bytes.len() - 1] == b',' {
                edits.push(Edit {
                    start_offset: ve - 1,
                    end_offset: ve,
                    replacement: String::new(),
                });
            }
            if !bytes.is_empty() && bytes[0] == b':' {
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

crate::register_cop!("Lint/PercentSymbolArray", |_cfg| {
    Some(Box::new(PercentSymbolArray::new()))
});
