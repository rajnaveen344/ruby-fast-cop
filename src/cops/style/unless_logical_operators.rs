//! Style/UnlessLogicalOperators - Check for logical operators in `unless` conditions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/unless_logical_operators.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const FORBID_MIXED_LOGICAL_OPERATORS: &str = "Do not use mixed logical operators in an `unless`.";
const FORBID_LOGICAL_OPERATORS: &str = "Do not use any logical operator in an `unless`.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    ForbidMixedLogicalOperators,
    ForbidLogicalOperators,
}

pub struct UnlessLogicalOperators {
    style: EnforcedStyle,
}

impl UnlessLogicalOperators {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Default for UnlessLogicalOperators {
    fn default() -> Self {
        Self::new(EnforcedStyle::ForbidMixedLogicalOperators)
    }
}

impl Cop for UnlessLogicalOperators {
    fn name(&self) -> &'static str {
        "Style/UnlessLogicalOperators"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        let cond = node.predicate();
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        match self.style {
            EnforcedStyle::ForbidMixedLogicalOperators => {
                if mixed_logical_operator(&cond, ctx) {
                    return vec![ctx.offense_with_range(
                        self.name(),
                        FORBID_MIXED_LOGICAL_OPERATORS,
                        Severity::Convention,
                        start,
                        end,
                    )];
                }
            }
            EnforcedStyle::ForbidLogicalOperators => {
                if matches!(cond, Node::AndNode { .. } | Node::OrNode { .. }) {
                    return vec![ctx.offense_with_range(
                        self.name(),
                        FORBID_LOGICAL_OPERATORS,
                        Severity::Convention,
                        start,
                        end,
                    )];
                }
            }
        }
        vec![]
    }
}

fn mixed_logical_operator(cond: &Node, ctx: &CheckContext) -> bool {
    or_with_and(cond) || and_with_or(cond) || mixed_precedence_and(cond, ctx) || mixed_precedence_or(cond, ctx)
}

/// Top-level `or` containing an `and` descendant.
fn or_with_and(node: &Node) -> bool {
    if matches!(node, Node::OrNode { .. }) {
        let mut v = FindKindVisitor { wants_and: true, found: false };
        v.visit(node);
        // FindKindVisitor visits self too — but since the top is `or`, it won't match an `and`.
        return v.found;
    }
    false
}

/// Top-level `and` containing an `or` descendant.
fn and_with_or(node: &Node) -> bool {
    if matches!(node, Node::AndNode { .. }) {
        let mut v = FindKindVisitor { wants_and: false, found: false };
        v.visit(node);
        return v.found;
    }
    false
}

/// Mix of `&&` and `and` used together — anywhere in the condition (including descendants).
fn mixed_precedence_and(cond: &Node, ctx: &CheckContext) -> bool {
    let mut v = CollectAndOps { source: ctx.source, ops: Vec::new() };
    v.visit(cond);
    if v.ops.is_empty() {
        return false;
    }
    !(v.ops.iter().all(|s| s == "&&") || v.ops.iter().all(|s| s == "and"))
}

/// Mix of `||` and `or` used together — anywhere in the condition (including descendants).
fn mixed_precedence_or(cond: &Node, ctx: &CheckContext) -> bool {
    let mut v = CollectOrOps { source: ctx.source, ops: Vec::new() };
    v.visit(cond);
    if v.ops.is_empty() {
        return false;
    }
    !(v.ops.iter().all(|s| s == "||") || v.ops.iter().all(|s| s == "or"))
}

struct FindKindVisitor {
    /// `true` = looking for `AndNode`; `false` = looking for `OrNode`.
    wants_and: bool,
    found: bool,
}

impl<'pr> Visit<'pr> for FindKindVisitor {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        if self.wants_and {
            self.found = true;
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode<'pr>) {
        if !self.wants_and {
            self.found = true;
        }
        ruby_prism::visit_or_node(self, node);
    }
}

struct CollectAndOps<'a> {
    source: &'a str,
    ops: Vec<String>,
}

impl<'pr, 'a> Visit<'pr> for CollectAndOps<'a> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        let loc = node.operator_loc();
        self.ops
            .push(self.source[loc.start_offset()..loc.end_offset()].to_string());
        ruby_prism::visit_and_node(self, node);
    }
}

struct CollectOrOps<'a> {
    source: &'a str,
    ops: Vec<String>,
}

impl<'pr, 'a> Visit<'pr> for CollectOrOps<'a> {
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode<'pr>) {
        let loc = node.operator_loc();
        self.ops
            .push(self.source[loc.start_offset()..loc.end_offset()].to_string());
        ruby_prism::visit_or_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

crate::register_cop!("Style/UnlessLogicalOperators", |cfg| {
    let c: Cfg = cfg.typed("Style/UnlessLogicalOperators");
    let style = match c.enforced_style.as_str() {
        "forbid_logical_operators" => EnforcedStyle::ForbidLogicalOperators,
        _ => EnforcedStyle::ForbidMixedLogicalOperators,
    };
    Some(Box::new(UnlessLogicalOperators::new(style)))
});
