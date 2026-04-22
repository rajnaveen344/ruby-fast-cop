//! Style/DefWithParentheses cop
//!
//! Checks for empty parentheses in method definitions with no arguments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::DefNode;

#[derive(Default)]
pub struct DefWithParentheses;

impl DefWithParentheses {
    pub fn new() -> Self {
        Self
    }

    fn check_def_node<'a>(&self, node: &DefNode<'a>, ctx: &CheckContext) -> Vec<Offense> {
        // Must have no parameters
        if node.parameters().is_some() {
            return vec![];
        }

        // Get lparen location
        let lparen_loc = match node.lparen_loc() {
            Some(loc) => loc,
            None => return vec![], // no parens
        };
        let rparen_loc = match node.rparen_loc() {
            Some(loc) => loc,
            None => return vec![],
        };

        // Single-line non-endless: parens required for syntax, skip
        let start_offset = node.location().start_offset();
        let end_offset = node.location().end_offset();
        let is_single_line = !ctx.source[start_offset..end_offset].contains('\n');
        let is_endless = node.equal_loc().is_some();

        if is_single_line && !is_endless {
            return vec![];
        }

        // Endless: skip if `=` immediately follows `)` with no space — `def foo()=`
        if is_endless {
            let after_rparen = rparen_loc.end_offset();
            if ctx.source.as_bytes().get(after_rparen).copied() == Some(b'=') {
                return vec![];
            }
        }

        let start = lparen_loc.start_offset();
        let end = rparen_loc.end_offset();
        let msg = "Omit the parentheses in defs when the method doesn't accept any arguments.";
        vec![ctx.offense_with_range(self.name(), msg, self.severity(), start, end)]
    }
}

impl Cop for DefWithParentheses {
    fn name(&self) -> &'static str {
        "Style/DefWithParentheses"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_def_node(node, ctx)
    }
}

crate::register_cop!("Style/DefWithParentheses", |_cfg| Some(Box::new(DefWithParentheses::new())));
