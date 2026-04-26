//! Style/DisableCopsWithinSourceCodeDirective - Detects rubocop:disable/enable directives.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/disable_cops_within_source_code_directive.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use regex::Regex;

const MSG: &str = "RuboCop disable/enable directives are not permitted.";

#[derive(Default)]
pub struct DisableCopsWithinSourceCodeDirective {
    allowed_cops: Vec<String>,
}

impl DisableCopsWithinSourceCodeDirective {
    pub fn new(allowed_cops: Vec<String>) -> Self {
        Self { allowed_cops }
    }
}

impl Cop for DisableCopsWithinSourceCodeDirective {
    fn name(&self) -> &'static str { "Style/DisableCopsWithinSourceCodeDirective" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        // Match `rubocop:disable|enable|todo` followed by cops list.
        // Cops list is everything up to end-of-comment-text (no trailing-comment support here).
        let directive_re = Regex::new(
            r"#\s*rubocop\s*:\s*(?:disable|enable|todo)\b\s*([^\n]*)",
        ).unwrap();

        let mut offenses = Vec::new();
        for c in result.comments() {
            let loc = c.location();
            let text = &ctx.source[loc.start_offset()..loc.end_offset()];
            let Some(caps) = directive_re.captures(text) else { continue; };
            let cops_part = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            // Parse cop list (split on commas). `all` is treated as a single
            // pseudo-cop and is never in allowed_cops.
            let directive_cops: Vec<String> = if cops_part.is_empty() {
                vec![]
            } else {
                cops_part.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            };

            let disallowed: Vec<String> = directive_cops.iter()
                .filter(|c| !self.allowed_cops.iter().any(|a| a == *c))
                .cloned()
                .collect();

            if disallowed.is_empty() {
                continue;
            }

            let any_allowed = !self.allowed_cops.is_empty();
            let message = if any_allowed {
                let formatted = disallowed.iter().map(|c| format!("`{}`", c)).collect::<Vec<_>>().join(", ");
                format!("RuboCop disable/enable directives for {} are not permitted.", formatted)
            } else {
                MSG.to_string()
            };

            // Build correction: if some cops are still permitted, keep only those.
            let replacement = if directive_cops.len() != disallowed.len() {
                // Remove disallowed cops from the original comment text.
                let mut text_str = text.to_string();
                // Remove each disallowed cop name plus optional trailing comma + spaces.
                let union = disallowed.iter().map(|c| regex::escape(c)).collect::<Vec<_>>().join("|");
                let r = Regex::new(&format!(r"(?:{}),?\s*", union)).unwrap();
                text_str = r.replace_all(&text_str, "").to_string();
                // Strip trailing `, ` (RuboCop's `.sub(/,\s*$/, '')`).
                let trail = Regex::new(r",\s*$").unwrap();
                trail.replace(&text_str, "").to_string()
            } else {
                String::new()
            };

            let location = Location::from_offsets(ctx.source, loc.start_offset(), loc.end_offset());
            let mut off = Offense::new(
                "Style/DisableCopsWithinSourceCodeDirective",
                message,
                Severity::Convention,
                location,
                ctx.filename,
            );
            off = off.with_correction(Correction::replace(loc.start_offset(), loc.end_offset(), replacement));
            offenses.push(off);
        }
        offenses
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_cops: Vec<String>,
}

crate::register_cop!("Style/DisableCopsWithinSourceCodeDirective", |cfg| {
    let c: Cfg = cfg.typed("Style/DisableCopsWithinSourceCodeDirective");
    Some(Box::new(DisableCopsWithinSourceCodeDirective::new(c.allowed_cops)))
});
