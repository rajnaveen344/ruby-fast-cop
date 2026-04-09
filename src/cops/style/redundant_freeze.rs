//! Style/RedundantFreeze cop
//!
//! Checks for uses of `Object#freeze` on immutable objects.
//! Regexp and Range are frozen since Ruby 3.0.
//! From Ruby 3.0, interpolated strings are NOT frozen even with
//! `# frozen_string_literal: true`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Do not freeze immutable objects, as freezing them has no effect.";

#[derive(Default)]
pub struct RedundantFreeze;

impl RedundantFreeze {
    pub fn new() -> Self {
        Self
    }

    /// Check for `# frozen_string_literal: true` magic comment.
    fn frozen_string_literals_enabled(source: &str) -> bool {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !trimmed.starts_with('#') {
                break;
            }
            let content = trimmed[1..].trim();
            if content.starts_with("-*-") && content.ends_with("-*-") {
                let inner = content[3..content.len() - 3].trim();
                for part in inner.split(';') {
                    if let Some((key, val)) = part.trim().split_once(':') {
                        if key.trim().to_lowercase().replace(['-', '_'], "") == "frozenstringliteral"
                        {
                            return val.trim().eq_ignore_ascii_case("true");
                        }
                    }
                }
                continue;
            }
            if let Some((key, val)) = content.split_once(':') {
                if key.trim().to_lowercase().replace(['-', '_'], "") == "frozenstringliteral" {
                    return val.trim().eq_ignore_ascii_case("true");
                }
            }
        }
        false
    }

    fn is_always_immutable(node: &Node) -> bool {
        matches!(
            node,
            Node::IntegerNode { .. }
                | Node::FloatNode { .. }
                | Node::RationalNode { .. }
                | Node::ImaginaryNode { .. }
                | Node::SymbolNode { .. }
                | Node::InterpolatedSymbolNode { .. }
                | Node::TrueNode { .. }
                | Node::FalseNode { .. }
                | Node::NilNode { .. }
        )
    }

    /// Check if node is a frozen string (magic comment present).
    fn is_frozen_string(node: &Node, source: &str, ruby_version: f64) -> bool {
        match node {
            Node::StringNode { .. } => Self::frozen_string_literals_enabled(source),
            Node::InterpolatedStringNode { .. } => {
                if !Self::frozen_string_literals_enabled(source) {
                    return false;
                }
                if ruby_version >= 3.0 {
                    let interp = node.as_interpolated_string_node().unwrap();
                    // On 3.0+, strings with real interpolation are NOT frozen
                    let has_interp = interp.parts().iter().any(|p| {
                        matches!(
                            p,
                            Node::EmbeddedStatementsNode { .. }
                                | Node::EmbeddedVariableNode { .. }
                        )
                    });
                    !has_interp
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    /// Check if a node is an immutable literal (directly or inside parens).
    fn check_immutable(node: &Node, source: &str, ruby_version: f64) -> bool {
        Self::is_always_immutable(node)
            || Self::is_frozen_string(node, source, ruby_version)
            || (ruby_version >= 3.0
                && matches!(
                    node,
                    Node::RegularExpressionNode { .. }
                        | Node::InterpolatedRegularExpressionNode { .. }
                        | Node::RangeNode { .. }
                ))
    }

    /// Check immutable literal, including stripping parentheses.
    fn is_immutable_receiver(node: &Node, source: &str, ruby_version: f64) -> bool {
        if Self::check_immutable(node, source, ruby_version) {
            return true;
        }

        // Strip parentheses and check inner
        if let Some(paren) = node.as_parentheses_node() {
            if let Some(body) = paren.body() {
                if let Some(stmts) = body.as_statements_node() {
                    let items: Vec<_> = stmts.body().iter().collect();
                    if items.len() == 1 {
                        return Self::check_immutable(&items[0], source, ruby_version);
                    }
                } else {
                    return Self::check_immutable(&body, source, ruby_version);
                }
            }
        }

        false
    }

    /// Check if receiver is an operation producing immutable result.
    /// count/length/size always return integers (with or without block).
    fn is_immutable_operation(node: &Node) -> bool {
        if let Some(call) = node.as_call_node() {
            let method = node_name!(call);
            // count/length/size (possibly with a block attached)
            if matches!(method.as_ref(), "count" | "length" | "size") {
                return true;
            }
        }
        false
    }

    /// Check if parenthesized expression produces immutable result.
    /// `(1 + 2)`, `(2 > 1)`, `('a' > 'b')`, `(a > b)`
    fn is_immutable_paren_operation(node: &Node) -> bool {
        let paren = match node.as_parentheses_node() {
            Some(p) => p,
            None => return false,
        };
        let body = match paren.body() {
            Some(b) => b,
            None => return false,
        };

        // Unwrap StatementsNode to get the single expression
        let inner_call = if let Some(stmts) = body.as_statements_node() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 {
                return false;
            }
            match items[0].as_call_node() {
                Some(c) => c,
                None => return false,
            }
        } else {
            match body.as_call_node() {
                Some(c) => c,
                None => return false,
            }
        };

        let method = node_name!(inner_call);

        // Comparison operators always return boolean
        if matches!(
            method.as_ref(),
            "==" | "===" | "!=" | "<=" | ">=" | "<" | ">"
        ) {
            return true;
        }

        // Arithmetic ops
        if matches!(
            method.as_ref(),
            "+" | "-" | "*" | "**" | "/" | "%" | "<<"
        ) {
            // Pattern 1: numeric receiver (e.g., `(1 + 2)`)
            if let Some(recv) = inner_call.receiver() {
                if matches!(recv, Node::IntegerNode { .. } | Node::FloatNode { .. }) {
                    return true;
                }
            }

            // Pattern 2: non-string/non-array receiver + numeric arg
            if let Some(recv) = inner_call.receiver() {
                let is_str_or_array = matches!(
                    recv,
                    Node::StringNode { .. }
                        | Node::InterpolatedStringNode { .. }
                        | Node::ArrayNode { .. }
                );
                if !is_str_or_array {
                    if let Some(args) = inner_call.arguments() {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if !arg_list.is_empty()
                            && matches!(
                                arg_list[0],
                                Node::IntegerNode { .. } | Node::FloatNode { .. }
                            )
                        {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}

impl Cop for RedundantFreeze {
    fn name(&self) -> &'static str {
        "Style/RedundantFreeze"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "freeze" {
            return vec![];
        }

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        // Must not have arguments
        if node.arguments().is_some() {
            return vec![];
        }

        let flagged = Self::is_immutable_receiver(&receiver, ctx.source, ctx.target_ruby_version)
            || Self::is_immutable_operation(&receiver)
            || Self::is_immutable_paren_operation(&receiver);

        if !flagged {
            return vec![];
        }

        let start = receiver.location().start_offset();
        let end = node.location().end_offset();
        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)]
    }
}
