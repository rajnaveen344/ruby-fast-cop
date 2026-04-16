//! Shared logic for `Style/NegatedIf`, `Style/NegatedUnless`, and `Style/NegatedWhile`.
//!
//! Ports RuboCop's `NegativeConditional` mixin:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/negative_conditional.rb
//!
//! Three node-pattern matchers:
//!   * `empty_condition?(node)` = `(begin)` — parenthesized group with no body.
//!   * `single_negative?(node)` = `(send !(send _ :!) :!)` — a `!` (or `not`)
//!      whose receiver is not itself another `!`.
//!   * The mixin walks `condition.children.last while condition.begin_type?`,
//!      which in Prism is ParenthesesNode containing a StatementsNode; we
//!      descend to the last statement of the group.

use ruby_prism::Node;

pub const MSG: &str = "Favor `%<inverse>s` over `%<current>s` for negative conditions.";

/// `(begin)` in RuboCop parlance = empty parenthesized group like `()`.
/// In Prism that is a `ParenthesesNode` with `body: None`.
pub fn is_empty_condition(node: &Node) -> bool {
    node.as_parentheses_node()
        .map(|p| p.body().is_none())
        .unwrap_or(false)
}

/// Mirror `condition.children.last while condition.begin_type?`: when the
/// condition is a parenthesized group containing a `StatementsNode`, descend
/// to the last statement and repeat.
pub fn unwrap_begin<'a>(node: Node<'a>) -> Node<'a> {
    let mut current = node;
    loop {
        let Some(paren) = current.as_parentheses_node() else { break };
        let Some(body) = paren.body() else { break };
        // body is typically a StatementsNode; take its last child.
        if let Some(stmts) = body.as_statements_node() {
            let items: Vec<Node<'a>> = stmts.body().iter().collect();
            let Some(last) = items.into_iter().last() else { break };
            current = last;
            continue;
        }
        // Body present but not a StatementsNode (rare): stop.
        break;
    }
    current
}

/// `single_negative?(node)` — the node is `!x` (or `not x`) where `x` is itself
/// not a `!` / `not` send. This rejects `!!x` (double negation).
pub fn is_single_negative(node: &Node, source: &str) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if !call_is_bang(&call, source) {
        return false;
    }
    let Some(receiver) = call.receiver() else { return false };
    let inner_is_bang = receiver
        .as_call_node()
        .map(|c| call_is_bang(&c, source))
        .unwrap_or(false);
    !inner_is_bang
}

/// A call is `!foo` or `not foo` when the method name is `!` and the
/// message token itself is literally `!` or `not` (Prism represents both as
/// the `!` method).
fn call_is_bang(call: &ruby_prism::CallNode, source: &str) -> bool {
    if String::from_utf8_lossy(call.name().as_slice()) != "!" {
        return false;
    }
    let Some(msg) = call.message_loc() else { return false };
    let text = &source[msg.start_offset()..msg.end_offset()];
    text == "!" || text == "not"
}

/// Outcome of `check_negative_conditional`: either no match, or the negated
/// inner call (needed for autocorrection) alongside its enclosing condition
/// after any `(begin)` unwrapping.
pub struct NegativeMatch<'a> {
    /// The `!x` / `not x` call node as it appears after unwrapping parens.
    pub negated_call: Node<'a>,
}

/// Port of `check_negative_conditional` from the mixin. Returns `Some` when
/// the guarded condition is a single negation (and the condition is not
/// empty). Callers are responsible for the per-cop early returns
/// (style/if-vs-unless/etc.) and for emitting the offense themselves.
pub fn match_negative_condition<'a>(condition: Node<'a>, source: &str) -> Option<NegativeMatch<'a>> {
    if is_empty_condition(&condition) {
        return None;
    }
    let unwrapped = unwrap_begin(condition);
    if !is_single_negative(&unwrapped, source) {
        return None;
    }
    Some(NegativeMatch {
        negated_call: unwrapped,
    })
}
