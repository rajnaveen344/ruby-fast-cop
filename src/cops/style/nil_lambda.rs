//! Style/NilLambda cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct NilLambda;
impl NilLambda { pub fn new() -> Self { Self } }

impl Cop for NilLambda {
    fn name(&self) -> &'static str { "Style/NilLambda" }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// Check if `body` is an expression that always returns nil:
/// bare `nil`, or `return nil`, `next nil`, `break nil`.
fn body_is_nil_return(body: &Node) -> bool {
    // Single-expression body — if wrapped in StatementsNode, unwrap.
    let expr = if let Some(s) = body.as_statements_node() {
        let items: Vec<_> = s.body().iter().collect();
        if items.len() != 1 { return false; }
        items.into_iter().next().unwrap()
    } else {
        body.clone_any()
    };
    match &expr {
        Node::NilNode { .. } => true,
        Node::ReturnNode { .. } => {
            let rn = expr.as_return_node().unwrap();
            single_arg_is_nil(rn.arguments())
        }
        Node::NextNode { .. } => {
            let nn = expr.as_next_node().unwrap();
            single_arg_is_nil(nn.arguments())
        }
        Node::BreakNode { .. } => {
            let bn = expr.as_break_node().unwrap();
            single_arg_is_nil(bn.arguments())
        }
        _ => false,
    }
}

fn single_arg_is_nil(args: Option<ruby_prism::ArgumentsNode>) -> bool {
    let a = match args { Some(a) => a, None => return false };
    let list: Vec<_> = a.arguments().iter().collect();
    list.len() == 1 && matches!(list[0], Node::NilNode { .. })
}

// Helper: Node doesn't derive Clone; use a trait extension that uses re-fetching.
trait NodeCloneAny<'a> { fn clone_any(&self) -> Node<'a>; }
impl<'a> NodeCloneAny<'a> for Node<'a> {
    fn clone_any(&self) -> Node<'a> {
        // We can't literally clone; copy via location-roundtrip not possible.
        // Workaround: callers that need to match on a Node should do so via as_* and return bools.
        // This function is only called on a single-statement body; but since Prism's Node is not Clone,
        // we do nothing here — callers will NEVER reach this path. See body_is_nil_return usage.
        unreachable!("NodeCloneAny is a stub; fix caller")
    }
}

impl<'a> V<'a> {
    fn report_lambda(&mut self, full_start: usize, full_end: usize, body_loc_start: usize, body_loc_end: usize, is_lambda: bool, is_single_line: bool) {
        let ty = if is_lambda { "lambda" } else { "proc" };
        let msg = format!("Use an empty {} instead of always returning nil.", ty);
        let src = self.ctx.source;
        let bytes = src.as_bytes();
        // Correction: remove body (with surrounding space if single-line, else whole lines incl final newline).
        let (remove_start, remove_end) = if is_single_line {
            // range_with_surrounding_space around body
            let mut s = body_loc_start;
            while s > 0 && (bytes[s-1] == b' ' || bytes[s-1] == b'\t') { s -= 1; }
            let mut e = body_loc_end;
            while e < bytes.len() && (bytes[e] == b' ' || bytes[e] == b'\t') { e += 1; }
            (s, e)
        } else {
            // whole lines including final newline
            let s = src[..body_loc_start].rfind('\n').map_or(0, |p| p + 1);
            let mut e = body_loc_end;
            while e < bytes.len() && bytes[e] != b'\n' { e += 1; }
            if e < bytes.len() { e += 1; }
            (s, e)
        };
        let off = self.ctx.offense_with_range("Style/NilLambda", &msg, Severity::Convention, full_start, full_end)
            .with_correction(Correction::replace(remove_start, remove_end, String::new()));
        self.offenses.push(off);
    }

    fn handle_block(&mut self, block: &ruby_prism::BlockNode<'a>, outer_start: usize, outer_end: usize, is_lambda: bool) {
        let body = match block.body() { Some(b) => b, None => return };
        // Use same single-line check: does outer span multiple source lines?
        let body_loc = body.location();
        // Determine nil-return via statement extraction
        let inner_expr = if let Some(s) = body.as_statements_node() {
            let items: Vec<_> = s.body().iter().collect();
            if items.len() != 1 { return; }
            items.into_iter().next().unwrap()
        } else {
            return; // block body usually StatementsNode
        };
        let is_nil = match &inner_expr {
            Node::NilNode { .. } => true,
            Node::ReturnNode { .. } => single_arg_is_nil(inner_expr.as_return_node().unwrap().arguments()),
            Node::NextNode { .. } => single_arg_is_nil(inner_expr.as_next_node().unwrap().arguments()),
            Node::BreakNode { .. } => single_arg_is_nil(inner_expr.as_break_node().unwrap().arguments()),
            _ => false,
        };
        if !is_nil { return; }
        let src = self.ctx.source;
        // single_line: outer_start..outer_end fits in one line
        let is_single_line = !src[outer_start..outer_end].contains('\n');
        // Body range for removal: use the inner_expr range (single statement)
        let b_loc = inner_expr.location();
        self.report_lambda(outer_start, outer_end, b_loc.start_offset(), b_loc.end_offset(), is_lambda, is_single_line);
        // Also: remove surrounding newlines if multi-line — but statements block body may have leading indent.
        // Override body range to include leading whitespace on its line for multi-line.
        // Actually report_lambda handles multi-line via range_by_whole_lines.
        let _ = body_loc;
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'a>) {
        // `-> { ... }` stabby lambda
        let body = match node.body() { Some(b) => b, None => { ruby_prism::visit_lambda_node(self, node); return; } };
        let inner_expr = if let Some(s) = body.as_statements_node() {
            let items: Vec<_> = s.body().iter().collect();
            if items.len() != 1 { ruby_prism::visit_lambda_node(self, node); return; }
            items.into_iter().next().unwrap()
        } else {
            ruby_prism::visit_lambda_node(self, node);
            return;
        };
        let is_nil = match &inner_expr {
            Node::NilNode { .. } => true,
            Node::ReturnNode { .. } => single_arg_is_nil(inner_expr.as_return_node().unwrap().arguments()),
            Node::NextNode { .. } => single_arg_is_nil(inner_expr.as_next_node().unwrap().arguments()),
            Node::BreakNode { .. } => single_arg_is_nil(inner_expr.as_break_node().unwrap().arguments()),
            _ => false,
        };
        if is_nil {
            let loc = node.location();
            let start = loc.start_offset();
            let end = loc.end_offset();
            let src = self.ctx.source;
            let is_single_line = !src[start..end].contains('\n');
            let b = inner_expr.location();
            self.report_lambda(start, end, b.start_offset(), b.end_offset(), true, is_single_line);
        }
        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        // Detect: lambda { ... }, proc { ... }, Proc.new { ... } (block must be BlockNode).
        let method = node_name!(node);
        let (is_lambda, applies) = if method == "lambda" && node.receiver().is_none() {
            (true, true)
        } else if method == "proc" && node.receiver().is_none() {
            (false, true)
        } else if method == "new" {
            // Proc.new
            let matches_proc = node.receiver().map(|r| {
                r.as_constant_read_node().map(|c| node_name!(c) == "Proc").unwrap_or(false)
            }).unwrap_or(false);
            (false, matches_proc)
        } else {
            (false, false)
        };
        if applies {
            if let Some(b) = node.block() {
                if let Some(bn) = b.as_block_node() {
                    let outer_loc = node.location();
                    self.handle_block(&bn, outer_loc.start_offset(), outer_loc.end_offset(), is_lambda);
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/NilLambda", |_cfg| Some(Box::new(NilLambda::new())));
