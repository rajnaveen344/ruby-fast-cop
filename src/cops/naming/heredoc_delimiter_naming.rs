//! Naming/HeredocDelimiterNaming cop
//! Checks that heredoc delimiters are meaningful (not in ForbiddenDelimiters list,
//! not empty, and consist of word characters).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/naming/heredoc_delimiter_naming.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct Cfg {
    #[serde(default)]
    forbidden_delimiters: Vec<String>,
}

pub struct HeredocDelimiterNaming {
    forbidden_delimiters: Vec<String>,
}

impl HeredocDelimiterNaming {
    pub fn new(forbidden_delimiters: Vec<String>) -> Self {
        Self { forbidden_delimiters }
    }

    fn is_meaningful_delimiter(&self, delimiter: &str) -> bool {
        if delimiter.is_empty() {
            return false;
        }
        let is_word = delimiter.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !is_word {
            return false;
        }
        !self.forbidden_delimiters.iter().any(|f| f == delimiter)
    }
}

impl Cop for HeredocDelimiterNaming {
    fn name(&self) -> &'static str { "Naming/HeredocDelimiterNaming" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let mut offenses = Vec::new();

        // Match heredoc openings: <<[~-]?['"`]?DELIM['"`]?
        // We need to match non-word delimiters too (e.g. +, empty)
        // Pattern: <<[~-]?(['"`]?)([^'"`\n]*)(['"`]?)
        let heredoc_re = Regex::new(r#"<<([~-])?(['"`])(.*?)(['"`])"#).unwrap();
        let heredoc_re_bare = Regex::new(r#"<<([~-])?([A-Za-z_]\w*)"#).unwrap();

        // Handle quoted heredocs (includes empty and non-word delimiters)
        for caps in heredoc_re.captures_iter(source) {
            let full_match = caps.get(0).unwrap();
            let open_quote = caps.get(2).unwrap().as_str();
            let delimiter = caps.get(3).unwrap().as_str();
            let close_quote = caps.get(4).unwrap().as_str();

            if open_quote != close_quote {
                continue;
            }

            if self.is_meaningful_delimiter(delimiter) {
                continue;
            }

            // Find the closing delimiter line
            let opening_start = full_match.start();
            let opening_end = full_match.end();
            let msg = "Use meaningful heredoc delimiters.";

            if delimiter.is_empty() {
                // Empty delimiter: offense is on the opening token itself
                offenses.push(ctx.offense_with_range(
                    self.name(), msg, Severity::Convention,
                    opening_start,
                    opening_end,
                ));
                continue;
            }

            // Find the closing delimiter (on its own line)
            let body_start = match source[opening_end..].find('\n') {
                Some(pos) => opening_end + pos + 1,
                None => continue,
            };

            let closing_re = Regex::new(
                &format!(r"(?m)^[ \t]*{}[ \t]*$", regex::escape(delimiter)),
            ).unwrap();

            if let Some(close_match) = closing_re.find(&source[body_start..]) {
                let close_abs_start = body_start + close_match.start();
                let close_text = close_match.as_str();
                // Find the actual delimiter position (skip leading whitespace)
                let indent = close_text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                let delim_start = close_abs_start + indent;
                let delim_end = delim_start + delimiter.len();
                offenses.push(ctx.offense_with_range(
                    self.name(), msg, Severity::Convention,
                    delim_start,
                    delim_end,
                ));
            }
        }

        // Handle bare (unquoted) heredocs with word delimiters that are forbidden/non-meaningful
        for caps in heredoc_re_bare.captures_iter(source) {
            let full_match = caps.get(0).unwrap();
            let delimiter = caps.get(2).unwrap().as_str();

            // Skip if meaningful
            if self.is_meaningful_delimiter(delimiter) {
                continue;
            }

            // This delimiter is forbidden
            let opening_end = full_match.end();
            let msg = "Use meaningful heredoc delimiters.";

            let body_start = match source[opening_end..].find('\n') {
                Some(pos) => opening_end + pos + 1,
                None => continue,
            };

            let closing_re = Regex::new(
                &format!(r"(?m)^[ \t]*{}[ \t]*$", regex::escape(delimiter)),
            ).unwrap();

            if let Some(close_match) = closing_re.find(&source[body_start..]) {
                let close_abs_start = body_start + close_match.start();
                let close_text = close_match.as_str();
                let indent = close_text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                let delim_start = close_abs_start + indent;
                let delim_end = delim_start + delimiter.len();
                offenses.push(ctx.offense_with_range(
                    self.name(), msg, Severity::Convention,
                    delim_start,
                    delim_end,
                ));
            }
        }

        // Sort by line/col
        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

crate::register_cop!("Naming/HeredocDelimiterNaming", |cfg| {
    let c: Cfg = cfg.typed("Naming/HeredocDelimiterNaming");
    Some(Box::new(HeredocDelimiterNaming::new(c.forbidden_delimiters)))
});
