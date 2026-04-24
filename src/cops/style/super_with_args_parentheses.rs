//! Style/SuperWithArgsParentheses cop
//!
//! Enforces parentheses for `super` with arguments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Use parentheses for `super` with arguments.";

#[derive(Default)]
pub struct SuperWithArgsParentheses;

impl SuperWithArgsParentheses {
    pub fn new() -> Self { Self }
}

impl Cop for SuperWithArgsParentheses {
    fn name(&self) -> &'static str { "Style/SuperWithArgsParentheses" }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'a>) {
        // parenthesized?  → skip
        if node.lparen_loc().is_some() {
            ruby_prism::visit_super_node(self, node);
            return;
        }
        let args = match node.arguments() {
            Some(a) => a,
            None => {
                ruby_prism::visit_super_node(self, node);
                return;
            }
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            ruby_prism::visit_super_node(self, node);
            return;
        }
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        // Build corrected: "super(<args>)"
        // keyword "super" is first 5 bytes
        let first_arg_start = arg_list.first().unwrap().location().start_offset();
        let last_arg_end = arg_list.last().unwrap().location().end_offset();
        let args_src = &self.ctx.source[first_arg_start..last_arg_end];
        let replacement = format!("super({})", args_src);
        let off = self
            .ctx
            .offense_with_range("Style/SuperWithArgsParentheses", MSG, Severity::Convention, start, end)
            .with_correction(Correction::replace(start, end, replacement));
        self.offenses.push(off);
        ruby_prism::visit_super_node(self, node);
    }
}

crate::register_cop!("Style/SuperWithArgsParentheses", |_cfg| Some(Box::new(SuperWithArgsParentheses::new())));
