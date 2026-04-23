//! Layout/SpaceAfterMethodName cop
//! Do not put a space between a method name and the opening parenthesis.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct SpaceAfterMethodName;

impl SpaceAfterMethodName {
    pub fn new() -> Self { Self }
}

impl Cop for SpaceAfterMethodName {
    fn name(&self) -> &'static str { "Layout/SpaceAfterMethodName" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = SpaceAfterMethodNameVisitor {
            source: ctx.source,
            offenses: Vec::new(),
            ctx,
        };
        let result = ruby_prism::parse(ctx.source.as_bytes());
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct SpaceAfterMethodNameVisitor<'a> {
    source: &'a str,
    offenses: Vec<Offense>,
    ctx: &'a CheckContext<'a>,
}

impl Visit<'_> for SpaceAfterMethodNameVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Only flag if there are params with parentheses
        if let Some(params) = node.parameters() {
            // Check if there's a lparen_loc
            if let Some(lparen_loc) = node.lparen_loc() {
                // Name end is the byte right after the method name
                let name_loc = node.name_loc();
                let name_end = name_loc.end_offset();
                let lparen_start = lparen_loc.start_offset();

                if lparen_start > name_end {
                    // There's space between name and (
                    let msg = "Do not put a space between a method name and the opening parenthesis.";
                    let offense = self.ctx.offense_with_range(
                        "Layout/SpaceAfterMethodName", msg, Severity::Convention,
                        name_end,
                        lparen_start,
                    ).with_correction(Correction::delete(name_end, lparen_start));
                    self.offenses.push(offense);
                }
                let _ = params; // suppress warning
            }
        }
        ruby_prism::visit_def_node(self, node);
    }
}

crate::register_cop!("Layout/SpaceAfterMethodName", |_cfg| Some(Box::new(SpaceAfterMethodName::new())));
