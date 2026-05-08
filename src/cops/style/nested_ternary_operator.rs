//! Style/NestedTernaryOperator cop
//!
//! Flags ternary operators nested inside other ternary operators.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
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

/// Build correction: rewrite outer ternary as if/else
fn build_ternary_correction(outer: &IfNode, source: &str) -> Option<Correction> {
    // then_keyword_loc = `?` for ternary
    let question_loc = outer.then_keyword_loc()?;
    let cond = outer.predicate();
    let else_clause = outer.subsequent()?;
    let else_node = else_clause.as_else_node()?;
    let colon_loc = else_node.else_keyword_loc();

    // then branch source (strip parentheses if wrapped)
    let then_src = if let Some(stmts) = outer.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() == 1 {
            let s = &source[body[0].location().start_offset()..body[0].location().end_offset()];
            // remove_parentheses
            if s.starts_with('(') && s.ends_with(')') {
                s[1..s.len()-1].to_string()
            } else {
                s.to_string()
            }
        } else {
            return None;
        }
    } else {
        return None;
    };

    // else branch source
    let else_src = if let Some(stmts) = else_node.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() == 1 {
            source[body[0].location().start_offset()..body[0].location().end_offset()].to_string()
        } else {
            return None;
        }
    } else {
        return None;
    };

    let cond_src = &source[cond.location().start_offset()..cond.location().end_offset()];

    // Build replacement: `if {cond}\n{then}\nelse\n{else}\nend`
    let replacement = format!("if {}\n{}\nelse\n{}\nend", cond_src, then_src, else_src);

    let outer_start = outer.location().start_offset();
    let outer_end = outer.location().end_offset();

    Some(Correction {
        edits: vec![Edit {
            start_offset: outer_start,
            end_offset: outer_end,
            replacement,
        }],
    })
}

impl<'a> NestedTernaryVisitor<'a> {
    fn check_ternary_branches(&mut self, outer: &IfNode) {
        let mut has_nested = false;

        // then branch
        if let Some(stmts) = outer.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() == 1 {
                let mut nested = Vec::new();
                find_nested_ternaries(&body[0], &mut nested);
                if !nested.is_empty() {
                    has_nested = true;
                    self.emit_nested_with_outer(nested, outer);
                    return; // Only emit correction once per outer
                }
            }
        }

        // else branch
        if let Some(sub) = outer.subsequent() {
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
                    find_nested_ternaries(other, &mut nested);
                }
            }
            if !nested.is_empty() {
                has_nested = true;
                self.emit_nested_with_outer(nested, outer);
            }
        }

        let _ = has_nested;
    }

    fn emit_nested_with_outer(&mut self, nested: Vec<(usize, usize)>, outer: &IfNode) {
        let correction = build_ternary_correction(outer, self.ctx.source);
        for (i, (start, end)) in nested.into_iter().enumerate() {
            let offense = self.ctx.offense_with_range(
                "Style/NestedTernaryOperator",
                "Ternary operators must not be nested. Prefer `if` or `else` constructs instead.",
                Severity::Convention,
                start,
                end,
            );
            // Only first offense gets correction (to avoid multi-edit conflicts)
            if i == 0 {
                if let Some(c) = correction.clone() {
                    self.offenses.push(offense.with_correction(c));
                    continue;
                }
            }
            self.offenses.push(offense);
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
