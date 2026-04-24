//! Style/NumberedParameters
//!
//! Enforces style for blocks that use numbered parameters.
//! Two styles: `allow_single_line` (default) and `disallow`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberedParametersStyle {
    AllowSingleLine,
    Disallow,
}

pub struct NumberedParameters {
    style: NumberedParametersStyle,
}

impl NumberedParameters {
    pub fn new() -> Self {
        Self { style: NumberedParametersStyle::AllowSingleLine }
    }
    pub fn with_style(style: NumberedParametersStyle) -> Self { Self { style } }
}

impl Default for NumberedParameters {
    fn default() -> Self { Self::new() }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: NumberedParametersStyle,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &CallNode) {
        if let Some(block) = node.block() {
            if let Some(block_node) = block.as_block_node() {
                if let Some(params) = block_node.parameters() {
                    if params.as_numbered_parameters_node().is_some() {
                        // RuboCop's offense range = whole numblock (call + block).
                        let start = node.location().start_offset();
                        let end = block_node.location().end_offset();
                        // Single-line check uses the block literal only, not the
                        // outer method chain (which may span multiple lines).
                        let block_loc = block_node.location();
                        let is_single_line = self.ctx.same_line(
                            block_loc.start_offset(), block_loc.end_offset()
                        );
                        let msg = match self.style {
                            NumberedParametersStyle::Disallow => Some("Avoid using numbered parameters."),
                            NumberedParametersStyle::AllowSingleLine => {
                                if is_single_line { None } else {
                                    Some("Avoid using numbered parameters for multi-line blocks.")
                                }
                            }
                        };
                        if let Some(m) = msg {
                            self.offenses.push(self.ctx.offense_with_range(
                                "Style/NumberedParameters", m, Severity::Convention, start, end,
                            ));
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for NumberedParameters {
    fn name(&self) -> &'static str { "Style/NumberedParameters" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, style: self.style, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/NumberedParameters", |cfg| {
    let style = cfg.get_cop_config("Style/NumberedParameters")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "disallow" => NumberedParametersStyle::Disallow,
            _ => NumberedParametersStyle::AllowSingleLine,
        })
        .unwrap_or(NumberedParametersStyle::AllowSingleLine);
    Some(Box::new(NumberedParameters::with_style(style)))
});
