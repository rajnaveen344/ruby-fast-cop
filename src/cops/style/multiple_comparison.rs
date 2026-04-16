//! Style/MultipleComparison - Suggests `Array#include?` when comparing a
//! variable with multiple items in an `||` chain.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/multiple_comparison.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const COP_NAME: &str = "Style/MultipleComparison";
const MSG: &str = "Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.";

pub struct MultipleComparison {
    allow_method_comparison: bool,
    comparisons_threshold: usize,
}

impl Default for MultipleComparison {
    fn default() -> Self { Self { allow_method_comparison: true, comparisons_threshold: 2 } }
}

impl MultipleComparison {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(allow_method_comparison: bool, comparisons_threshold: usize) -> Self {
        Self { allow_method_comparison, comparisons_threshold }
    }
}

impl Cop for MultipleComparison {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            or_depth: 0,
            allow_method_comparison: self.allow_method_comparison,
            comparisons_threshold: self.comparisons_threshold,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    or_depth: usize,
    allow_method_comparison: bool,
    comparisons_threshold: usize,
}

impl<'a> Visitor<'a> {
    fn root_or_node(&mut self, node: &ruby_prism::OrNode) {
        if !is_nested_comparison(self.ctx, node, self.allow_method_comparison) { return; }

        // Walk or-tree collecting (variable_src, comparison_range). If we see simple_double_comparison
        // (lvar == lvar) abort. If we see >1 distinct variable, abort.
        let mut vars: HashSet<String> = HashSet::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut abort = false;
        walk(self.ctx, node, &mut vars, &mut ranges, self.allow_method_comparison, &mut abort);
        if abort { return; }
        if vars.len() != 1 { return; }
        if ranges.len() < self.comparisons_threshold { return; }

        let start = ranges.first().unwrap().0;
        let end = ranges.last().unwrap().1;
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention, start, end,
        ));
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        if self.or_depth == 0 {
            self.root_or_node(node);
        }
        self.or_depth += 1;
        ruby_prism::visit_or_node(self, node);
        self.or_depth -= 1;
    }
}

fn is_nested_comparison(ctx: &CheckContext, node: &ruby_prism::OrNode, allow_method_comparison: bool) -> bool {
    let left = node.left();
    let right = node.right();
    is_comparison(ctx, &left, allow_method_comparison)
        && is_comparison(ctx, &right, allow_method_comparison)
}

fn is_comparison(ctx: &CheckContext, node: &Node, allow_method_comparison: bool) -> bool {
    if let Some(or) = node.as_or_node() {
        return is_nested_comparison(ctx, &or, allow_method_comparison);
    }
    simple_comparison_src(ctx, node, allow_method_comparison).is_some()
}

/// Return `(var_src, value_is_call)` for a `==` comparison between a variable (lvar/call)
/// and any rhs — or None if no valid simple_comparison match.
fn simple_comparison_src(
    ctx: &CheckContext,
    node: &Node,
    allow_method_comparison: bool,
) -> Option<(String, bool)> {
    let call = node.as_call_node()?;
    if node_name!(call) != "==" { return None; }
    let recv = call.receiver()?;
    let args: Vec<Node> = call.arguments()?.arguments().iter().collect();
    if args.len() != 1 { return None; }
    let arg = &args[0];

    // simple_comparison_lhs: (send ${lvar call} :== $_)
    if is_lvar(&recv) || (is_call(&recv) && allow_method_comparison) {
        // var = recv, obj = arg
        return Some((src_of(ctx, &recv), is_call(arg)));
    }
    // simple_comparison_rhs: (send $_ :== ${lvar call})
    if is_lvar(arg) || (is_call(arg) && allow_method_comparison) {
        return Some((src_of(ctx, arg), is_call(&recv)));
    }
    None
}

fn is_lvar(node: &Node) -> bool { matches!(node, Node::LocalVariableReadNode { .. }) }
fn is_call(node: &Node) -> bool { matches!(node, Node::CallNode { .. }) }
fn src_of(ctx: &CheckContext, node: &Node) -> String {
    let loc = node.location();
    ctx.source[loc.start_offset()..loc.end_offset()].to_string()
}

fn is_simple_double_comparison(node: &Node) -> bool {
    let call = match node.as_call_node() { Some(c) => c, None => return false };
    if node_name!(call) != "==" { return false; }
    let recv = match call.receiver() { Some(r) => r, None => return false };
    if !is_lvar(&recv) { return false; }
    let args: Vec<Node> = match call.arguments() {
        Some(a) => a.arguments().iter().collect(),
        None => return false,
    };
    args.len() == 1 && is_lvar(&args[0])
}

fn walk(
    ctx: &CheckContext,
    node: &ruby_prism::OrNode,
    vars: &mut HashSet<String>,
    ranges: &mut Vec<(usize, usize)>,
    allow_method_comparison: bool,
    abort: &mut bool,
) {
    if *abort { return; }
    walk_one(ctx, &node.left(), vars, ranges, allow_method_comparison, abort);
    walk_one(ctx, &node.right(), vars, ranges, allow_method_comparison, abort);
}

fn walk_one(
    ctx: &CheckContext,
    node: &Node,
    vars: &mut HashSet<String>,
    ranges: &mut Vec<(usize, usize)>,
    allow_method_comparison: bool,
    abort: &mut bool,
) {
    if *abort { return; }
    if let Some(or) = node.as_or_node() {
        walk(ctx, &or, vars, ranges, allow_method_comparison, abort);
        return;
    }
    if is_simple_double_comparison(node) {
        *abort = true;
        return;
    }
    if let Some((var_src, obj_is_call)) = simple_comparison_src(ctx, node, allow_method_comparison) {
        // RuboCop: `return if allow_method_comparison? && obj.call_type?`
        if allow_method_comparison && obj_is_call { return; }
        vars.insert(var_src);
        if vars.len() > 1 {
            // Don't abort — in RuboCop, this resets (stops adding values). Mirror: stop collecting.
            *abort = true;
            return;
        }
        let loc = node.location();
        ranges.push((loc.start_offset(), loc.end_offset()));
    }
}
