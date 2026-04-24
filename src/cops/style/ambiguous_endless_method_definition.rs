//! Style/AmbiguousEndlessMethodDefinition
//!
//! Flags endless method definitions inside ambiguous lower-precedence
//! operations: `and`, `or`, or modifier `if`/`unless`/`while`/`until`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

fn is_endless_def(n: &Node) -> bool {
    n.as_def_node().map(|d| d.equal_loc().is_some()).unwrap_or(false)
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> V<'a> {
    fn flag(&mut self, start: usize, end: usize, keyword: &str) {
        let msg = format!("Avoid using `{}` statements with endless methods.", keyword);
        self.offenses.push(self.ctx.offense_with_range(
            "Style/AmbiguousEndlessMethodDefinition",
            &msg, Severity::Convention, start, end,
        ));
    }
}

impl<'a> Visit<'_> for V<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // modifier form: no end_keyword_loc
        if node.end_keyword_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if is_endless_def(&stmt) {
                        let loc = node.location();
                        self.flag(loc.start_offset(), loc.end_offset(), "if");
                        break;
                    }
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if node.end_keyword_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if is_endless_def(&stmt) {
                        let loc = node.location();
                        self.flag(loc.start_offset(), loc.end_offset(), "unless");
                        break;
                    }
                }
            }
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if node.closing_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if is_endless_def(&stmt) {
                        let loc = node.location();
                        self.flag(loc.start_offset(), loc.end_offset(), "while");
                        break;
                    }
                }
            }
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if node.closing_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if is_endless_def(&stmt) {
                        let loc = node.location();
                        self.flag(loc.start_offset(), loc.end_offset(), "until");
                        break;
                    }
                }
            }
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let left = node.left();
        if is_endless_def(&left) {
            let loc = node.location();
            self.flag(loc.start_offset(), loc.end_offset(), "and");
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let left = node.left();
        if is_endless_def(&left) {
            let loc = node.location();
            self.flag(loc.start_offset(), loc.end_offset(), "or");
        }
        ruby_prism::visit_or_node(self, node);
    }
}

#[derive(Default)]
pub struct AmbiguousEndlessMethodDefinition;

impl AmbiguousEndlessMethodDefinition {
    pub fn new() -> Self { Self }
}

impl Cop for AmbiguousEndlessMethodDefinition {
    fn name(&self) -> &'static str { "Style/AmbiguousEndlessMethodDefinition" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 0) { return vec![] }
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/AmbiguousEndlessMethodDefinition", |_cfg| {
    Some(Box::new(AmbiguousEndlessMethodDefinition::new()))
});
