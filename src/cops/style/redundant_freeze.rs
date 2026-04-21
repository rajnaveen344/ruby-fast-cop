//! Style/RedundantFreeze cop
//!
//! Checks for uses of `Object#freeze` on immutable objects.
//! Regexp and Range are frozen since Ruby 3.0.
//! From Ruby 3.0, interpolated strings are NOT frozen even with
//! `# frozen_string_literal: true`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Do not freeze immutable objects, as freezing them has no effect.";

#[derive(Default)]
pub struct RedundantFreeze {
    /// AllCops/StringLiteralsFrozenByDefault — when true, string literals are
    /// implicitly frozen unless `# frozen_string_literal: false` is present.
    string_literals_frozen_by_default: bool,
}

impl RedundantFreeze {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(string_literals_frozen_by_default: bool) -> Self {
        Self { string_literals_frozen_by_default }
    }

    /// Check for `# frozen_string_literal: false` magic comment.
    fn frozen_string_literals_disabled(source: &str) -> bool {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !trimmed.starts_with('#') {
                break;
            }
            let content = trimmed[1..].trim();
            if let Some((key, val)) = content.split_once(':') {
                if key.trim().to_lowercase().replace(['-', '_'], "") == "frozenstringliteral" {
                    return val.trim().eq_ignore_ascii_case("false");
                }
            }
        }
        false
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

    /// Whether string literals are effectively frozen for this source.
    /// True if magic comment is `true`, OR if StringLiteralsFrozenByDefault
    /// is enabled and no explicit `false` magic comment is present.
    fn strings_effectively_frozen(&self, source: &str) -> bool {
        if Self::frozen_string_literals_enabled(source) {
            return true;
        }
        self.string_literals_frozen_by_default && !Self::frozen_string_literals_disabled(source)
    }

    /// Check if node is a frozen string (magic comment present).
    fn is_frozen_string(&self, node: &Node, source: &str, ruby_version: f64) -> bool {
        match node {
            Node::StringNode { .. } => self.strings_effectively_frozen(source),
            Node::InterpolatedStringNode { .. } => {
                if !self.strings_effectively_frozen(source) {
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
    fn check_immutable(&self, node: &Node, source: &str, ruby_version: f64) -> bool {
        m::is_always_immutable_literal(node)
            || self.is_frozen_string(node, source, ruby_version)
            || (ruby_version >= 3.0
                && matches!(
                    node,
                    Node::RegularExpressionNode { .. }
                        | Node::InterpolatedRegularExpressionNode { .. }
                        | Node::RangeNode { .. }
                ))
    }

    /// Check immutable literal, including stripping parentheses.
    fn is_immutable_receiver(&self, node: &Node, source: &str, ruby_version: f64) -> bool {
        if self.check_immutable(node, source, ruby_version) {
            return true;
        }
        if let Some(inner) = m::unwrap_single_parens(node) {
            return self.check_immutable(&inner, source, ruby_version);
        }
        false
    }

    /// Check if receiver is an operation producing immutable result.
    /// count/length/size always return integers (with or without block).
    fn is_immutable_operation(node: &Node) -> bool {
        m::is_call_named_any(node, &["count", "length", "size"])
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

        let flagged = self.is_immutable_receiver(&receiver, ctx.source, ctx.target_ruby_version)
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

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { all_cops_string_literals_frozen_by_default: bool }

crate::register_cop!("Style/RedundantFreeze", |cfg| {
    let c: Cfg = cfg.typed("Style/RedundantFreeze");
    Some(Box::new(RedundantFreeze::with_config(c.all_cops_string_literals_frozen_by_default)))
});
