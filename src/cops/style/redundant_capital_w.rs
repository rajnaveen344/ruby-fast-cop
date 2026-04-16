//! Style/RedundantCapitalW cop — flags `%W` without interpolation → use `%w`.
//!
//! Translates RuboCop's
//! `lib/rubocop/cop/style/redundant_capital_w.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::percent_literal;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Do not use `%W` unless interpolation is needed. If not, use `%w`.";

#[derive(Default)]
pub struct RedundantCapitalW;

impl RedundantCapitalW {
    pub fn new() -> Self {
        Self
    }
}

/// Mirrors RuboCop's `Util.double_quotes_required?`:
/// `/'|(?<! \\) \\{2}* \\ (?![\\"])/x` — true when `src` contains a single
/// quote OR a lone backslash (not part of a doubled `\\` run) not followed by
/// `\` or `"`.
fn double_quotes_required(src: &str) -> bool {
    let bytes = src.as_bytes();
    if bytes.contains(&b'\'') {
        return true;
    }
    // Scan for a backslash preceded by an EVEN number of `\` and followed by
    // something other than `\` or `"`.
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        // Count run of backslashes starting at i.
        let mut j = i;
        while j < bytes.len() && bytes[j] == b'\\' {
            j += 1;
        }
        // Run length
        let run = j - i;
        // We want a run of ODD length (the final unpaired `\`) followed by a
        // char that's not `\` (it isn't by construction) and not `"`.
        if run % 2 == 1 {
            // The lone trailing backslash is at position j-1; next char is
            // bytes[j] (or end).
            if j >= bytes.len() {
                return true; // dangling backslash counts
            }
            if bytes[j] != b'"' {
                return true;
            }
        }
        i = j;
    }
    false
}

fn requires_interpolation(node: &ruby_prism::ArrayNode, source: &str) -> bool {
    node.elements().iter().any(|child| match child {
        Node::InterpolatedStringNode { .. } => true,
        _ => {
            let loc = child.location();
            let s = &source[loc.start_offset()..loc.end_offset()];
            double_quotes_required(s)
        }
    })
}

impl Cop for RedundantCapitalW {
    fn name(&self) -> &'static str {
        "Style/RedundantCapitalW"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(open_loc) = node.opening_loc() else { return vec![] };
        let open_start = open_loc.start_offset();
        let open_end = open_loc.end_offset();
        let open_src = &ctx.source[open_start..open_end];
        let Some(ty) = percent_literal::percent_type(open_src) else { return vec![] };
        if ty != "%W" {
            return vec![];
        }
        if requires_interpolation(node, ctx.source) {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        // Replace `W` in the opening token with `w`.
        let new_open = open_src.replacen('W', "w", 1);
        let offense = ctx
            .offense_with_range(self.name(), MSG, self.severity(), start, end)
            .with_correction(Correction::replace(open_start, open_end, new_open));
        vec![offense]
    }
}
