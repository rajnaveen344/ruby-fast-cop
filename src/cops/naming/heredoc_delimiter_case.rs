//! Naming/HeredocDelimiterCase cop
//!
//! Checks heredoc delimiter case (uppercase or lowercase).
//! The offense is reported at the closing delimiter.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/naming/heredoc_delimiter_case.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use regex::Regex;

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Uppercase,
    Lowercase,
}

pub struct HeredocDelimiterCase {
    style: Style,
}

impl Default for HeredocDelimiterCase {
    fn default() -> Self {
        Self { style: Style::Uppercase }
    }
}

impl HeredocDelimiterCase {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(style_str: &str) -> Self {
        Self {
            style: if style_str == "lowercase" { Style::Lowercase } else { Style::Uppercase },
        }
    }

    fn check_source(&self, source: &str, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        // Match heredoc openings: <<[-~]? then optional quote, word chars, optional quote
        let heredoc_re = Regex::new(r#"<<([-~]?)(['"`]?)([A-Za-z_]\w*)(['"`]?)"#).unwrap();

        for mat in heredoc_re.find_iter(source) {
            let opening_str = mat.as_str();
            let opening_end = mat.end();

            let caps = heredoc_re.captures(opening_str).unwrap();
            let open_quote = caps.get(2).map_or("", |m| m.as_str());
            let close_quote = caps.get(4).map_or("", |m| m.as_str());
            // Validate matching quotes
            if open_quote != close_quote {
                continue;
            }

            let delimiter = caps.get(3).unwrap().as_str();

            // Skip if delimiter has no alpha chars (all digits/underscores)
            if !delimiter.bytes().any(|b| b.is_ascii_alphabetic()) {
                continue;
            }

            // Check if delimiter is already correct case
            let delimiter_ok = match self.style {
                Style::Uppercase => delimiter.bytes().all(|b| !b.is_ascii_lowercase()),
                Style::Lowercase => delimiter.bytes().all(|b| !b.is_ascii_uppercase()),
            };
            if delimiter_ok {
                continue;
            }

            // Find the closing delimiter: it must appear alone on its own line
            let body_start = match source[opening_end..].find('\n') {
                Some(pos) => opening_end + pos + 1,
                None => continue,
            };
            if body_start >= source.len() {
                continue;
            }

            // Closing delimiter: line that is exactly the delimiter (possibly surrounded by whitespace)
            // Per RuboCop, the closing delimiter is exactly the unquoted identifier text, at start of line
            let closing_re = Regex::new(
                &format!(r"(?m)^{}$", regex::escape(delimiter)),
            )
            .unwrap();

            let closing_match = match closing_re.find(&source[body_start..]) {
                Some(m) => m,
                None => continue,
            };

            let closing_start = body_start + closing_match.start();
            let closing_end = body_start + closing_match.end();

            let msg = match self.style {
                Style::Uppercase => "Use uppercase heredoc delimiters.",
                Style::Lowercase => "Use lowercase heredoc delimiters.",
            };

            let offense = ctx.offense_with_range(
                "Naming/HeredocDelimiterCase",
                msg,
                Severity::Convention,
                closing_start,
                closing_end,
            );

            // Correction: fix both the opening delimiter and the closing delimiter
            let corrected_delimiter = match self.style {
                Style::Uppercase => delimiter.to_uppercase(),
                Style::Lowercase => delimiter.to_lowercase(),
            };

            // Build opening replacement: replace just the delimiter portion in the opening
            // The opening is e.g. `<<-sql` or `<<~'Sql'`
            let delimiter_in_opening_start = mat.start()
                + caps.get(1).map_or(2, |_| 3)  // "<<" + maybe "-"/"~"
                + open_quote.len();
            let delimiter_in_opening_end = delimiter_in_opening_start + delimiter.len();

            let correction = Correction {
                edits: vec![
                    // Fix closing delimiter
                    Edit {
                        start_offset: closing_start,
                        end_offset: closing_end,
                        replacement: corrected_delimiter.clone(),
                    },
                    // Fix opening delimiter identifier
                    Edit {
                        start_offset: delimiter_in_opening_start,
                        end_offset: delimiter_in_opening_end,
                        replacement: corrected_delimiter,
                    },
                ],
            };

            offenses.push(offense.with_correction(correction));
        }

        offenses
    }
}

impl Cop for HeredocDelimiterCase {
    fn name(&self) -> &'static str {
        "Naming/HeredocDelimiterCase"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let _ = node;
        self.check_source(ctx.source, ctx)
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

crate::register_cop!("Naming/HeredocDelimiterCase", |cfg| {
    let c: Cfg = cfg.typed("Naming/HeredocDelimiterCase");
    Some(Box::new(HeredocDelimiterCase::with_style(&c.enforced_style)))
});
