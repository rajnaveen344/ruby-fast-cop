//! Lint/InterpolationCheck - Warn about #{} in single-quoted strings.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str =
    "Interpolation in single quoted string detected. Use double quoted strings if you need interpolation.";

#[derive(Default)]
pub struct InterpolationCheck;

impl InterpolationCheck {
    pub fn new() -> Self { Self }
}

impl Cop for InterpolationCheck {
    fn name(&self) -> &'static str { "Lint/InterpolationCheck" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        self.check_string_node(node);
        ruby_prism::visit_string_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn check_string_node(&mut self, node: &ruby_prism::StringNode) {
        let loc = node.location();
        let source = match self.ctx.source.get(loc.start_offset()..loc.end_offset()) {
            Some(s) => s,
            None => return,
        };

        // Must start with single-quote
        if !source.starts_with('\'') {
            return;
        }

        // Must have #{...} pattern (not escaped \#{)
        if !has_interpolation_pattern(source) {
            return;
        }

        // Must have closing loc (not heredoc style)
        if node.opening_loc().is_none() || node.closing_loc().is_none() {
            return;
        }

        // Validate: would parsing as double-quoted produce a dstr?
        if !valid_interpolation(source) {
            return;
        }

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/InterpolationCheck",
            MSG,
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        ));
    }
}

/// Check for unescaped #{...} pattern in source text.
fn has_interpolation_pattern(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped char
            continue;
        }
        if bytes[i] == b'#' && bytes[i + 1] == b'{' {
            return true;
        }
        i += 1;
    }
    false
}

/// Simple validation: check the interpolation braces are balanced enough
/// to not break a %{...} literal (which needs balanced braces).
/// RuboCop does a full re-parse; we do a simpler check.
fn valid_interpolation(source: &str) -> bool {
    // Check that #{...} content doesn't have invalid syntax indicators
    // like format specifiers %<...>s that would break parsing.
    // Also check that converting to %{} would have balanced braces.

    // Find all #{...} blocks
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut has_double_quote = false;
    let mut has_unbalanced_brace = false;

    while i < bytes.len() {
        if bytes[i] == b'"' {
            has_double_quote = true;
        }
        i += 1;
    }

    // If source contains double quotes, we'd use %{} literal.
    // Check brace balance in that case.
    if has_double_quote {
        // Scan for } outside interpolation — unbalanced } breaks %{}
        let mut depth = 0i32;
        let mut in_interpolation = false;
        let mut interp_depth = 0i32;
        let mut i = 0usize;
        let bytes = source.as_bytes();
        while i < bytes.len() {
            if !in_interpolation {
                if i + 1 < bytes.len() && bytes[i] == b'#' && bytes[i+1] == b'{' {
                    in_interpolation = true;
                    interp_depth = 0;
                    i += 2;
                    continue;
                }
                if bytes[i] == b'}' {
                    depth -= 1;
                    has_unbalanced_brace = depth < 0;
                }
                if bytes[i] == b'{' {
                    depth += 1;
                }
            } else {
                if bytes[i] == b'{' { interp_depth += 1; }
                if bytes[i] == b'}' {
                    if interp_depth == 0 {
                        in_interpolation = false;
                    } else {
                        interp_depth -= 1;
                    }
                }
            }
            i += 1;
        }
        if has_unbalanced_brace { return false; }
    }

    // Check for format specifiers like %<...>s inside interpolation that would break things
    // This is a heuristic: look for %< inside the string
    // RuboCop re-parses; we check for known-bad patterns
    let content = &source[1..source.len().saturating_sub(1)]; // strip quotes
    // Find #{...} blocks and check their content
    let mut i = 0usize;
    let cbytes = content.as_bytes();
    while i + 1 < cbytes.len() {
        if cbytes[i] == b'#' && cbytes[i+1] == b'{' {
            i += 2;
            let start = i;
            let mut depth = 1i32;
            while i < cbytes.len() && depth > 0 {
                if cbytes[i] == b'{' { depth += 1; }
                if cbytes[i] == b'}' { depth -= 1; }
                i += 1;
            }
            let interp_content = &content[start..i.saturating_sub(1)];
            // If interpolation content has %< style format spec, likely invalid
            if interp_content.contains("%<") {
                return false;
            }
        } else {
            i += 1;
        }
    }

    true
}

crate::register_cop!("Lint/InterpolationCheck", |_cfg| Some(Box::new(InterpolationCheck::new())));
