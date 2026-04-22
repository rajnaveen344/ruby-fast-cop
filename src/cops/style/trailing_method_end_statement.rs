//! Style/TrailingMethodEndStatement cop
//!
//! Checks when `end` of a multi-line method appears on the same line as trailing code.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::DefNode;

#[derive(Default)]
pub struct TrailingMethodEndStatement;

impl TrailingMethodEndStatement {
    pub fn new() -> Self {
        Self
    }

    fn check_def_node(&self, node: &DefNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip endless methods
        if node.equal_loc().is_some() {
            return vec![];
        }

        let def_start = node.location().start_offset();
        let def_end = node.location().end_offset();

        // Must span multiple lines
        if !ctx.source[def_start..def_end].contains('\n') {
            return vec![];
        }

        // Must have an end keyword
        let end_loc = match node.end_keyword_loc() {
            Some(loc) => loc,
            None => return vec![],
        };

        // Must have a body
        let body = match node.body() {
            Some(b) => b,
            None => return vec![],
        };

        let body_end = body.location().end_offset();
        let end_start = end_loc.start_offset();

        // body_end and end_start must be on the same line
        if ctx.source[body_end..end_start].contains('\n') {
            return vec![];
        }

        // And body must not be at start of line (trivial single-line body)
        // Actually just check: there's content between body end and end keyword on same line
        // The offense is at the `end` keyword location
        let msg = "Place the end statement of a multi-line method on its own line.";
        vec![ctx.offense(self.name(), msg, self.severity(), &end_loc)]
    }
}

impl Cop for TrailingMethodEndStatement {
    fn name(&self) -> &'static str {
        "Style/TrailingMethodEndStatement"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &DefNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_def_node(node, ctx)
    }
}

crate::register_cop!("Style/TrailingMethodEndStatement", |_cfg| Some(Box::new(TrailingMethodEndStatement::new())));
