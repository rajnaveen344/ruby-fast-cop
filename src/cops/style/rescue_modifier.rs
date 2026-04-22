//! Style/RescueModifier cop
//!
//! Checks for uses of `rescue` in its modifier form.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Avoid using `rescue` in its modifier form.";

#[derive(Default)]
pub struct RescueModifier;

impl RescueModifier {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RescueModifier {
    fn name(&self) -> &'static str {
        "Style/RescueModifier"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RescueModifierVisitor {
            ctx,
            offenses: Vec::new(),
            skip_rescue_at: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RescueModifierVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Start offsets of RescueModifierNodes already reported via a MultiWriteNode
    skip_rescue_at: Vec<usize>,
}

impl<'a> Visit<'_> for RescueModifierVisitor<'a> {
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        // In Ruby >= 2.6, `a, b = 1, 2 rescue nil` creates a MultiWriteNode where
        // the direct value is a RescueModifierNode. Report offense at MultiWriteNode.
        if self.ctx.ruby_version_at_least(2, 6) {
            let value = node.value();
            if value.as_rescue_modifier_node().is_some() {
                // Skip inner rescue modifier (it will be reported here instead)
                self.skip_rescue_at.push(value.location().start_offset());
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                self.offenses.push(self.ctx.offense_with_range(
                    "Style/RescueModifier",
                    MSG,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode) {
        let start = node.location().start_offset();
        if !self.skip_rescue_at.contains(&start) {
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Style/RescueModifier",
                MSG,
                Severity::Convention,
                start,
                end,
            ));
        }
        ruby_prism::visit_rescue_modifier_node(self, node);
    }
}

crate::register_cop!("Style/RescueModifier", |_cfg| {
    Some(Box::new(RescueModifier::new()))
});
