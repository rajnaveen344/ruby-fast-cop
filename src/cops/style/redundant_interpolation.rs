//! Style/RedundantInterpolation — prefer `to_s` over `"#{x}"`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_interpolation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantInterpolation";
const MSG: &str = "Prefer `to_s` over string interpolation.";

#[derive(Default)]
pub struct RedundantInterpolation;

impl RedundantInterpolation {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantInterpolation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            parent_is_dstr: false,
            parent_is_percent_array: false,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    parent_is_dstr: bool,
    parent_is_percent_array: bool,
}

impl<'a> Visitor<'a> {
    fn check_interp_string(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        // Skip if implicit concatenation: parent is also dstr
        if self.parent_is_dstr {
            return;
        }
        // Skip %W/%I arrays
        if self.parent_is_percent_array {
            return;
        }

        let parts: Vec<_> = node.parts().iter().collect();
        if parts.len() != 1 {
            return;
        }
        let first = &parts[0];

        // Must be an interpolation (EmbeddedStatementsNode or EmbeddedVariableNode)
        let is_interp = matches!(
            first,
            Node::EmbeddedStatementsNode { .. } | Node::EmbeddedVariableNode { .. }
        );
        if !is_interp {
            return;
        }

        // Skip one-line `in` pattern match (MatchRequiredNode) — they are
        // not valid outside interpolation in the rewrite. Actually `42 in var`
        // is a MatchPredicateNode (allowed), but `42 => var` (MatchRequiredNode)
        // is NOT allowed and should not trigger.
        if let Some(es) = first.as_embedded_statements_node() {
            if let Some(stmts) = es.statements() {
                for stmt in stmts.body().iter() {
                    if matches!(&stmt, Node::MatchRequiredNode { .. }) {
                        return;
                    }
                }
            }
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG,
            Severity::Convention,
            start,
            end,
        ));
    }
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        self.check_interp_string(node);
        // Children of dstr: if any child is itself a dstr (implicit concat), suppress.
        let was = self.parent_is_dstr;
        self.parent_is_dstr = true;
        ruby_prism::visit_interpolated_string_node(self, node);
        self.parent_is_dstr = was;
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'pr>) {
        // %W/%I arrays: opening starts with `%`.
        let is_percent = node.opening_loc().map_or(false, |loc| {
            let s = &self.ctx.source[loc.start_offset()..loc.end_offset()];
            s.starts_with('%')
        });
        let saved = self.parent_is_percent_array;
        if is_percent {
            self.parent_is_percent_array = true;
        }
        ruby_prism::visit_array_node(self, node);
        self.parent_is_percent_array = saved;
    }
}
