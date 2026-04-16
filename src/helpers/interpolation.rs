//! Shared helpers for Interpolation-mixin cops.
//!
//! Mirrors RuboCop's `RuboCop::Cop::Interpolation` mixin: walk the parts of
//! `InterpolatedStringNode`, `InterpolatedSymbolNode`, `InterpolatedXStringNode`,
//! and `InterpolatedRegularExpressionNode`, invoking a callback for each
//! `EmbeddedStatementsNode` (begin) and optionally each `EmbeddedVariableNode`
//! (variable shorthand).

use ruby_prism::Node;

/// Iterate over each `EmbeddedStatementsNode` that appears as a direct part
/// of any interpolation-bearing node (`InterpolatedStringNode` etc.).
/// Returns parts in source order.
pub fn embedded_statements_parts<'pr>(
    node: &Node<'pr>,
) -> Vec<ruby_prism::EmbeddedStatementsNode<'pr>> {
    let mut out = Vec::new();
    let parts = match parts_of(node) {
        Some(p) => p,
        None => return out,
    };
    for part in parts {
        if let Node::EmbeddedStatementsNode { .. } = &part {
            out.push(part.as_embedded_statements_node().unwrap());
        }
    }
    out
}

/// Iterate over each `EmbeddedVariableNode` part (variable shorthand like `"#@x"`).
pub fn embedded_variable_parts<'pr>(
    node: &Node<'pr>,
) -> Vec<ruby_prism::EmbeddedVariableNode<'pr>> {
    let mut out = Vec::new();
    let parts = match parts_of(node) {
        Some(p) => p,
        None => return out,
    };
    for part in parts {
        if let Node::EmbeddedVariableNode { .. } = &part {
            out.push(part.as_embedded_variable_node().unwrap());
        }
    }
    out
}

/// Return the parts iterator (collected as Vec) of any node type that carries
/// interpolation. `None` for non-interpolation nodes.
fn parts_of<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    match node {
        Node::InterpolatedStringNode { .. } => {
            Some(node.as_interpolated_string_node().unwrap().parts().iter().collect())
        }
        Node::InterpolatedSymbolNode { .. } => {
            Some(node.as_interpolated_symbol_node().unwrap().parts().iter().collect())
        }
        Node::InterpolatedXStringNode { .. } => {
            Some(node.as_interpolated_x_string_node().unwrap().parts().iter().collect())
        }
        Node::InterpolatedRegularExpressionNode { .. } => Some(
            node.as_interpolated_regular_expression_node()
                .unwrap()
                .parts()
                .iter()
                .collect(),
        ),
        _ => None,
    }
}

/// True if the given node is an array node opened with a `%w`/`%W`/`%i`/`%I` percent literal.
pub fn is_percent_literal_array(array: &ruby_prism::ArrayNode<'_>, source: &str) -> bool {
    let loc = array.location();
    let src = &source[loc.start_offset()..loc.end_offset()];
    src.starts_with("%w") || src.starts_with("%W") || src.starts_with("%i") || src.starts_with("%I")
}
