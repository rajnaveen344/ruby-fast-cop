//! Shared guard-clause detection utilities.
//!
//! Mirrors RuboCop's `node.guard_clause?` / `match_guard_clause?` pattern matcher
//! from rubocop-ast:
//!
//! ```text
//! [${(send nil? {:raise :fail} ...) return break next} single_line?]
//! ```
//!
//! The outer node is considered a "guard clause" if it's one of the terminating
//! expressions OR if it's an `and`/`or` whose right-hand side is such an expression.
//! The whole outer expression must fit on a single line.
//!
//! Used by `Style/GuardClause` and `Layout/EmptyLineAfterGuardClause`.

use ruby_prism::Node;

/// Returns `true` if `node` is considered a guard clause: either it's a terminating
/// expression (`raise`/`fail`/`throw` bare call, `return`, `break`, `next`), or an
/// `and`/`or` whose rhs is such an expression. Must be single-line.
pub fn is_guard_clause(node: &Node, source: &str) -> bool {
    // Must be single-line
    if !is_single_line(node, source) {
        return false;
    }

    // Check self or rhs-of-operator-keyword
    if match_terminator(node) {
        return true;
    }
    match node {
        Node::AndNode { .. } => {
            let and = node.as_and_node().unwrap();
            match_terminator(&and.right())
        }
        Node::OrNode { .. } => {
            let or = node.as_or_node().unwrap();
            match_terminator(&or.right())
        }
        _ => false,
    }
}

/// Returns true if the node is a bare `raise`/`fail`/`throw` call or a
/// `return`/`break`/`next` node (without checking and/or wrapping).
pub fn match_terminator(node: &Node) -> bool {
    match node {
        Node::ReturnNode { .. } | Node::BreakNode { .. } | Node::NextNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if call.receiver().is_some() {
                return false;
            }
            let name = String::from_utf8_lossy(call.name().as_slice());
            matches!(name.as_ref(), "raise" | "fail" | "throw")
        }
        _ => false,
    }
}

/// Check if a node is on a single line.
pub fn is_single_line(node: &Node, source: &str) -> bool {
    let loc = node.location();
    let start = loc.start_offset();
    let end = loc.end_offset();
    !source[start..end].contains('\n')
}
