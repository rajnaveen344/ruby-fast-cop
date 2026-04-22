//! Style/RedundantAssignment cop
//!
//! Checks for redundant assignment before returning.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantAssignment";
const MSG: &str = "Redundant assignment before returning detected.";

#[derive(Default)]
pub struct RedundantAssignment;

impl RedundantAssignment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantAssignment {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn offense_at(&mut self, start: usize, end: usize) {
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention, start, end,
        ));
    }
}

/// Check a method body node for redundant assignments.
fn check_body(node: &Node, offenses: &mut Vec<(usize, usize)>) {
    match node {
        Node::StatementsNode { .. } => {
            let stmts = node.as_statements_node().unwrap();
            let body: Vec<_> = stmts.body().iter().collect();
            check_begin_stmts(&body, offenses);
        }
        Node::BeginNode { .. } => {
            check_begin_node(node, offenses);
        }
        _ => {}
    }
}

fn check_begin_node(node: &Node, offenses: &mut Vec<(usize, usize)>) {
    let begin = node.as_begin_node().unwrap();
    // If has ensure, skip
    if begin.ensure_clause().is_some() {
        return;
    }
    // Check begin's own statements
    if let Some(stmts) = begin.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        check_begin_stmts(&body, offenses);
    }
    // Check rescue chains
    if let Some(rescue) = begin.rescue_clause() {
        check_rescue_chain(&rescue, offenses);
    }
}

fn check_rescue_chain(rescue: &ruby_prism::RescueNode, offenses: &mut Vec<(usize, usize)>) {
    if let Some(stmts) = rescue.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        check_begin_stmts(&body, offenses);
    }
    let mut next_opt = rescue.subsequent();
    while let Some(next) = next_opt {
        if let Some(stmts) = next.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            check_begin_stmts(&body, offenses);
        }
        next_opt = next.subsequent();
    }
}

/// Check a list of statements — look for `x = expr; x` pattern and recurse into control flow.
fn check_begin_stmts(stmts: &[Node], offenses: &mut Vec<(usize, usize)>) {
    if stmts.len() >= 2 {
        let last = &stmts[stmts.len() - 1];
        let second_last = &stmts[stmts.len() - 2];

        if let Some(name) = lvar_write_name(second_last) {
            if is_lvar_read_named(last, &name) {
                offenses.push((
                    second_last.location().start_offset(),
                    second_last.location().end_offset(),
                ));
                return;
            }
        }
    }
    // Recurse into the last statement if it's a control flow node
    if let Some(last) = stmts.last() {
        check_control_flow(last, offenses);
    }
}

fn check_control_flow(node: &Node, offenses: &mut Vec<(usize, usize)>) {
    match node {
        Node::IfNode { .. } => check_if(node, offenses),
        Node::CaseNode { .. } => check_case(node, offenses),
        Node::CaseMatchNode { .. } => check_case_match(node, offenses),
        Node::BeginNode { .. } => check_begin_node(node, offenses),
        _ => {}
    }
}

fn check_if(node: &Node, offenses: &mut Vec<(usize, usize)>) {
    let if_node = node.as_if_node().unwrap();
    // Skip modifier/ternary
    if if_node.end_keyword_loc().is_none() {
        return;
    }
    if let Some(stmts) = if_node.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        check_begin_stmts(&body, offenses);
    }
    if let Some(sub) = if_node.subsequent() {
        if let Some(elsif) = sub.as_if_node() {
            check_if(&elsif.as_node(), offenses);
        } else if let Some(else_node) = sub.as_else_node() {
            if let Some(stmts) = else_node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                check_begin_stmts(&body, offenses);
            }
        }
    }
}

fn check_case(node: &Node, offenses: &mut Vec<(usize, usize)>) {
    let case = node.as_case_node().unwrap();
    for when in case.conditions().iter() {
        if let Some(when_node) = when.as_when_node() {
            if let Some(stmts) = when_node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                check_begin_stmts(&body, offenses);
            }
        }
    }
    if let Some(else_node) = case.else_clause() {
        if let Some(stmts) = else_node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            check_begin_stmts(&body, offenses);
        }
    }
}

fn check_case_match(node: &Node, offenses: &mut Vec<(usize, usize)>) {
    let case = node.as_case_match_node().unwrap();
    for in_pattern in case.conditions().iter() {
        if let Some(in_node) = in_pattern.as_in_node() {
            if let Some(stmts) = in_node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                check_begin_stmts(&body, offenses);
            }
        }
    }
    if let Some(else_node) = case.else_clause() {
        if let Some(stmts) = else_node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            check_begin_stmts(&body, offenses);
        }
    }
}

fn lvar_write_name(node: &Node) -> Option<String> {
    if let Some(lv) = node.as_local_variable_write_node() {
        return Some(String::from_utf8_lossy(lv.name().as_slice()).to_string());
    }
    None
}

fn is_lvar_read_named(node: &Node, name: &str) -> bool {
    if let Some(lv) = node.as_local_variable_read_node() {
        let n = String::from_utf8_lossy(lv.name().as_slice());
        return n == name;
    }
    false
}

impl Visit<'_> for Visitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if let Some(body) = node.body() {
            let mut offenses: Vec<(usize, usize)> = Vec::new();
            check_body(&body, &mut offenses);
            for (start, end) in offenses {
                self.offense_at(start, end);
            }
        }
        ruby_prism::visit_def_node(self, node);
    }
}

crate::register_cop!("Style/RedundantAssignment", |_cfg| {
    Some(Box::new(RedundantAssignment::new()))
});
