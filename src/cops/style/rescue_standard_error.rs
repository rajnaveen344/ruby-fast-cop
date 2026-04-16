//! Style/RescueStandardError cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Explicit,
    Implicit,
}

pub struct RescueStandardError {
    enforced_style: EnforcedStyle,
}

impl RescueStandardError {
    pub fn new(enforced_style: EnforcedStyle) -> Self { Self { enforced_style } }
}

impl Default for RescueStandardError {
    fn default() -> Self { Self::new(EnforcedStyle::Explicit) }
}

struct RescueVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    enforced_style: EnforcedStyle,
    cop_name: &'static str,
    offenses: Vec<Offense>,
}

impl Visit<'_> for RescueVisitor<'_> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        let exceptions = node.exceptions();
        let keyword_loc = node.keyword_loc();

        match self.enforced_style {
            EnforcedStyle::Implicit => {
                if exceptions.len() == 1 {
                    if let Some(exc) = exceptions.first() {
                        if m::is_toplevel_constant_named(&exc, "StandardError") {
                            self.offenses.push(self.ctx.offense_with_range(
                                self.cop_name,
                                "Omit the error class when rescuing `StandardError` by itself.",
                                Severity::Convention, keyword_loc.start_offset(), exc.location().end_offset(),
                            ).with_correction(Correction::delete(keyword_loc.end_offset(), exc.location().end_offset())));
                        }
                    }
                }
            }
            EnforcedStyle::Explicit => {
                if exceptions.is_empty() {
                    self.offenses.push(self.ctx.offense(
                        self.cop_name, "Avoid rescuing without specifying an error class.",
                        Severity::Convention, &keyword_loc,
                    ).with_correction(Correction::insert(keyword_loc.end_offset(), " StandardError")));
                }
            }
        }
        ruby_prism::visit_rescue_node(self, node);
    }
}

impl Cop for RescueStandardError {
    fn name(&self) -> &'static str { "Style/RescueStandardError" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RescueVisitor { ctx, enforced_style: self.enforced_style, cop_name: self.name(), offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
