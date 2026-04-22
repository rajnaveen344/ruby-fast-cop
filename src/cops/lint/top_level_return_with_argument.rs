//! Lint/TopLevelReturnWithArgument - Top level return with argument gets ignored.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/top_level_return_with_argument.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct TopLevelReturnWithArgument;

impl TopLevelReturnWithArgument {
    pub fn new() -> Self {
        Self
    }
}

struct TopLevelReturnVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Depth of def/defs/block scopes (return inside these is NOT top-level)
    scope_depth: usize,
}

impl TopLevelReturnVisitor<'_> {
    fn is_top_level(&self) -> bool {
        self.scope_depth == 0
    }
}

impl Visit<'_> for TopLevelReturnVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.scope_depth += 1;
        ruby_prism::visit_def_node(self, node);
        self.scope_depth -= 1;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.scope_depth += 1;
        ruby_prism::visit_block_node(self, node);
        self.scope_depth -= 1;
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        self.scope_depth += 1;
        ruby_prism::visit_lambda_node(self, node);
        self.scope_depth -= 1;
    }

    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        if self.is_top_level() {
            // Check if return has arguments
            if let Some(args) = node.arguments() {
                if !args.arguments().is_empty() {
                    let start = node.location().start_offset();
                    let end = node.location().end_offset();

                    // Correction: remove arguments, keep just `return`
                    let return_kw_end = start + 6; // "return" is 6 bytes
                    let correction = Correction::delete(return_kw_end, end);

                    let offense = self.ctx.offense_with_range(
                        "Lint/TopLevelReturnWithArgument",
                        "Top level return with argument detected.",
                        Severity::Warning,
                        start,
                        end,
                    );
                    self.offenses.push(offense.with_correction(correction));
                }
            }
        }
        ruby_prism::visit_return_node(self, node);
    }
}

impl Cop for TopLevelReturnWithArgument {
    fn name(&self) -> &'static str {
        "Lint/TopLevelReturnWithArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = TopLevelReturnVisitor {
            ctx,
            offenses: Vec::new(),
            scope_depth: 0,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/TopLevelReturnWithArgument", |_cfg| {
    Some(Box::new(TopLevelReturnWithArgument::new()))
});
