//! Style/RedundantSelfAssignmentBranch
//!
//! `foo = cond ? foo : bar` → `foo = bar unless cond`
//! `foo = cond ? bar : foo` → `foo = bar if cond`
//! Also handles non-ternary `if/else` form. Skips elsif, multiple statements,
//! and non-local variable LHS (ivar/cvar/gvar).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{IfNode, LocalVariableWriteNode, Node};

const MSG: &str = "Remove the self-assignment branch.";

#[derive(Default)]
pub struct RedundantSelfAssignmentBranch;

impl RedundantSelfAssignmentBranch {
    pub fn new() -> Self { Self }
}

fn src_of(node: &Node, source: &str) -> String {
    let loc = node.location();
    source[loc.start_offset()..loc.end_offset()].to_string()
}

fn single_stmt<'a>(body: Option<Node<'a>>) -> Option<Node<'a>> {
    let body = body?;
    if let Some(stmts) = body.as_statements_node() {
        let list: Vec<_> = stmts.body().iter().collect();
        if list.len() == 1 { Some(list.into_iter().next().unwrap()) } else { None }
    } else {
        Some(body)
    }
}

fn body_is_multi(body: Option<&Node>) -> bool {
    match body {
        None => false,
        Some(b) => {
            if let Some(s) = b.as_statements_node() {
                s.body().iter().count() > 1
            } else {
                false
            }
        }
    }
}

fn is_elsif(if_node: &IfNode, ctx: &CheckContext) -> bool {
    let start = if_node.location().start_offset();
    ctx.source[start..].starts_with("elsif")
}

fn is_self_reference(branch: &Option<Node>, var_name: &str) -> bool {
    let Some(b) = branch else { return false };
    if let Some(r) = b.as_local_variable_read_node() {
        String::from_utf8_lossy(r.name().as_slice()) == var_name
    } else {
        false
    }
}

fn heredoc_trailing(node: &Node, source: &str) -> Option<(usize, usize)> {
    let (open_start, close_end) = match node {
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            (s.opening_loc()?.start_offset(), s.closing_loc()?.end_offset())
        }
        Node::InterpolatedStringNode { .. } => {
            let s = node.as_interpolated_string_node().unwrap();
            (s.opening_loc()?.start_offset(), s.closing_loc()?.end_offset())
        }
        _ => return None,
    };
    if !source.as_bytes()[open_start..].starts_with(b"<<") { return None; }
    let node_end = node.location().end_offset();
    if close_end <= node_end { return None; }
    Some((node_end, close_end))
}

impl Cop for RedundantSelfAssignmentBranch {
    fn name(&self) -> &'static str { "Style/RedundantSelfAssignmentBranch" }

    fn check_local_variable_write(
        &self,
        node: &LocalVariableWriteNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let value = node.value();
        let Some(if_node) = value.as_if_node() else { return vec![] };

        if is_elsif(&if_node, ctx) { return vec![]; }

        // Must have an else clause (not another elsif).
        let Some(else_clause) = if_node.subsequent() else { return vec![] };
        if matches!(else_clause, Node::IfNode { .. }) { return vec![]; }
        let Some(else_inner) = else_clause.as_else_node() else { return vec![] };

        // Reject multi-statement branches.
        let if_body = if_node.statements().map(|s| s.as_node());
        let else_body = else_inner.statements().map(|s| s.as_node());
        if body_is_multi(if_body.as_ref()) || body_is_multi(else_body.as_ref()) {
            return vec![];
        }

        let var_name = String::from_utf8_lossy(node.name().as_slice()).into_owned();

        // Recompute single stmt fresh each time (Node is not Clone).
        let if_is_self = is_self_reference(
            &single_stmt(if_node.statements().map(|s| s.as_node())),
            &var_name,
        );
        let else_is_self = is_self_reference(
            &single_stmt(else_inner.statements().map(|s| s.as_node())),
            &var_name,
        );

        let (offense_branch, opposite_branch, keyword) = if if_is_self {
            let ob = single_stmt(if_node.statements().map(|s| s.as_node())).unwrap();
            let opp = single_stmt(else_inner.statements().map(|s| s.as_node()));
            (ob, opp, "unless")
        } else if else_is_self {
            let ob = single_stmt(else_inner.statements().map(|s| s.as_node())).unwrap();
            let opp = single_stmt(if_node.statements().map(|s| s.as_node()));
            (ob, opp, "if")
        } else {
            return vec![];
        };

        let off_loc = offense_branch.location();
        let offense_start = off_loc.start_offset();
        let offense_end = off_loc.end_offset();

        let cond = if_node.predicate();
        let cond_src = src_of(&cond, ctx.source);

        let assignment_value = match &opposite_branch {
            Some(n) => src_of(n, ctx.source),
            None => "nil".to_string(),
        };

        let mut replacement = format!("{} {} {}", assignment_value, keyword, cond_src);
        if let Some(ref opp) = opposite_branch {
            if let Some((hs, he)) = heredoc_trailing(opp, ctx.source) {
                // Strip a trailing newline so we don't duplicate the source's
                // line break past the `end` keyword.
                let slice = &ctx.source[hs..he];
                let trimmed = slice.strip_suffix('\n').unwrap_or(slice);
                replacement.push_str(trimmed);
            }
        }

        let if_loc = if_node.location();
        let correction = Correction::replace(if_loc.start_offset(), if_loc.end_offset(), replacement);

        vec![
            ctx.offense_with_range(self.name(), MSG, Severity::Convention, offense_start, offense_end)
                .with_correction(correction),
        ]
    }
}

crate::register_cop!("Style/RedundantSelfAssignmentBranch", |_cfg| Some(Box::new(RedundantSelfAssignmentBranch::new())));
