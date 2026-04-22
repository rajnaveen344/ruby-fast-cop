//! Lint/MixedRegexpCaptureTypes - Do not mix named and numbered captures.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Do not mix named captures and numbered captures in a Regexp literal.";

#[derive(Default)]
pub struct MixedRegexpCaptureTypes;

impl MixedRegexpCaptureTypes {
    pub fn new() -> Self { Self }
}

impl Cop for MixedRegexpCaptureTypes {
    fn name(&self) -> &'static str { "Lint/MixedRegexpCaptureTypes" }
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
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let source = String::from_utf8_lossy(node.unescaped());
        if has_named_capture(&source) && has_numbered_capture(&source) {
            let loc = node.location();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/MixedRegexpCaptureTypes",
                MSG,
                Severity::Warning,
                loc.start_offset(),
                loc.end_offset(),
            ));
        }
        ruby_prism::visit_regular_expression_node(self, node);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        // RuboCop skips interpolated regexps (node.interpolation? returns true)
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }
}

/// Check for named captures: (?<name>...) or (?'name'...)
fn has_named_capture(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'(' && bytes[i + 1] == b'?' {
            if i + 2 < bytes.len() && bytes[i + 2] == b'<' {
                // (?< — but not (?<= or (?<!  (lookbehind)
                if i + 3 < bytes.len() && bytes[i + 3] != b'=' && bytes[i + 3] != b'!' {
                    return true;
                }
            }
            if i + 2 < bytes.len() && bytes[i + 2] == b'\'' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Check for numbered captures: plain (...)  not (?:...) not (?<...) not (?= etc.
fn has_numbered_capture(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            if i + 1 >= bytes.len() {
                // bare ( at end — treat as numbered
                return true;
            }
            let next = bytes[i + 1];
            if next != b'?' {
                // plain capturing group
                return true;
            }
            // (?... — non-capturing or lookahead etc.
        }
        i += 1;
    }
    false
}

crate::register_cop!("Lint/MixedRegexpCaptureTypes", |_cfg| Some(Box::new(MixedRegexpCaptureTypes::new())));
