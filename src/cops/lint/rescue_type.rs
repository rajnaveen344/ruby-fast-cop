//! Lint/RescueType - Checks for `rescue` with a non-exception type argument.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/rescue_type.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct RescueType;

impl RescueType {
    pub fn new() -> Self {
        Self
    }

    /// Check if a node is an invalid type for rescue (not a class/constant reference).
    fn is_invalid_type(node: &Node) -> bool {
        matches!(
            node,
            Node::ArrayNode { .. }
                | Node::InterpolatedStringNode { .. }
                | Node::FloatNode { .. }
                | Node::HashNode { .. }
                | Node::NilNode { .. }
                | Node::IntegerNode { .. }
                | Node::StringNode { .. }
                | Node::SymbolNode { .. }
        )
    }
}

struct RescueTypeVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for RescueTypeVisitor<'_> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        let exceptions: Vec<Node> = node.exceptions().iter().collect();
        if exceptions.is_empty() {
            ruby_prism::visit_rescue_node(self, node);
            return;
        }

        let invalid: Vec<&Node> = exceptions.iter().filter(|e| RescueType::is_invalid_type(e)).collect();
        if invalid.is_empty() {
            ruby_prism::visit_rescue_node(self, node);
            return;
        }

        // Build the invalid exceptions source text
        let invalid_sources: Vec<&str> = invalid
            .iter()
            .map(|n| {
                let loc = n.location();
                &self.ctx.source[loc.start_offset()..loc.end_offset()]
            })
            .collect();

        let message = format!(
            "Rescuing from `{}` will raise a `TypeError` instead of catching the actual exception.",
            invalid_sources.join(", ")
        );

        // Offense range: from "rescue" keyword to end of last exception
        let rescue_kw_start = node.location().start_offset();
        let last_exception = exceptions.last().unwrap();
        let end_offset = last_exception.location().end_offset();

        let offense = self.ctx.offense_with_range(
            "Lint/RescueType",
            &message,
            Severity::Warning,
            rescue_kw_start,
            end_offset,
        );

        // Build correction: keep only valid exceptions
        let valid_sources: Vec<&str> = exceptions
            .iter()
            .filter(|e| !RescueType::is_invalid_type(e))
            .map(|n| {
                let loc = n.location();
                &self.ctx.source[loc.start_offset()..loc.end_offset()]
            })
            .collect();

        let correction_text = if valid_sources.is_empty() {
            String::new()
        } else {
            format!(" {}", valid_sources.join(", "))
        };

        // Replace from after "rescue" keyword to end of last exception
        let rescue_kw_end = rescue_kw_start + 6; // "rescue" is 6 bytes
        let correction = Correction::replace(rescue_kw_end, end_offset, &correction_text);

        self.offenses.push(offense.with_correction(correction));

        ruby_prism::visit_rescue_node(self, node);
    }
}

impl Cop for RescueType {
    fn name(&self) -> &'static str {
        "Lint/RescueType"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = RescueTypeVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
