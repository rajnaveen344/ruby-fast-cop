//! Layout/SpaceInsideArrayPercentLiteral cop
//!
//! Checks for unnecessary additional spaces inside array percent literals (%i/%w).
//! Mirrors RuboCop's approach: visit ArrayNode, check opening_loc for %w/%W/%i/%I,
//! then regex-match the content range for multiple consecutive spaces.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;
use std::sync::LazyLock;

const MSG: &str = "Use only a single space inside array percent literal.";

// RuboCop: /(?:[\S&&[^\\]](?:\\ )*)( {2,})(?=\S)/
// Rust regex doesn't support lookahead, so we match spaces after non-whitespace
// and manually verify the next char is non-whitespace.
static MULTIPLE_SPACES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\s\\](?:\\ )*( {2,})").unwrap());

pub struct SpaceInsideArrayPercentLiteral;

impl SpaceInsideArrayPercentLiteral {
    pub fn new() -> Self {
        Self
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visitor<'_> {
    fn check_percent_literal(&mut self, node: &ruby_prism::ArrayNode) {
        let open = match node.opening_loc() {
            Some(loc) => loc,
            None => return,
        };
        let close = match node.closing_loc() {
            Some(loc) => loc,
            None => return,
        };

        // Only array percent literals: %w, %W, %i, %I
        let open_src = String::from_utf8_lossy(open.as_slice());
        if !matches!(open_src.as_ref(), s if s.starts_with("%w") || s.starts_with("%W") || s.starts_with("%i") || s.starts_with("%I")) {
            return;
        }

        let content_start = open.end_offset();
        let content_end = close.start_offset();
        let content = &self.ctx.source[content_start..content_end];

        // Skip multiline literals (RuboCop behavior)
        if content.contains('\n') {
            return;
        }

        // Find all multiple-space matches within the content
        for cap in MULTIPLE_SPACES_RE.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                // Manual lookahead: next char after spaces must be non-whitespace
                let after = &content[m.end()..];
                if after.is_empty() || after.starts_with(|c: char| c.is_whitespace()) {
                    continue;
                }
                let abs_start = content_start + m.start();
                let abs_end = content_start + m.end();
                self.offenses.push(
                    self.ctx
                        .offense_with_range(
                            "Layout/SpaceInsideArrayPercentLiteral",
                            MSG,
                            Severity::Convention,
                            abs_start,
                            abs_end,
                        )
                        .with_correction(Correction::replace(abs_start, abs_end, " ".to_string())),
                );
            }
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        self.check_percent_literal(node);
        ruby_prism::visit_array_node(self, node);
    }
}

impl Cop for SpaceInsideArrayPercentLiteral {
    fn name(&self) -> &'static str {
        "Layout/SpaceInsideArrayPercentLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
