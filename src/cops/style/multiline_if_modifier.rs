//! Style/MultilineIfModifier cop
//!
//! Checks for multiline bodies with trailing if/unless modifier.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{IfNode, Node, UnlessNode};

#[derive(Default)]
pub struct MultilineIfModifier;

impl MultilineIfModifier {
    pub fn new() -> Self {
        Self
    }

    /// Is this a modifier-form if? (no `then` keyword, no `end`)
    fn is_modifier_if(node: &IfNode) -> bool {
        node.then_keyword_loc().is_none() && node.end_keyword_loc().is_none()
    }

    fn is_modifier_unless(node: &UnlessNode) -> bool {
        node.end_keyword_loc().is_none()
    }

    fn body_is_multiline(start: usize, end: usize, source: &str) -> bool {
        source[start..end].contains('\n')
    }

    fn cond_is_multiline(start: usize, end: usize, source: &str) -> bool {
        source[start..end].contains('\n')
    }

    /// Check if a StatementsNode contains exactly one child that is itself a modifier if/unless.
    /// If so, we skip — the inner modifier will be flagged instead (avoids duplicate offenses).
    fn body_is_modifier_conditional(stmts_node: &ruby_prism::StatementsNode) -> bool {
        let items: Vec<Node> = stmts_node.body().iter().collect();
        if items.len() != 1 { return false; }
        match &items[0] {
            Node::IfNode { .. } => {
                let inner = items[0].as_if_node().unwrap();
                // Is inner a modifier if? (no then/end keyword)
                inner.then_keyword_loc().is_none() && inner.end_keyword_loc().is_none()
            }
            Node::UnlessNode { .. } => {
                let inner = items[0].as_unless_node().unwrap();
                inner.end_keyword_loc().is_none()
            }
            _ => false,
        }
    }
}

impl Cop for MultilineIfModifier {
    fn name(&self) -> &'static str {
        "Style/MultilineIfModifier"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &IfNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_modifier_if(node) {
            return vec![];
        }

        let stmts = match node.statements() {
            Some(s) => s,
            None => return vec![],
        };

        let body_start = stmts.location().start_offset();
        let body_end = stmts.location().end_offset();

        // Allow if condition is multiline
        let cond = node.predicate();
        if Self::cond_is_multiline(cond.location().start_offset(), cond.location().end_offset(), ctx.source) {
            return vec![];
        }

        if !Self::body_is_multiline(body_start, body_end, ctx.source) {
            return vec![];
        }

        // Skip if body is itself a modifier conditional — inner will fire
        if Self::body_is_modifier_conditional(&stmts) {
            return vec![];
        }

        let msg = "Favor a normal if-statement over a modifier clause in a multiline statement.";
        vec![ctx.offense_with_range(self.name(), msg, self.severity(), body_start, body_start + 1)]
    }

    fn check_unless(&self, node: &UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_modifier_unless(node) {
            return vec![];
        }

        let stmts = match node.statements() {
            Some(s) => s,
            None => return vec![],
        };

        let body_start = stmts.location().start_offset();
        let body_end = stmts.location().end_offset();

        let cond = node.predicate();
        if Self::cond_is_multiline(cond.location().start_offset(), cond.location().end_offset(), ctx.source) {
            return vec![];
        }

        if !Self::body_is_multiline(body_start, body_end, ctx.source) {
            return vec![];
        }

        if Self::body_is_modifier_conditional(&stmts) {
            return vec![];
        }

        let msg = "Favor a normal unless-statement over a modifier clause in a multiline statement.";
        vec![ctx.offense_with_range(self.name(), msg, self.severity(), body_start, body_start + 1)]
    }
}

crate::register_cop!("Style/MultilineIfModifier", |_cfg| Some(Box::new(MultilineIfModifier::new())));
