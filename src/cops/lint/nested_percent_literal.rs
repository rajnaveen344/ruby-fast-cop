//! Lint/NestedPercentLiteral cop — flags nested `%w(%w(...))`, `%i(%i(...))`, etc.
//!
//! Translates RuboCop's
//! `lib/rubocop/cop/lint/nested_percent_literal.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::percent_literal;
use crate::offense::{Offense, Severity};

const MSG: &str =
    "Within percent literals, nested percent literals do not function and may be unwanted in the result.";

/// The percent-literal types RuboCop considers (from `PreferredDelimiters::PERCENT_LITERAL_TYPES`).
const PERCENT_TYPES: &[&str] = &["%w", "%W", "%i", "%I", "%q", "%Q", "%r", "%s", "%x"];

#[derive(Default)]
pub struct NestedPercentLiteral;

impl NestedPercentLiteral {
    pub fn new() -> Self {
        Self
    }
}

/// True if `text` matches `/\A(%w|%W|%i|%I|%q|%Q|%r|%s|%x)\W/` — i.e. starts
/// with one of the percent-literal prefixes followed by a non-word character.
fn contains_nested_percent(text: &str) -> bool {
    for ty in PERCENT_TYPES {
        if text.len() > ty.len() && text.starts_with(ty) {
            let next = text.as_bytes()[ty.len()];
            let is_word = next.is_ascii_alphanumeric() || next == b'_';
            if !is_word {
                return true;
            }
        }
    }
    false
}

impl Cop for NestedPercentLiteral {
    fn name(&self) -> &'static str {
        "Lint/NestedPercentLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(open_loc) = node.opening_loc() else { return vec![] };
        let open_src = &ctx.source[open_loc.start_offset()..open_loc.end_offset()];
        let Some(ty) = percent_literal::percent_type(open_src) else { return vec![] };
        if !PERCENT_TYPES.contains(&ty) {
            return vec![];
        }

        let contains_nested = node.elements().iter().any(|child| {
            let loc = child.location();
            let text = &ctx.source[loc.start_offset()..loc.end_offset()];
            contains_nested_percent(text)
        });

        if !contains_nested {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)]
    }
}

crate::register_cop!("Lint/NestedPercentLiteral", |_cfg| {
    Some(Box::new(NestedPercentLiteral::new()))
});
