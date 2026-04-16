//! Lint/AmbiguousRegexpLiteral - Detects ambiguous regexp literals as first arg of
//! unparenthesized method calls.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/ambiguous_regexp_literal.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.";

#[derive(Default)]
pub struct AmbiguousRegexpLiteral;

impl AmbiguousRegexpLiteral {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for AmbiguousRegexpLiteral {
    fn name(&self) -> &'static str {
        "Lint/AmbiguousRegexpLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Is `node` a regexp literal (slash or %r form)?
    fn is_regexp(node: &Node) -> bool {
        matches!(
            node,
            Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
        )
    }

    /// Get the opening `/` offset of a slash-style regexp (leftmost).
    /// Returns None if not a slash-style regexp literal.
    fn slash_regexp_start(source: &str, offset: usize) -> Option<usize> {
        if source.as_bytes().get(offset) == Some(&b'/') {
            Some(offset)
        } else {
            None
        }
    }

    /// Returns true when first_arg represents a `/regex/ =~ rhs` expression
    /// (match-with-lvasgn form) that should keep the space before `(` during
    /// correction: `assert /r/ =~ s` -> `assert (/r/ =~ s)`.
    fn is_match_with_lvasgn(node: &Node) -> bool {
        match node {
            Node::MatchWriteNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = node_name!(call);
                if method != "=~" && method != "!~" {
                    return false;
                }
                if let Some(recv) = call.receiver() {
                    Self::is_regexp(&recv)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn check(&mut self, node: &ruby_prism::CallNode) {
        // Skip if call is parenthesized
        if node.opening_loc().is_some() {
            return;
        }

        // Must have arguments
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return;
        }
        let first_arg = &arg_list[0];

        // First arg's source must begin with `/` — the ambiguity happens only for
        // slash-delimited regexps. `%r{...}` is unambiguous.
        let first_start = first_arg.location().start_offset();
        let slash_off = match Self::slash_regexp_start(self.ctx.source, first_start) {
            Some(off) => off,
            None => return,
        };

        // Must be a method call without a receiver-side context that already disambiguates.
        // (e.g., attribute writes are CallNodes too but with equal_loc set.)
        if node.equal_loc().is_some() || node.is_attribute_write() {
            return;
        }

        // Offense at the `/` character.
        let mut offense = self.ctx.offense_with_range(
            "Lint/AmbiguousRegexpLiteral",
            MSG,
            Severity::Warning,
            slash_off,
            slash_off + 1,
        );

        // Correction: wrap arguments with parens.
        // - When first_arg is a match-with-lvasgn (regexp =~ rhs), keep the
        //   space after the selector: `assert (/r/ =~ s)`.
        // - Otherwise, replace the space-after-selector with `(`: `p(/r/)`.
        let last_arg = &arg_list[arg_list.len() - 1];
        let last_end = last_arg.location().end_offset();
        let first_arg_start = first_arg.location().start_offset();

        // `message_loc` points to the method name selector (e.g. `p`, `match`).
        let selector_end = node
            .message_loc()
            .map(|l| l.end_offset())
            .unwrap_or(first_arg_start);

        let edits = if Self::is_match_with_lvasgn(first_arg) {
            vec![
                Edit { start_offset: first_arg_start, end_offset: first_arg_start, replacement: "(".to_string() },
                Edit { start_offset: last_end, end_offset: last_end, replacement: ")".to_string() },
            ]
        } else {
            vec![
                Edit {
                    start_offset: selector_end,
                    end_offset: first_arg_start,
                    replacement: "(".to_string(),
                },
                Edit { start_offset: last_end, end_offset: last_end, replacement: ")".to_string() },
            ]
        };

        offense = offense.with_correction(Correction { edits });
        self.offenses.push(offense);
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check(node);
        ruby_prism::visit_call_node(self, node);
    }
}
