//! Shared logic for Style/HashSlice and Style/HashExcept.
//! Mirrors RuboCop's `lib/rubocop/cop/mixin/hash_subset.rb`.

use ruby_prism::Node;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SubsetOp {
    /// k == X, X == k, k.eql?(X), X.eql?(k)
    Eq,
    /// k != X, X != k
    Ne,
    /// arr.include?(k), [..]include?(k)
    Include,
    /// k.in?(arr) (active_support)
    In,
    /// arr.exclude?(k) (active_support)
    Exclude,
}

#[derive(Debug)]
pub struct ExtractedSubset {
    /// Whether the body was wrapped in a `!`
    pub negated: bool,
    /// The matched operation
    pub op: SubsetOp,
    /// The "key" expression (the rhs that's not the block-key-arg).
    /// For `==`/`!=`/`eql?` this is the literal sym/str.
    /// For `include?`/`exclude?` this is the array.
    /// For `in?` this is the array.
    pub key_node_start: usize,
    pub key_node_end: usize,
    /// Source of key node (already sliced).
    pub key_source: String,
    /// Whether key_node is an array literal (for unpacking)
    pub key_is_array: bool,
    /// Whether key_node is a "literal" value (sym/str/array/etc) for `==`/`!=` safety check.
    pub key_is_sym_or_str: bool,
}

/// Try to match `block.body` to a hash-subset pattern using `key_arg_name` & `value_arg_name`.
/// `active_support` enables `in?`/`exclude?` matchers.
/// Returns `Some(ExtractedSubset)` if pattern matches, `None` otherwise.
pub fn extract_hash_subset<'a>(
    body: &Node<'a>,
    key_arg_name: &str,
    value_arg_name: &str,
    source: &'a str,
    active_support: bool,
) -> Option<ExtractedSubset> {
    // Step 1: Strip outer `!` if present (RuboCop's extract_body_if_negated)
    let (negated, call) = if let Some(c) = body.as_call_node() {
        if std::str::from_utf8(c.name().as_slice()).unwrap_or("") == "!" {
            // unwrap inner; possibly through ParenthesesNode
            let inner = c.receiver()?;
            let unwrapped = if let Node::ParenthesesNode { .. } = &inner {
                let pn = inner.as_parentheses_node().unwrap();
                let stmts = pn.body().and_then(|b| b.as_statements_node())?;
                let v: Vec<_> = stmts.body().iter().collect();
                if v.len() != 1 { return None; }
                v.into_iter().next().unwrap().as_call_node()?
            } else {
                inner.as_call_node()?
            };
            (true, unwrapped)
        } else {
            (false, c)
        }
    } else {
        return None;
    };
    let method = std::str::from_utf8(call.name().as_slice()).ok()?;

    // Check supported method
    let op = match method {
        "==" => SubsetOp::Eq,
        "!=" => SubsetOp::Ne,
        "eql?" => SubsetOp::Eq,
        "include?" => SubsetOp::Include,
        "in?" if active_support => SubsetOp::In,
        "exclude?" if active_support => SubsetOp::Exclude,
        _ => return None,
    };

    let recv = call.receiver()?;
    let args: Vec<_> = call.arguments().map_or(vec![], |a| a.arguments().iter().collect());

    match op {
        SubsetOp::Eq | SubsetOp::Ne => {
            if args.len() != 1 { return None; }
            // For eql? recv is key/value; for ==/!= either side may be the key.
            let (key_node, lhs_is_key) = if is_local_var_named(&recv, key_arg_name) {
                (&args[0], true)
            } else if is_local_var_named(&args[0], key_arg_name) {
                (&recv, false)
            } else {
                return None;
            };
            let _ = lhs_is_key;
            // Safety: for ==/!= only sym/str; eql? unconstrained
            if matches!(method, "==" | "!=") && !is_sym_or_str(key_node) { return None; }
            let loc = key_node.location();
            let src = source[loc.start_offset()..loc.end_offset()].to_string();
            Some(ExtractedSubset {
                negated,
                op,
                key_node_start: loc.start_offset(),
                key_node_end: loc.end_offset(),
                key_source: src,
                key_is_array: false,
                key_is_sym_or_str: true,
            })
        }
        SubsetOp::Include | SubsetOp::Exclude => {
            // arr.include?(k) - first arg must be key-arg
            if args.len() != 1 { return None; }
            if !is_local_var_named(&args[0], key_arg_name) { return None; }
            // Receiver should not be value-arg
            if is_local_var_named(&recv, value_arg_name) { return None; }
            // Range receiver -> skip (semantics differ)
            if is_range_receiver(&recv) { return None; }
            // Receiver should not be the key (k.include?('oo') is string substring check)
            if is_local_var_named(&recv, key_arg_name) { return None; }
            let loc = recv.location();
            let src = source[loc.start_offset()..loc.end_offset()].to_string();
            Some(ExtractedSubset {
                negated,
                op,
                key_node_start: loc.start_offset(),
                key_node_end: loc.end_offset(),
                key_source: src,
                key_is_array: matches!(&recv, Node::ArrayNode { .. }),
                key_is_sym_or_str: false,
            })
        }
        SubsetOp::In => {
            // k.in?(arr) - receiver must be key-arg
            if args.len() != 1 { return None; }
            if !is_local_var_named(&recv, key_arg_name) { return None; }
            // arg should not be value
            if is_local_var_named(&args[0], value_arg_name) { return None; }
            // Range arg -> skip
            if is_range_node(&args[0]) { return None; }
            let loc = args[0].location();
            let src = source[loc.start_offset()..loc.end_offset()].to_string();
            Some(ExtractedSubset {
                negated,
                op,
                key_node_start: loc.start_offset(),
                key_node_end: loc.end_offset(),
                key_source: src,
                key_is_array: matches!(&args[0], Node::ArrayNode { .. }),
                key_is_sym_or_str: false,
            })
        }
    }
}

fn is_local_var_named(node: &Node, name: &str) -> bool {
    if let Node::LocalVariableReadNode { .. } = node {
        let lv = node.as_local_variable_read_node().unwrap();
        return std::str::from_utf8(lv.name().as_slice()).unwrap_or("") == name;
    }
    false
}

fn is_sym_or_str(node: &Node) -> bool {
    matches!(node, Node::SymbolNode { .. } | Node::StringNode { .. })
}

fn is_range_node(node: &Node) -> bool {
    matches!(node, Node::RangeNode { .. })
        || matches!(node, Node::ParenthesesNode { .. }) && {
            let pn = node.as_parentheses_node().unwrap();
            pn.body().and_then(|b| b.as_statements_node()).map_or(false, |s| {
                let v: Vec<_> = s.body().iter().collect();
                v.len() == 1 && matches!(v[0], Node::RangeNode { .. })
            })
        }
}

fn is_range_receiver(node: &Node) -> bool {
    is_range_node(node)
}

/// Build the key_source for the replacement, handling array unpacking.
/// `op_for_unpacking`: when key is array literal we unpack (no `*`); else use `*var`.
pub fn build_key_source(es: &ExtractedSubset, source: &str) -> String {
    match es.op {
        SubsetOp::Eq | SubsetOp::Ne => es.key_source.clone(),
        SubsetOp::Include | SubsetOp::Exclude | SubsetOp::In => {
            if es.key_is_array {
                // Unpack array elements
                let arr_src = &source[es.key_node_start..es.key_node_end];
                unpack_array_source(arr_src)
            } else {
                // Splat the variable / method call
                format!("*{}", es.key_source)
            }
        }
    }
}

/// Convert array literal source (e.g. `%i[foo bar]`, `[:foo, :bar]`, `%w[a b]`, `%W[#{x} b]`)
/// into a comma-separated key list.
fn unpack_array_source(src: &str) -> String {
    let trimmed = src.trim();
    // Bracket arrays [:foo, :bar]
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return trimmed[1..trimmed.len()-1].trim().to_string();
    }
    // %i[foo bar] -> :foo, :bar
    if trimmed.starts_with("%i[") || trimmed.starts_with("%i(") {
        let body = &trimmed[3..trimmed.len()-1];
        return body.split_whitespace().map(|w| format!(":{}", w)).collect::<Vec<_>>().join(", ");
    }
    // %I[#{foo} bar] -> :"#{foo}", :bar
    if trimmed.starts_with("%I[") || trimmed.starts_with("%I(") {
        let body = &trimmed[3..trimmed.len()-1];
        return body.split_whitespace().map(|w| {
            if w.contains("#{") { format!(":\"{}\"", w) } else { format!(":{}", w) }
        }).collect::<Vec<_>>().join(", ");
    }
    // %w[foo bar] -> 'foo', 'bar'
    if trimmed.starts_with("%w[") || trimmed.starts_with("%w(") {
        let body = &trimmed[3..trimmed.len()-1];
        return body.split_whitespace().map(|w| format!("'{}'", w)).collect::<Vec<_>>().join(", ");
    }
    // %W[#{foo} bar] -> "#{foo}", 'bar'
    if trimmed.starts_with("%W[") || trimmed.starts_with("%W(") {
        let body = &trimmed[3..trimmed.len()-1];
        return body.split_whitespace().map(|w| {
            if w.contains("#{") { format!("\"{}\"", w) } else { format!("'{}'", w) }
        }).collect::<Vec<_>>().join(", ");
    }
    // Fallback - shouldn't happen
    trimmed.to_string()
}

/// Determine if the matched block is "except-like" (vs "slice-like") given the parent method name.
/// Mirrors RuboCop's `semantically_except_method?`.
pub fn is_except_like(parent_method: &str, es: &ExtractedSubset) -> bool {
    let body_negated = es.negated;
    match parent_method {
        "reject" | "delete_if" => match es.op {
            SubsetOp::Eq | SubsetOp::Include | SubsetOp::In => !body_negated,
            SubsetOp::Ne => body_negated,
            SubsetOp::Exclude => body_negated,
        },
        // select/filter/keep_if
        _ => match es.op {
            SubsetOp::Ne => !body_negated,
            SubsetOp::Eq | SubsetOp::Include | SubsetOp::In => body_negated,
            SubsetOp::Exclude => !body_negated,
        },
    }
}
