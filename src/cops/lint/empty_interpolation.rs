//! Lint/EmptyInterpolation - Checks for empty interpolation (e.g. `"#{}"`).

use crate::cops::{CheckContext, Cop};
use crate::helpers::interpolation::{embedded_statements_parts, is_percent_literal_array};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Empty interpolation detected.";

#[derive(Default)]
pub struct EmptyInterpolation;

impl EmptyInterpolation {
    pub fn new() -> Self { Self }
}

impl Cop for EmptyInterpolation {
    fn name(&self) -> &'static str { "Lint/EmptyInterpolation" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = EmptyInterpolationVisitor { ctx, offenses: Vec::new(), in_percent_array: 0 };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EmptyInterpolationVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_percent_array: u32,
}

impl<'a> EmptyInterpolationVisitor<'a> {
    fn handle(&mut self, node: &Node<'_>) {
        if self.in_percent_array > 0 { return; }
        for begin in embedded_statements_parts(node) {
            if !is_effectively_empty(&begin) { continue; }
            let loc = begin.location();
            let start = loc.start_offset();
            let end = loc.end_offset();
            let offense = self
                .ctx
                .offense_with_range("Lint/EmptyInterpolation", MSG, Severity::Warning, start, end)
                .with_correction(Correction::delete(start, end));
            self.offenses.push(offense);
        }
    }
}

/// True if the begin node's children are all nil-like / empty-string literals.
fn is_effectively_empty(begin: &ruby_prism::EmbeddedStatementsNode<'_>) -> bool {
    let stmts = match begin.statements() {
        Some(s) => s,
        None => return true,
    };
    let body: Vec<Node> = stmts.body().iter().collect();
    if body.is_empty() { return true; }
    body.iter().all(is_nil_or_empty_literal)
}

fn is_nil_or_empty_literal(node: &Node<'_>) -> bool {
    match node {
        Node::NilNode { .. } => true,
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            // basic_literal? && str_content&.empty?
            // Exclude heredoc / interpolation-escaped strings. Use opening_loc()/unescaped.
            if s.opening_loc().is_none() { return false; }
            let bytes: &[u8] = s.unescaped().as_ref();
            bytes.is_empty()
        }
        _ => false,
    }
}

impl Visit<'_> for EmptyInterpolationVisitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_symbol_node(self, node);
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        self.handle(&node.as_node());
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        if is_percent_literal_array(node, self.ctx.source) {
            self.in_percent_array += 1;
            ruby_prism::visit_array_node(self, node);
            self.in_percent_array -= 1;
        } else {
            ruby_prism::visit_array_node(self, node);
        }
    }
}
