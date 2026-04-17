//! Lint/RedundantStringCoercion - Flags redundant `Object#to_s` in interpolation and
//! `print`/`puts`/`warn` arguments.

use crate::cops::{CheckContext, Cop};
use crate::helpers::interpolation::embedded_statements_parts;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_DEFAULT: &str = "Redundant use of `Object#to_s` in";
const MSG_SELF: &str = "Use `self` instead of `Object#to_s` in";
const PRINT_METHODS: &[&str] = &["print", "puts", "warn"];

#[derive(Default)]
pub struct RedundantStringCoercion;

impl RedundantStringCoercion {
    pub fn new() -> Self { Self }
}

impl Cop for RedundantStringCoercion {
    fn name(&self) -> &'static str { "Lint/RedundantStringCoercion" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RedundantStringCoercionVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RedundantStringCoercionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RedundantStringCoercionVisitor<'a> {
    fn handle_interpolation(&mut self, node: &Node<'_>) {
        for begin in embedded_statements_parts(node) {
            let stmts = match begin.statements() {
                Some(s) => s,
                None => continue,
            };
            let body: Vec<Node> = stmts.body().iter().collect();
            let final_node = match body.last() {
                Some(n) => n,
                None => continue,
            };
            if let Some(call) = as_to_s_without_args(final_node) {
                self.register(&call, "interpolation");
            }
        }
    }

    fn handle_send(&mut self, node: &ruby_prism::CallNode<'_>) {
        if node.receiver().is_some() { return; }
        let method = node_name!(node);
        if !PRINT_METHODS.contains(&method.as_ref()) { return; }
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        for arg in args.arguments().iter() {
            if let Some(call) = as_to_s_without_args(&arg) {
                self.register(&call, &format!("`{}`", method));
            }
        }
    }

    fn register(&mut self, call: &ruby_prism::CallNode<'_>, context: &str) {
        let sel = match call.message_loc() {
            Some(l) => l,
            None => return,
        };
        let sel_start = sel.start_offset();
        let sel_end = sel.end_offset();

        let (msg_prefix, replacement) = if let Some(recv) = call.receiver() {
            let rloc = recv.location();
            let text = self.ctx.source[rloc.start_offset()..rloc.end_offset()].to_string();
            (MSG_DEFAULT, text)
        } else {
            (MSG_SELF, "self".to_string())
        };
        let message = format!("{} {}.", msg_prefix, context);

        let call_loc = call.location();
        let call_start = call_loc.start_offset();
        let call_end = call_loc.end_offset();

        let offense = self
            .ctx
            .offense_with_range(
                "Lint/RedundantStringCoercion",
                &message,
                Severity::Warning,
                sel_start,
                sel_end,
            )
            .with_correction(Correction::replace(call_start, call_end, replacement));
        self.offenses.push(offense);
    }
}

/// Matches RuboCop pattern `(call _ :to_s)` — a CallNode with method `:to_s` and no arguments.
fn as_to_s_without_args<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        if node_name!(call) != "to_s" { return None; }
        if call.arguments().map_or(false, |a| a.arguments().iter().count() > 0) {
            return None;
        }
        if call.block().is_some() { return None; }
        Some(call)
    } else {
        None
    }
}

impl Visit<'_> for RedundantStringCoercionVisitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        self.handle_interpolation(&node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        self.handle_interpolation(&node.as_node());
        ruby_prism::visit_interpolated_symbol_node(self, node);
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        self.handle_interpolation(&node.as_node());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        self.handle_interpolation(&node.as_node());
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.handle_send(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/RedundantStringCoercion", |_cfg| {
    Some(Box::new(RedundantStringCoercion::new()))
});
