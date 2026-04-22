//! Style/MultilineMemoization cop
//!
//! Checks wrapping styles for multiline memoization (`||=`).
//! keyword style (default): requires `begin...end`
//! braces style: requires `(...)`

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Keyword,
    Braces,
}

pub struct MultilineMemoization {
    style: Style,
}

impl Default for MultilineMemoization {
    fn default() -> Self {
        Self { style: Style::Keyword }
    }
}

impl MultilineMemoization {
    pub fn new(style: Style) -> Self {
        Self { style }
    }
}

struct MemoVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    style: Style,
}

impl MemoVisitor<'_> {
    fn is_multiline_source(start: usize, end: usize, source: &str) -> bool {
        source[start..end].contains('\n')
    }

    fn check_rhs(&mut self, rhs: Node, node_start: usize, node_end: usize) {
        if !Self::is_multiline_source(rhs.location().start_offset(), rhs.location().end_offset(), self.ctx.source) {
            return;
        }

        let is_bad = match self.style {
            Style::Keyword => {
                // bad: rhs is ParenthesesNode (parenthesized begin)
                matches!(rhs, Node::ParenthesesNode { .. })
            }
            Style::Braces => {
                // bad: rhs is BeginNode (begin...end keyword)
                matches!(rhs, Node::BeginNode { .. })
            }
        };

        if is_bad {
            let msg = match self.style {
                Style::Keyword => "Wrap multiline memoization blocks in `begin` and `end`.",
                Style::Braces => "Wrap multiline memoization blocks in `(` and `)`.",
            };
            self.offenses.push(self.ctx.offense_with_range(
                "Style/MultilineMemoization",
                msg,
                Severity::Convention,
                node_start,
                node_end,
            ));
        }
    }
}

impl<'a> Visit<'_> for MemoVisitor<'a> {
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }

    fn visit_class_variable_or_write_node(&mut self, node: &ruby_prism::ClassVariableOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_class_variable_or_write_node(self, node);
    }

    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_constant_or_write_node(self, node);
    }

    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_call_or_write_node(self, node);
    }
}

impl Cop for MultilineMemoization {
    fn name(&self) -> &'static str {
        "Style/MultilineMemoization"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MemoVisitor {
            ctx,
            offenses: vec![],
            style: self.style,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/MultilineMemoization", |cfg| {
    let c: Cfg = cfg.typed("Style/MultilineMemoization");
    let style = match c.enforced_style.as_deref() {
        Some("braces") => Style::Braces,
        _ => Style::Keyword,
    };
    Some(Box::new(MultilineMemoization::new(style)))
});
