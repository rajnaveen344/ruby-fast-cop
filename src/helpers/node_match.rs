//! Combinator helpers that compress common Prism node matching idioms.
//!
//! These helpers don't try to mirror RuboCop's `def_node_matcher` DSL. They
//! expose plain Rust functions for the node shapes we match most often:
//! call-by-name, constant-by-name, nil-literal, parenthesis unwrap, and so on.
//!
//! ## Why
//!
//! Without these helpers, a check like "is this node `foo.freeze` with no
//! args" unfolds into ~10 lines of `node_name!(...)` + `receiver()` +
//! `arguments()` ceremony. With them, it's one or two lines — closer to the
//! Ruby source the cop was translated from.
//!
//! ## Example
//!
//! ```ignore
//! use crate::helpers::node_match as m;
//!
//! // Before:
//! //   let method = node_name!(node);
//! //   if method != "freeze" { return; }
//! //   if node.arguments().is_some() { return; }
//! //   let recv = match node.receiver() { Some(r) => r, None => return };
//!
//! // After:
//! if let Some(recv) = m::call_receiver_no_args(&node.as_node(), "freeze") {
//!     // `recv` is the receiver, method is confirmed `freeze`, no args
//! }
//! ```
//!
//! Keep this file small. If a helper is only used in one cop, inline it there
//! instead of adding it here.

use crate::node_name;
use ruby_prism::{CallNode, Node};
use std::borrow::Cow;

// ── Call-node helpers ────────────────────────────────────────────────────

/// Returns the method name of `node` if it's a `CallNode`, else `None`.
pub fn call_method_name<'a>(node: &Node<'a>) -> Option<Cow<'a, str>> {
    node.as_call_node()
        .map(|c| String::from_utf8_lossy(c.name().as_slice()))
}

/// Returns the `CallNode` view of `node` if its method name equals `name`.
pub fn as_call_named<'a>(node: &Node<'a>, name: &str) -> Option<CallNode<'a>> {
    let call = node.as_call_node()?;
    if node_name!(call) == name { Some(call) } else { None }
}

/// True if `node` is a `CallNode` whose method name equals `name`.
pub fn is_call_named(node: &Node<'_>, name: &str) -> bool {
    as_call_named(node, name).is_some()
}

/// True if `node` is a `CallNode` whose method name is one of `names`.
pub fn is_call_named_any(node: &Node<'_>, names: &[&str]) -> bool {
    match call_method_name(node) {
        Some(m) => names.iter().any(|n| m.as_ref() == *n),
        None => false,
    }
}

/// Matches `<recv>.<name>` with no arguments and returns `<recv>`.
///
/// Use for idioms like `foo.freeze`, `foo.to_s`, `foo.dup` where an extra
/// argument would invalidate the pattern.
pub fn call_receiver_no_args<'a>(node: &Node<'a>, name: &str) -> Option<Node<'a>> {
    let call = as_call_named(node, name)?;
    if call.arguments().is_some() {
        return None;
    }
    call.receiver()
}

// ── Constant helpers ─────────────────────────────────────────────────────

/// Returns the simple (rightmost) name of a constant node.
///
/// Handles both `ConstantReadNode` (`Foo`) and `ConstantPathNode` (`A::B::C`
/// returns `C`). Returns `None` for any other node kind.
pub fn constant_simple_name<'a>(node: &Node<'a>) -> Option<Cow<'a, str>> {
    if let Some(c) = node.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()));
    }
    if let Some(c) = node.as_constant_path_node() {
        return c.name().map(|n| String::from_utf8_lossy(n.as_slice()));
    }
    None
}

/// True if `node` is a top-level constant (no namespace) named `name`.
///
/// Matches `Foo` and `::Foo`, but not `A::Foo`. Use for cases like
/// `rescue StandardError` where a nested reference would be a different class.
pub fn is_toplevel_constant_named(node: &Node<'_>, name: &str) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        return node_name!(c) == name;
    }
    if let Some(c) = node.as_constant_path_node() {
        // parent() = None means `::Foo` (cbase) or un-namespaced path;
        // both read as top-level for our purposes.
        return c.parent().is_none()
            && c.name()
                .map(|n| String::from_utf8_lossy(n.as_slice()) == name)
                .unwrap_or(false);
    }
    false
}

// ── Structural helpers ───────────────────────────────────────────────────

/// Strips one layer of `(expr)` parens and returns the inner node if the
/// parentheses wrap exactly one expression.
///
/// Specifically: if `node` is a `ParenthesesNode` whose body is either a
/// single expression or a single-statement `StatementsNode`, returns that
/// expression. Returns `None` for empty parens, multi-statement bodies, or
/// non-paren nodes.
pub fn unwrap_single_parens<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let paren = node.as_parentheses_node()?;
    let body = paren.body()?;
    if let Some(stmts) = body.as_statements_node() {
        if stmts.body().len() == 1 {
            return stmts.body().first();
        }
        return None;
    }
    Some(body)
}

// ── Literal helpers ──────────────────────────────────────────────────────

/// True if `node` is a `NilNode`.
pub fn is_nil(node: &Node<'_>) -> bool {
    matches!(node, Node::NilNode { .. })
}

/// True if `node` is an "always immutable" literal — numbers, symbols,
/// booleans, or nil. Excludes strings (which depend on magic comments) and
/// regexps (which depend on target Ruby version).
pub fn is_always_immutable_literal(node: &Node<'_>) -> bool {
    matches!(
        node,
        Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
    )
}
