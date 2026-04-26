//! Style/MultilineMethodSignature cop
//!
//! Flags method signatures that span multiple lines.
//! Ported from `rubocop/cop/style/multiline_method_signature.rb`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Style/MultilineMethodSignature";
const MSG: &str = "Avoid multi-line method signatures.";

pub struct MultilineMethodSignature {
    max_line_length: Option<usize>,
}

impl MultilineMethodSignature {
    pub fn new(max_line_length: Option<usize>) -> Self {
        Self { max_line_length }
    }
}

impl Default for MultilineMethodSignature {
    fn default() -> Self {
        Self::new(Some(80))
    }
}

impl Cop for MultilineMethodSignature {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        // Need at least one parameter (RuboCop: `node.arguments?`)
        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![],
        };
        // Skip if parameters node has no actual params (block-only/etc) — Prism still
        // provides a ParametersNode if there's a `(` even when empty; but in such a case
        // there are no args sources to be multiline anyway, so the line check below filters.

        // Need a `(` in the signature, otherwise nothing to flag.
        let lparen = match node.lparen_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let rparen = match node.rparen_loc() {
            Some(r) => r,
            None => return vec![],
        };

        let params_loc = params.location();
        // Must have actual parameters (skip when the `(` is followed only by a `)`).
        if params_loc.start_offset() >= params_loc.end_offset() {
            return vec![];
        }

        // RuboCop:
        //   opening_line = node.first_line
        //   closing_line = node.arguments.last_line
        //   return if opening_line == closing_line
        // We use lparen line as opening and rparen line as closing — Prism's def first
        // line is the def keyword line. Use def_keyword line so a multi-line `def\nfoo`
        // is also caught (test cases 3 & 4).
        let def_kw = node.def_keyword_loc();
        let opening_line = ctx.line_of(def_kw.start_offset());
        let closing_line = ctx.line_of(rparen.start_offset());
        if opening_line == closing_line {
            return vec![];
        }

        // correction_exceeds_max_line_length?
        if let Some(max) = self.max_line_length {
            // indentation_width = leading spaces of first source line of node
            let indent = ctx.indentation_of(def_kw.start_offset());
            // definition_width = chars from node start to arguments end
            let def_width = rparen.end_offset().saturating_sub(def_kw.start_offset());
            if indent + def_width > max {
                return vec![];
            }
        }

        // Offense range: from def keyword start through end of arguments line on line 1.
        // Location::from_offsets widens multi-line ranges to col-at-newline of start_line.
        let start = def_kw.start_offset();
        let end = rparen.end_offset();
        vec![ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, start, end)]
    }
}

crate::register_cop!("Style/MultilineMethodSignature", |cfg| {
    let max_line_length = if cfg.is_cop_enabled("Layout/LineLength") {
        cfg.get_cop_config("Layout/LineLength")
            .and_then(|c| c.max)
            .map(|m| m as usize)
    } else {
        None
    };
    Some(Box::new(MultilineMethodSignature::new(max_line_length)))
});
