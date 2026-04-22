//! Style/NestedTernaryOperator cop
//!
//! Flags ternary operators nested inside other ternary operators.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{IfNode, Node, Visit};

#[derive(Default)]
pub struct NestedTernaryOperator;

impl NestedTernaryOperator {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for NestedTernaryOperator {
    fn name(&self) -> &'static str {
        "Style/NestedTernaryOperator"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = NestedTernaryVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct NestedTernaryVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

fn is_ternary(node: &IfNode) -> bool {
    // Ternary has no `if` keyword loc (no `if`/`unless`/`elsif` keyword)
    node.if_keyword_loc().is_none()
}

/// Recursively search a node for ternary operators.
/// When a ternary is found, record it and stop searching deeper into that branch.
fn find_nested_ternaries(node: &Node, results: &mut Vec<(usize, usize)>) {
    match node {
        Node::IfNode { .. } => {
            if let Some(if_node) = node.as_if_node() {
                if is_ternary(&if_node) {
                    results.push((
                        if_node.location().start_offset(),
                        if_node.location().end_offset(),
                    ));
                    // Stop — don't descend into this ternary
                    return;
                }
            }
            // Non-ternary if: recurse into children
            recurse_into_node(node, results);
        }
        _ => {
            recurse_into_node(node, results);
        }
    }
}

fn recurse_into_node(node: &Node, results: &mut Vec<(usize, usize)>) {
    match node {
        Node::CallNode { .. } => {
            if let Some(call) = node.as_call_node() {
                if let Some(recv) = call.receiver() {
                    find_nested_ternaries(&recv, results);
                }
                if let Some(args) = call.arguments() {
                    for arg in args.arguments().iter() {
                        find_nested_ternaries(&arg, results);
                    }
                }
                if let Some(block) = call.block() {
                    find_nested_ternaries(&block, results);
                }
            }
        }
        Node::ParenthesesNode { .. } => {
            if let Some(parens) = node.as_parentheses_node() {
                if let Some(body) = parens.body() {
                    if let Some(stmts) = body.as_statements_node() {
                        for child in stmts.body().iter() {
                            find_nested_ternaries(&child, results);
                        }
                    }
                }
            }
        }
        Node::StatementsNode { .. } => {
            if let Some(stmts) = node.as_statements_node() {
                for child in stmts.body().iter() {
                    find_nested_ternaries(&child, results);
                }
            }
        }
        Node::BeginNode { .. } => {
            if let Some(begin) = node.as_begin_node() {
                if let Some(stmts) = begin.statements() {
                    for child in stmts.body().iter() {
                        find_nested_ternaries(&child, results);
                    }
                }
            }
        }
        Node::IfNode { .. } => {
            if let Some(if_node) = node.as_if_node() {
                if is_ternary(&if_node) {
                    results.push((if_node.location().start_offset(), if_node.location().end_offset()));
                    return;
                }
                // Non-ternary if: recurse into body + subsequent
                if let Some(stmts) = if_node.statements() {
                    for child in stmts.body().iter() {
                        find_nested_ternaries(&child, results);
                    }
                }
                if let Some(sub) = if_node.subsequent() {
                    find_nested_ternaries(&sub, results);
                }
            }
        }
        Node::ElseNode { .. } => {
            if let Some(else_node) = node.as_else_node() {
                if let Some(stmts) = else_node.statements() {
                    for child in stmts.body().iter() {
                        find_nested_ternaries(&child, results);
                    }
                }
            }
        }
        Node::BlockNode { .. } => {
            if let Some(block) = node.as_block_node() {
                if let Some(body) = block.body() {
                    find_nested_ternaries(&body, results);
                }
            }
        }
        _ => {}
    }
}

impl<'a> NestedTernaryVisitor<'a> {
    fn check_ternary_branches(&mut self, node: &IfNode) {
        // then branch
        if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() == 1 {
                let mut nested = Vec::new();
                find_nested_ternaries(&body[0], &mut nested);
                self.emit_nested(nested);
            }
        }

        // else branch — subsequent is ElseNode or another IfNode
        if let Some(sub) = node.subsequent() {
            let mut nested = Vec::new();
            match &sub {
                Node::ElseNode { .. } => {
                    if let Some(else_node) = sub.as_else_node() {
                        if let Some(stmts) = else_node.statements() {
                            let body: Vec<_> = stmts.body().iter().collect();
                            if body.len() == 1 {
                                find_nested_ternaries(&body[0], &mut nested);
                            }
                        }
                    }
                }
                other => {
                    // Could be another ternary for chained `a ? b : c ? d : e`
                    find_nested_ternaries(other, &mut nested);
                }
            }
            self.emit_nested(nested);
        }
    }

    fn emit_nested(&mut self, nested: Vec<(usize, usize)>) {
        for (start, end) in nested {
            self.offenses.push(self.ctx.offense_with_range(
                "Style/NestedTernaryOperator",
                "Ternary operators must not be nested. Prefer `if` or `else` constructs instead.",
                Severity::Convention,
                start,
                end,
            ));
        }
    }
}

impl Visit<'_> for NestedTernaryVisitor<'_> {
    fn visit_if_node(&mut self, node: &IfNode) {
        if is_ternary(node) {
            self.check_ternary_branches(node);
        }
        ruby_prism::visit_if_node(self, node);
    }
}

crate::register_cop!("Style/NestedTernaryOperator", |_cfg| {
    Some(Box::new(NestedTernaryOperator::new()))
});
