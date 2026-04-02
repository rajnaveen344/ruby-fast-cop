//! Shared helpers for access modifier detection.
//!
//! Mirrors RuboCop's `node.bare_access_modifier?` and related checks.
//! Used by: EmptyLinesAroundAccessModifier, UselessAccessModifier,
//!          AccessModifierDeclarations, IndentationWidth.

use ruby_prism::Node;

/// Access modifier method names (including module_function).
pub const ACCESS_MODIFIERS: &[&str] = &["private", "protected", "public", "module_function"];

/// Access modifier method names (without module_function).
/// Used by Lint/UselessAccessModifier which doesn't treat module_function as an access modifier.
pub const ACCESS_MODIFIERS_WITHOUT_MODULE_FUNCTION: &[&str] = &["private", "protected", "public"];

/// Check if a CallNode is a bare access modifier (no receiver, no arguments, no block).
/// Equivalent to RuboCop's `node.bare_access_modifier?`.
pub fn is_bare_access_modifier(call: &ruby_prism::CallNode) -> bool {
    if call.receiver().is_some() {
        return false;
    }
    let name = String::from_utf8_lossy(call.name().as_slice());
    if !ACCESS_MODIFIERS.contains(&name.as_ref()) {
        return false;
    }
    let has_args = call
        .arguments()
        .map_or(false, |args| args.arguments().iter().next().is_some());
    !has_args && call.block().is_none()
}

/// Check if a CallNode is an access modifier (bare or with arguments).
/// Equivalent to RuboCop's `node.access_modifier?`.
pub fn is_access_modifier(call: &ruby_prism::CallNode) -> bool {
    if call.receiver().is_some() {
        return false;
    }
    let name = String::from_utf8_lossy(call.name().as_slice());
    ACCESS_MODIFIERS.contains(&name.as_ref())
}

/// Extract access modifier name from a Node if it's a bare access modifier.
/// Returns (name, msg_start_offset, msg_end_offset) if it is.
pub fn extract_bare_access_modifier(node: &Node) -> Option<(String, usize, usize)> {
    let call = node.as_call_node()?;
    if !is_bare_access_modifier(&call) {
        return None;
    }
    let msg_loc = call.message_loc()?;
    let name = String::from_utf8_lossy(call.name().as_slice()).to_string();
    Some((name, msg_loc.start_offset(), msg_loc.end_offset()))
}

/// Get the access modifier name from a CallNode (without checking if it's bare).
pub fn access_modifier_name(call: &ruby_prism::CallNode) -> String {
    String::from_utf8_lossy(call.name().as_slice()).to_string()
}
