//! Shared helpers for Style/HashTransformKeys and Style/HashTransformValues.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/hash_transform_method.rb

use crate::node_name;
use ruby_prism::{BlockNode, CallNode, Node, Visit};

/// Outcome of matching a hash-transform pattern. Offsets are for the outer node
/// (the whole block/call that constitutes the offense).
pub struct Match {
    pub start_offset: usize,
    pub end_offset: usize,
    /// Human name for the pattern (`each_with_object`, `Hash[_.map {...}]`, `map {...}.to_h`, `to_h {...}`).
    pub pattern_label: &'static str,
}

/// Return true if `node` is recognized as returning a hash:
/// - hash literal
/// - call to a known hash-returning method (to_h, merge, invert, etc.)
/// - block whose inner call is group_by/to_h/tally/transform_keys/transform_values...
pub fn is_hash_receiver(node: &Node) -> bool {
    match node {
        Node::HashNode { .. } => true,
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            let name = node_name!(c);
            matches!(
                name.as_ref(),
                "to_h" | "to_hash" | "merge" | "merge!" | "update" | "invert" | "except" | "tally"
            )
        }
        _ => {
            // BlockNode in Prism is stored as the .block() of a CallNode, so receivers
            // that are "blocks" appear as CallNodes with block attached.
            false
        }
    }
}

/// Variant that also recognizes a CallNode-with-block receiver whose method is one of
/// group_by/to_h/tally/transform_keys/transform_values.
pub fn is_hash_receiver_expr(node: &Node) -> bool {
    if is_hash_receiver(node) {
        return true;
    }
    if let Some(c) = node.as_call_node() {
        if c.block().is_some() {
            let name = node_name!(c);
            return matches!(
                name.as_ref(),
                "group_by"
                    | "to_h"
                    | "tally"
                    | "transform_keys"
                    | "transform_keys!"
                    | "transform_values"
                    | "transform_values!"
            );
        }
    }
    false
}

/// Identifies whether a Node is a local-variable read with a specific name.
pub fn is_lvar_ref(node: &Node, name: &str) -> bool {
    if let Some(n) = node.as_local_variable_read_node() {
        return String::from_utf8_lossy(n.name().as_slice()) == name;
    }
    false
}

/// Check whether the subtree contains any local-variable reference with `name`.
pub fn subtree_references(node: &Node, name: &str) -> bool {
    struct V<'a> {
        name: &'a str,
        found: bool,
    }
    impl<'p> Visit<'p> for V<'_> {
        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'p>) {
            if String::from_utf8_lossy(n.name().as_slice()) == self.name {
                self.found = true;
            }
        }
    }
    let mut v = V { name, found: false };
    v.visit(node);
    v.found
}

/// Block parameter info for `each_with_object` pattern.
/// `|(first, second), memo|` — where first/second come from an MultiTargetNode / MultiWriteNode
/// (destructuring) and memo is a simple required parameter.
pub struct EwoParams {
    pub first: String,  // key arg name (keys cop) OR key arg (values cop)
    pub second: String, // value arg name
    pub memo: String,
}

/// Extract `(first, second, memo)` from a BlockNode whose parameters look like
/// `|(a, b), memo|`. Returns None if shape doesn't match.
pub fn extract_ewo_params(block: &BlockNode) -> Option<EwoParams> {
    let params_node = block.parameters()?;
    let block_params = params_node.as_block_parameters_node()?;
    let inner = block_params.parameters()?;
    // Collect required params
    let requireds: Vec<Node> = inner.requireds().iter().collect();
    if requireds.len() != 2 {
        return None;
    }
    // First required is destructure (MultiTargetNode)
    let destruct = requireds[0].as_multi_target_node()?;
    let lefts: Vec<Node> = destruct.lefts().iter().collect();
    if lefts.len() != 2 {
        return None;
    }
    let first = required_param_name(&lefts[0])?;
    let second = required_param_name(&lefts[1])?;
    // Second required is memo (simple required param)
    let memo = required_param_name(&requireds[1])?;
    Some(EwoParams {
        first,
        second,
        memo,
    })
}

/// Simple block params `|k, v|` — returns (key, val).
pub fn extract_simple_two_params(block: &BlockNode) -> Option<(String, String)> {
    let params_node = block.parameters()?;
    let block_params = params_node.as_block_parameters_node()?;
    let inner = block_params.parameters()?;
    let requireds: Vec<Node> = inner.requireds().iter().collect();
    if requireds.len() != 2 {
        return None;
    }
    let a = required_param_name(&requireds[0])?;
    let b = required_param_name(&requireds[1])?;
    Some((a, b))
}

fn required_param_name(node: &Node) -> Option<String> {
    if let Some(n) = node.as_required_parameter_node() {
        return Some(String::from_utf8_lossy(n.name().as_slice()).into_owned());
    }
    None
}

/// Extract the single body call from a block body that is a single assignment:
/// returns the IndexOperatorWriteNode or equivalent `memo[KEY] = VAL` call.
/// Returns Some(KEY, VAL) as owned Nodes (borrowed via the tree lifetime).
pub fn body_single_stmt<'a>(block: &BlockNode<'a>) -> Option<Node<'a>> {
    let body = block.body()?;
    let stmts = body.as_statements_node()?;
    let mut it = stmts.body().iter();
    let first = it.next()?;
    if it.next().is_some() {
        return None;
    }
    Some(first)
}

/// Match `memo[KEY] = VAL` — in Prism this is a CallNode with method `:[]=`,
/// receiver `memo` (lvar), and two arguments (KEY, VAL).
pub fn match_index_assign<'a>(
    node: &Node<'a>,
    memo_name: &str,
) -> Option<(Node<'a>, Node<'a>)> {
    let call = node.as_call_node()?;
    if node_name!(call).as_ref() != "[]=" {
        return None;
    }
    let recv = call.receiver()?;
    if !is_lvar_ref(&recv, memo_name) {
        return None;
    }
    let args: Vec<Node> = call.arguments()?.arguments().iter().collect();
    if args.len() != 2 {
        return None;
    }
    // Can't clone Node, so reconstruct via the original iter
    let mut it = call.arguments()?.arguments().iter();
    let key = it.next()?;
    let val = it.next()?;
    Some((key, val))
}

/// Match body of `map { |k, v| [K_EXPR, V_EXPR] }` — a two-element array literal.
/// Returns Some((k_expr, v_expr)) if body is a single two-element ArrayNode.
pub fn match_array_pair<'a>(block: &BlockNode<'a>) -> Option<(Node<'a>, Node<'a>)> {
    let stmt = body_single_stmt(block)?;
    let arr = stmt.as_array_node()?;
    let elements: Vec<Node> = arr.elements().iter().collect();
    if elements.len() != 2 {
        return None;
    }
    let mut it = arr.elements().iter();
    let a = it.next()?;
    let b = it.next()?;
    Some((a, b))
}

/// Is this CallNode `receiver.each_with_object({})` (with empty hash literal arg)?
pub fn is_each_with_object_empty_hash(call: &CallNode) -> bool {
    if node_name!(call).as_ref() != "each_with_object" {
        return false;
    }
    let args = match call.arguments() {
        Some(a) => a,
        None => return false,
    };
    let arg_vec: Vec<Node> = args.arguments().iter().collect();
    if arg_vec.len() != 1 {
        return false;
    }
    let hash = match arg_vec[0].as_hash_node() {
        Some(h) => h,
        None => return false,
    };
    hash.elements().iter().count() == 0
}

/// Match the outer `Hash[inner_block]` CallNode: constant `Hash`, method `:[]`, one arg
/// which is a CallNode-with-block of map/collect on a hash receiver.
/// Returns Some((BlockNode, inner_call)) if match.
pub fn match_hash_brackets_map<'a>(outer: &CallNode<'a>) -> Option<(BlockNode<'a>, CallNode<'a>)> {
    if node_name!(outer).as_ref() != "[]" {
        return None;
    }
    // Receiver must be constant `Hash`
    let recv = outer.receiver()?;
    let c = recv.as_constant_read_node()?;
    if String::from_utf8_lossy(c.name().as_slice()) != "Hash" {
        return None;
    }
    let args: Vec<Node> = outer.arguments()?.arguments().iter().collect();
    if args.len() != 1 {
        return None;
    }
    // The arg is itself a CallNode that has a block (the map/collect call).
    let inner_call = args[0].as_call_node()?;
    let block_node = inner_call.block()?;
    let block = block_node.as_block_node()?;
    let name = node_name!(inner_call);
    if !matches!(name.as_ref(), "map" | "collect") {
        return None;
    }
    // Check the map/collect receiver is a hash receiver
    let map_recv = inner_call.receiver()?;
    if !is_hash_receiver_expr(&map_recv) {
        return None;
    }
    Some((block, inner_call))
}

/// Build multiple edits for a hash-transform correction.
///
/// Translates RuboCop's 4-step Autocorrection:
/// 1. strip_prefix_and_suffix: remove "Hash[" prefix + "]" suffix (for Hash[] case)
/// 2. set_new_method_name: replace old method selector with new_method
/// 3. set_new_arg_name: replace block params with |new_arg|
/// 4. set_new_body_expression: replace block body expr with body_src (wrapping bare hash)
/// Additionally for map.to_h: strip ".to_h" suffix (from block_end to outer_end).
///
/// Parameters:
/// - `source`: full source text
/// - `outer_start`/`outer_end`: the offense byte range
/// - `block_call`: the inner call node (map/each_with_object/to_h)
/// - `block`: the block node attached to block_call
/// - `new_method`: "transform_values" or "transform_keys"
/// - `new_arg`: the single param name for the new block
/// - `body_expr`: the body expression node to use as new body
/// - `map_to_h_outer`: for map.to_h, Some(outer_call_end) to strip trailing ".to_h"
///
/// Returns a list of (start, end, replacement) edits, sorted in order.
pub fn hash_transform_edits<'a>(
    source: &str,
    outer_start: usize,
    outer_end: usize,
    block_call: &CallNode<'a>,
    block: &BlockNode<'a>,
    new_method: &str,
    new_arg: &str,
    body_expr: &Node<'a>,
    map_to_h_outer_end: Option<usize>,
    hash_brackets_outer: bool,
) -> Vec<(usize, usize, String)> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();

    // Edit 1a: For Hash[] case — remove "Hash[" prefix
    if hash_brackets_outer {
        edits.push((outer_start, outer_start + 5, String::new())); // remove "Hash["
        edits.push((outer_end - 1, outer_end, String::new())); // remove "]"
    }

    // Edit 1b: For map.to_h case — remove ".to_h" (from block_end to outer_end)
    if let Some(to_h_end) = map_to_h_outer_end {
        let block_end = block.location().end_offset();
        if block_end < to_h_end {
            edits.push((block_end, to_h_end, String::new()));
        }
    }

    // Edit 2: Replace method selector (and args if call has closing paren — e.g. each_with_object({}))
    // RuboCop: `range = selector; if send_end = block_node.send_node.loc.end; range = range.begin.join(send_end)`
    if let Some(sel) = block_call.message_loc() {
        let sel_end = if let Some(close) = block_call.closing_loc() {
            close.end_offset()
        } else {
            sel.end_offset()
        };
        edits.push((sel.start_offset(), sel_end, new_method.to_string()));
    }

    // Edit 3: Replace block params with |new_arg|
    if let Some(params) = block.parameters() {
        edits.push((
            params.location().start_offset(),
            params.location().end_offset(),
            format!("|{}|", new_arg),
        ));
    }

    // Edit 4: Replace block body with new body expression
    // Check if body is a bare hash (needs `{ }` wrapping)
    // Both HashNode (braced `{k: v}`) and KeywordHashNode (bare `k: v` in method args) possible.
    let body_src = &source[body_expr.location().start_offset()..body_expr.location().end_offset()];
    let is_bare_hash = (body_expr.as_hash_node().is_some() || body_expr.as_keyword_hash_node().is_some())
        && !body_src.trim_start().starts_with('{');
    let new_body_src = if is_bare_hash {
        format!("{{ {} }}", body_src)
    } else {
        body_src.to_string()
    };

    if let Some(body_node) = block.body() {
        let stmts_start = body_node.location().start_offset();
        let stmts_end = body_node.location().end_offset();
        edits.push((stmts_start, stmts_end, new_body_src));
    }

    // Sort by start offset ascending
    edits.sort_by_key(|(s, _, _)| *s);
    edits
}

/// Match `hash.map { |k, v| [...] }.to_h` — outer CallNode with method `:to_h`, no args,
/// no block, whose receiver is a map/collect call-with-block.
pub fn match_map_to_h<'a>(outer: &CallNode<'a>) -> Option<(BlockNode<'a>, CallNode<'a>)> {
    if node_name!(outer).as_ref() != "to_h" {
        return None;
    }
    let recv = outer.receiver()?;
    let inner_call = recv.as_call_node()?;
    let block_node = inner_call.block()?;
    let block = block_node.as_block_node()?;
    let name = node_name!(inner_call);
    if !matches!(name.as_ref(), "map" | "collect") {
        return None;
    }
    let map_recv = inner_call.receiver()?;
    if !is_hash_receiver_expr(&map_recv) {
        return None;
    }
    Some((block, inner_call))
}
