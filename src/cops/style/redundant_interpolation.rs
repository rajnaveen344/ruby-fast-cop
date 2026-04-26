//! Style/RedundantInterpolation — prefer `to_s` over `"#{x}"`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_interpolation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantInterpolation";
const MSG: &str = "Prefer `to_s` over string interpolation.";

#[derive(Default)]
pub struct RedundantInterpolation;

impl RedundantInterpolation {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantInterpolation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            parent_is_dstr: false,
            parent_is_percent_array: false,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    parent_is_dstr: bool,
    parent_is_percent_array: bool,
}

impl<'a> Visitor<'a> {
    fn check_interp_string(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        // Skip if implicit concatenation: parent is also dstr
        if self.parent_is_dstr {
            return;
        }
        // Skip %W/%I arrays
        if self.parent_is_percent_array {
            return;
        }

        let parts: Vec<_> = node.parts().iter().collect();
        if parts.len() != 1 {
            return;
        }
        let first = &parts[0];

        // Must be an interpolation (EmbeddedStatementsNode or EmbeddedVariableNode)
        let is_interp = matches!(
            first,
            Node::EmbeddedStatementsNode { .. } | Node::EmbeddedVariableNode { .. }
        );
        if !is_interp {
            return;
        }

        // Skip one-line `in` pattern match (MatchRequiredNode) — they are
        // not valid outside interpolation in the rewrite. Actually `42 in var`
        // is a MatchPredicateNode (allowed), but `42 => var` (MatchRequiredNode)
        // is NOT allowed and should not trigger.
        if let Some(es) = first.as_embedded_statements_node() {
            if let Some(stmts) = es.statements() {
                for stmt in stmts.body().iter() {
                    if matches!(&stmt, Node::MatchRequiredNode { .. }) {
                        return;
                    }
                }
            }
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let correction = build_interpolation_correction(first, node, self.ctx.source);
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG,
            Severity::Convention,
            start,
            end,
        ).with_correction(correction));
    }
}

/// For a call node: if it has unparenthesized arguments, wrap them in parens.
/// `do_something 42` → `do_something(42)`
/// `foo.do_something 42` → `foo.do_something(42)`
/// `foo.bar` → `foo.bar` (no change)
fn add_parens_if_needed(call: &ruby_prism::CallNode, source: &str) -> String {
    let has_args = call.arguments().map_or(false, |a| a.arguments().iter().count() > 0);
    let already_parens = call.closing_loc().is_some();

    if !has_args || already_parens {
        return source[call.location().start_offset()..call.location().end_offset()].to_string();
    }

    // Build: receiver.selector(args)
    let args = call.arguments().unwrap();
    let arg_nodes: Vec<_> = args.arguments().iter().collect();
    let args_src = arg_nodes.iter()
        .map(|a| source[a.location().start_offset()..a.location().end_offset()].to_string())
        .collect::<Vec<_>>()
        .join(", ");

    // From start to end of selector (message_loc), then add `(args)`
    let selector_end = call.message_loc()
        .map(|l| l.end_offset())
        .unwrap_or(call.location().end_offset());
    let prefix = &source[call.location().start_offset()..selector_end];
    format!("{}({})", prefix, args_src)
}

/// Returns true if the node is a "simple" expression that can be written as `expr.to_s`
/// without parentheses. For CallNodes, also handles the case where unparenthesized args
/// need parens (caller must use `add_parens_if_needed`).
///
/// NOT simple (needs wrapping): binary operators, assignments, ternary, etc.
fn is_simple_expression(node: &Node) -> bool {
    match node {
        Node::LocalVariableReadNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::GlobalVariableReadNode { .. }
        | Node::ConstantReadNode { .. }
        | Node::ConstantPathNode { .. }
        | Node::SelfNode { .. }
        | Node::NilNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::StringNode { .. }
        | Node::SymbolNode { .. }
        | Node::NumberedReferenceReadNode { .. }
        | Node::BackReferenceReadNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            // Binary/unary operators: has receiver AND no call_operator_loc (no `.`)
            // e.g. `1 + 1`, `!foo`, `-foo` → NOT simple (needs parens)
            // Regular method calls: either no receiver (standalone) or has dot operator
            // e.g. `foo`, `foo.bar`, `do_something 42` → simple (may need parens added)
            let is_operator = call.receiver().is_some() && call.call_operator_loc().is_none();
            !is_operator
        }
        _ => false,
    }
}

/// Build autocorrect: `"#{expr}"` → `expr.to_s`
/// Three cases matching RuboCop:
/// 1. EmbeddedVariableNode (`"#@var"`) → `@var.to_s`
/// 2. EmbeddedStatementsNode with single simple expression → `expr.to_s`
/// 3. EmbeddedStatementsNode with complex/multi expressions → `(expr).to_s`
fn build_interpolation_correction(
    embedded: &ruby_prism::Node,
    interp_node: &ruby_prism::InterpolatedStringNode,
    source: &str,
) -> Correction {
    let node_start = interp_node.location().start_offset();
    let node_end = interp_node.location().end_offset();

    match embedded {
        Node::EmbeddedVariableNode { .. } => {
            // `"#@var"` → variable source + `.to_s`
            let ev = embedded.as_embedded_variable_node().unwrap();
            let var_src = &source[ev.variable().location().start_offset()..ev.variable().location().end_offset()];
            let replacement = format!("{}.to_s", var_src);
            Correction::replace(node_start, node_end, replacement)
        }
        Node::EmbeddedStatementsNode { .. } => {
            let es = embedded.as_embedded_statements_node().unwrap();
            let stmts_opt = es.statements();
            match stmts_opt {
                None => {
                    Correction::replace(node_start, node_end, "nil.to_s".to_string())
                }
                Some(stmts) => {
                    let stmt_list: Vec<_> = stmts.body().iter().collect();
                    if stmt_list.len() == 1 {
                        let inner = &stmt_list[0];
                        let inner_src = &source[inner.location().start_offset()..inner.location().end_offset()];
                        // Check if inner is a "simple" expression (variable, method call, constant)
                        // that doesn't need parens. Complex expressions need `(expr).to_s`.
                        if is_simple_expression(inner) {
                            // For CallNode with unparenthesized args, add parens
                            let formatted_src = if let Node::CallNode { .. } = inner {
                                let call = inner.as_call_node().unwrap();
                                add_parens_if_needed(&call, source)
                            } else {
                                inner_src.to_string()
                            };
                            let replacement = format!("{}.to_s", formatted_src);
                            Correction::replace(node_start, node_end, replacement)
                        } else {
                            let replacement = format!("({}).to_s", inner_src);
                            Correction::replace(node_start, node_end, replacement)
                        }
                    } else {
                        // Multi-statement: `(stmts_src).to_s`
                        let stmts_src = &source[stmts.location().start_offset()..stmts.location().end_offset()];
                        let replacement = format!("({}).to_s", stmts_src);
                        Correction::replace(node_start, node_end, replacement)
                    }
                }
            }
        }
        _ => {
            // Fallback: replace whole node with source of embedded + `.to_s`
            let embedded_src = &source[embedded.location().start_offset()..embedded.location().end_offset()];
            Correction::replace(node_start, node_end, format!("{}.to_s", embedded_src))
        }
    }
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        self.check_interp_string(node);
        // Children of dstr: if any child is itself a dstr (implicit concat), suppress.
        let was = self.parent_is_dstr;
        self.parent_is_dstr = true;
        ruby_prism::visit_interpolated_string_node(self, node);
        self.parent_is_dstr = was;
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'pr>) {
        // %W/%I arrays: opening starts with `%`.
        let is_percent = node.opening_loc().map_or(false, |loc| {
            let s = &self.ctx.source[loc.start_offset()..loc.end_offset()];
            s.starts_with('%')
        });
        let saved = self.parent_is_percent_array;
        if is_percent {
            self.parent_is_percent_array = true;
        }
        ruby_prism::visit_array_node(self, node);
        self.parent_is_percent_array = saved;
    }
}

crate::register_cop!("Style/RedundantInterpolation", |_cfg| {
    Some(Box::new(RedundantInterpolation::new()))
});
