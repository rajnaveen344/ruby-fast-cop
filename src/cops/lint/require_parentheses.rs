//! Lint/RequireParentheses - Predicate method calls without parens where last arg has boolean operator.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

const MSG: &str = "Use parentheses in the method call to avoid confusion about precedence.";

#[derive(Default)]
pub struct RequireParentheses;

impl RequireParentheses {
    pub fn new() -> Self { Self }
}

fn is_operator_keyword(node: &ruby_prism::Node) -> bool {
    match node {
        ruby_prism::Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            n.operator_loc().as_slice() == b"&&"
        }
        ruby_prism::Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            n.operator_loc().as_slice() == b"||"
        }
        _ => false,
    }
}

fn is_ternary_if(node: &ruby_prism::Node) -> bool {
    if let Some(if_node) = node.as_if_node() {
        return if_node.then_keyword_loc().map_or(false, |loc| loc.as_slice() == b"?");
    }
    false
}

impl Cop for RequireParentheses {
    fn name(&self) -> &'static str { "Lint/RequireParentheses" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Must not be parenthesized
        if node.opening_loc().is_some() {
            return vec![];
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return vec![];
        }

        let method = node_name!(node);
        let is_assignment = method.ends_with('=');
        let is_operator = {
            let m = method.as_ref();
            matches!(m, "[]" | "[]=" | "+" | "-" | "*" | "/" | "%" | "**"
                | "==" | "!=" | "===" | "<" | ">" | "<=" | ">=" | "<=>"
                | "&" | "|" | "^" | "<<" | ">>" | "=~" | "!~")
        };

        let first_arg = &arg_list[0];
        let last_arg = &arg_list[arg_list.len() - 1];

        // Case 1: first arg is a ternary with operator condition
        if is_ternary_if(first_arg) {
            if is_assignment || is_operator {
                return vec![];
            }
            let if_node = first_arg.as_if_node().unwrap();
            // Only flag if ternary condition contains a boolean operator
            if !is_operator_keyword(&if_node.predicate()) {
                return vec![];
            }
            // Offense range: call start to end of ternary condition
            let cond_end = if_node.predicate().location().end_offset();
            let start = node.location().start_offset();
            return vec![ctx.offense_with_range(
                "Lint/RequireParentheses", MSG, Severity::Warning, start, cond_end,
            )];
        }

        // Case 2: predicate method and last arg is `&&` or `||`
        let is_predicate = method.ends_with('?');
        if is_predicate && !is_assignment && !is_operator {
            if is_operator_keyword(last_arg) {
                return vec![ctx.offense_with_range(
                    "Lint/RequireParentheses",
                    MSG,
                    Severity::Warning,
                    node.location().start_offset(),
                    node.location().end_offset(),
                )];
            }
        }

        vec![]
    }
}

crate::register_cop!("Lint/RequireParentheses", |_cfg| Some(Box::new(RequireParentheses::new())));
