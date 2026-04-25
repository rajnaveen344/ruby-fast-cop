//! Style/NegatedIfElseCondition — invert negated `if-else` / ternary conditions and swap branches.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/negated_if_else_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use crate::node_name;
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/NegatedIfElseCondition";

const NEGATED_EQ: &[&str] = &["!=", "!~"];

#[derive(Default)]
pub struct NegatedIfElseCondition;

impl NegatedIfElseCondition {
    pub fn new() -> Self { Self }
}

impl Cop for NegatedIfElseCondition {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = NegVisitor { ctx, offenses: Vec::new(), corrected_depth: 0 };
        v.visit_program_node(node);
        v.offenses
    }
}

struct NegVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    corrected_depth: usize,
}

impl<'a> Visit<'_> for NegVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let (off, corrected) = self.try_check_if(node);
        if let Some(o) = off {
            self.offenses.push(o);
        }
        if corrected {
            self.corrected_depth += 1;
            ruby_prism::visit_if_node(self, node);
            self.corrected_depth -= 1;
        } else {
            ruby_prism::visit_if_node(self, node);
        }
    }
}

impl<'a> NegVisitor<'a> {
    fn try_check_if(&self, node: &ruby_prism::IfNode) -> (Option<Offense>, bool) {
        let ctx = self.ctx;
        // Skip elsif (an elsif IfNode has if_keyword == "elsif").
        if let Some(kw) = node.if_keyword_loc() {
            let kw_text = ctx.src(kw.start_offset(), kw.end_offset());
            if kw_text == "elsif" {
                return (None, false);
            }
        }

        // Must have a non-elsif else branch.
        let Some(sub) = node.subsequent() else { return (None, false) };
        let else_node = match &sub {
            Node::ElseNode { .. } => sub.as_else_node().unwrap(),
            _ => return (None, false), // elsif chain
        };

        // Skip when else-branch is empty (RuboCop: `if_else?` requires else_branch).
        if else_node.statements().is_none() {
            return (None, false);
        }

        // Unwrap parentheses / begin nodes around the condition.
        let pred = node.predicate();
        let Some(call) = unwrap_grouping_call(&pred) else { return (None, false) };
        let method = node_name!(call);
        let method_str = method.as_ref();

        // Must have <2 arguments.
        let arg_count = call.arguments().map_or(0, |a| a.arguments().iter().count());
        if arg_count >= 2 {
            return (None, false);
        }

        let is_neg_method = method_str == "!" || method_str == "not";
        let is_neg_eq = NEGATED_EQ.contains(&method_str);

        if !is_neg_method && !is_neg_eq {
            return (None, false);
        }

        // Skip double negation: `!!x`
        if is_neg_method {
            if let Some(recv) = call.receiver() {
                if let Some(inner) = recv.as_call_node() {
                    let inner_name = node_name!(inner);
                    if inner_name == "!" {
                        return (None, false);
                    }
                }
            }
        }

        let is_ternary = is_ternary(node, ctx.source);
        let type_label = if is_ternary { "ternary" } else { "if-else" };
        let message = format!("Invert the negated condition and swap the {} branches.", type_label);

        let n_start = node.location().start_offset();
        let n_end = node.location().end_offset();
        let mut offense = ctx.offense_with_range(COP_NAME, &message, Severity::Convention, n_start, n_end);

        // RuboCop: skip correction (but emit offense) when an enclosing IfNode was already corrected.
        let mut emitted_correction = false;
        if self.corrected_depth == 0 {
            if let Some(corr) = build_correction(node, &else_node, call, is_neg_method, is_ternary, ctx) {
                offense = offense.with_correction(corr);
                emitted_correction = true;
            }
        }

        (Some(offense), emitted_correction)
    }
}

fn is_ternary(node: &ruby_prism::IfNode, source: &str) -> bool {
    node.end_keyword_loc().is_none() && {
        let s = node.location().start_offset();
        !source[s..].starts_with("if") && !source[s..].starts_with("unless")
    }
}

fn unwrap_grouping_call<'a>(node: &Node<'a>) -> Option<ruby_prism::CallNode<'a>> {
    match node {
        Node::CallNode { .. } => node.as_call_node(),
        Node::ParenthesesNode { .. } => {
            let p = node.as_parentheses_node().unwrap();
            let body = p.body()?;
            unwrap_grouping_call(&body)
        }
        Node::BeginNode { .. } => {
            let b = node.as_begin_node().unwrap();
            let stmts = b.statements()?;
            let first = stmts.body().iter().next()?;
            unwrap_grouping_call(&first)
        }
        Node::StatementsNode { .. } => {
            let s = node.as_statements_node().unwrap();
            let first = s.body().iter().next()?;
            unwrap_grouping_call(&first)
        }
        _ => None,
    }
}

fn build_correction(
    if_node: &ruby_prism::IfNode,
    else_node: &ruby_prism::ElseNode,
    cond_call: ruby_prism::CallNode,
    is_neg_method: bool,
    is_ternary: bool,
    ctx: &CheckContext,
) -> Option<Correction> {
    let mut edits = Vec::new();

    // 1. Replace the condition.
    let cond_start = cond_call.location().start_offset();
    let cond_end = cond_call.location().end_offset();
    let new_cond = if is_neg_method {
        let recv = cond_call.receiver()?;
        let rs = recv.location().start_offset();
        let re = recv.location().end_offset();
        ctx.src(rs, re).to_string()
    } else {
        let method = node_name!(cond_call);
        let inv = method.replace('!', "=");
        let recv = cond_call.receiver()?;
        let arg = cond_call.arguments()?.arguments().iter().next()?;
        let rs = recv.location().start_offset();
        let re = recv.location().end_offset();
        let as_ = arg.location().start_offset();
        let ae = arg.location().end_offset();
        format!("{} {} {}", ctx.src(rs, re), inv, ctx.src(as_, ae))
    };
    edits.push(Edit { start_offset: cond_start, end_offset: cond_end, replacement: new_cond });

    // 2. Swap branches.
    let then_branch = if_node.statements();
    if then_branch.is_none() {
        // remove `else` line entirely (whole-line range).
        let else_kw = else_node.else_keyword_loc();
        let line_start = ctx.line_start(else_kw.start_offset());
        // find end of line (newline included).
        let bytes = ctx.source.as_bytes();
        let mut p = else_kw.end_offset();
        while p < bytes.len() && bytes[p] != b'\n' { p += 1; }
        if p < bytes.len() { p += 1; }
        edits.push(Edit { start_offset: line_start, end_offset: p, replacement: String::new() });
        return Some(Correction { edits });
    }

    if is_ternary {
        // Swap if_branch and else_branch source.
        let if_b = then_branch.unwrap().body().iter().next()?;
        let else_b = else_node.statements()?.body().iter().next()?;
        let i_s = if_b.location().start_offset();
        let i_e = if_b.location().end_offset();
        let e_s = else_b.location().start_offset();
        let e_e = else_b.location().end_offset();
        let if_text = ctx.src(i_s, i_e).to_string();
        let else_text = ctx.src(e_s, e_e).to_string();
        edits.push(Edit { start_offset: i_s, end_offset: i_e, replacement: else_text });
        edits.push(Edit { start_offset: e_s, end_offset: e_e, replacement: if_text });
    } else {
        // if-else form. Swap text from end-of-condition .. begin-of-`else`
        // with text from end-of-`else` .. begin-of-`end`.
        let else_kw = else_node.else_keyword_loc();
        let cond_loc = if_node.predicate().location();
        let cond_e = cond_loc.end_offset();
        let else_kw_s = else_kw.start_offset();
        let else_kw_e = else_kw.end_offset();
        let end_kw = if_node.end_keyword_loc()?;
        let end_kw_s = end_kw.start_offset();

        let if_text = ctx.src(cond_e, else_kw_s).to_string();
        let else_text = ctx.src(else_kw_e, end_kw_s).to_string();
        edits.push(Edit { start_offset: cond_e, end_offset: else_kw_s, replacement: else_text });
        edits.push(Edit { start_offset: else_kw_e, end_offset: end_kw_s, replacement: if_text });
    }

    Some(Correction { edits })
}

crate::register_cop!("Style/NegatedIfElseCondition", |_cfg| Some(Box::new(NegatedIfElseCondition::new())));
