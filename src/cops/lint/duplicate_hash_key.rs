//! Lint/DuplicateHashKey cop.
//!
//! Ported from https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_hash_key.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct DuplicateHashKey;

impl DuplicateHashKey {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateHashKey {
    fn name(&self) -> &'static str { "Lint/DuplicateHashKey" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        check_elements_for_duplicates(node.elements().iter().collect::<Vec<_>>(), ctx)
    }

    fn check_keyword_hash(&self, node: &ruby_prism::KeywordHashNode, ctx: &CheckContext) -> Vec<Offense> {
        check_elements_for_duplicates(node.elements().iter().collect::<Vec<_>>(), ctx)
    }
}

fn check_elements_for_duplicates(elements: Vec<Node>, ctx: &CheckContext) -> Vec<Offense> {
    // For each assoc: derive (comparison_text, offense_start, offense_end).
    // Offense range excludes the trailing `:` for shorthand symbol keys, matching RuboCop.
    let mut keys: Vec<(String, usize, usize)> = Vec::new();
    for el in elements {
        if let Node::AssocNode { .. } = &el {
            let assoc = el.as_assoc_node().unwrap();
            let key = assoc.key();
            if !is_recursive_basic_literal(&key) && !is_constant_ref(&key) {
                continue;
            }
            let is_shorthand_sym = assoc.operator_loc().is_none();
            let (off_start, off_end) = if is_shorthand_sym {
                if let Some(sym) = key.as_symbol_node() {
                    let vloc = sym.value_loc().unwrap_or_else(|| sym.location());
                    (vloc.start_offset(), vloc.end_offset())
                } else {
                    let l = key.location();
                    (l.start_offset(), l.end_offset())
                }
            } else {
                let l = key.location();
                (l.start_offset(), l.end_offset())
            };
            // Comparison text: full key source (consistent across pairs with same style).
            let kl = key.location();
            let cmp = ctx.source[kl.start_offset()..kl.end_offset()].to_string();
            keys.push((cmp, off_start, off_end));
        }
    }

    let mut seen: Vec<String> = Vec::new();
    let mut offenses = Vec::new();
    for (src, off_start, off_end) in &keys {
        if seen.iter().any(|s| s == src) {
            offenses.push(ctx.offense_with_range(
                "Lint/DuplicateHashKey",
                "Duplicated key in hash literal.",
                Severity::Warning,
                *off_start,
                *off_end,
            ));
        } else {
            seen.push(src.clone());
        }
    }
    offenses
}

/// Mirror RuboCop's `recursive_basic_literal?` — treats nodes as literal when
/// they recursively evaluate to a constant expression (literals, arrays/hashes
/// of literals, operator calls on literals, etc.).
fn is_recursive_basic_literal(node: &Node) -> bool {
    match node {
        Node::NilNode { .. } | Node::TrueNode { .. } | Node::FalseNode { .. }
        | Node::IntegerNode { .. } | Node::FloatNode { .. }
        | Node::RationalNode { .. } | Node::ImaginaryNode { .. }
        | Node::SymbolNode { .. } | Node::SourceFileNode { .. }
        | Node::SourceLineNode { .. } | Node::SourceEncodingNode { .. } => true,

        Node::StringNode { .. } => true,
        Node::RegularExpressionNode { .. } => true,

        Node::InterpolatedStringNode { .. } => node.as_interpolated_string_node().unwrap()
            .parts().iter().all(|p| is_recursive_basic_literal(&p)),
        Node::InterpolatedSymbolNode { .. } => node.as_interpolated_symbol_node().unwrap()
            .parts().iter().all(|p| is_recursive_basic_literal(&p)),
        Node::InterpolatedRegularExpressionNode { .. } => node.as_interpolated_regular_expression_node().unwrap()
            .parts().iter().all(|p| is_recursive_basic_literal(&p)),
        Node::EmbeddedStatementsNode { .. } => {
            let esn = node.as_embedded_statements_node().unwrap();
            match esn.statements() {
                Some(stmts) => stmts.body().iter().all(|s| is_recursive_basic_literal(&s)),
                None => true,
            }
        }
        Node::StatementsNode { .. } => node.as_statements_node().unwrap()
            .body().iter().all(|s| is_recursive_basic_literal(&s)),

        Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter()
            .all(|e| is_recursive_basic_literal(&e)),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().all(|e| {
            if let Some(a) = e.as_assoc_node() {
                is_recursive_basic_literal(&a.key()) && is_recursive_basic_literal(&a.value())
            } else { false }
        }),
        Node::RangeNode { .. } => {
            let r = node.as_range_node().unwrap();
            r.left().as_ref().map_or(true, is_recursive_basic_literal)
                && r.right().as_ref().map_or(true, is_recursive_basic_literal)
        }

        Node::AndNode { .. } => {
            let a = node.as_and_node().unwrap();
            is_recursive_basic_literal(&a.left()) && is_recursive_basic_literal(&a.right())
        }
        Node::OrNode { .. } => {
            let o = node.as_or_node().unwrap();
            is_recursive_basic_literal(&o.left()) && is_recursive_basic_literal(&o.right())
        }

        Node::CallNode { .. } => {
            // Method call is literal iff receiver literal AND all args literal.
            let call = node.as_call_node().unwrap();
            let recv_ok = match call.receiver() {
                Some(r) => is_recursive_basic_literal(&r),
                None => false,
            };
            if !recv_ok { return false; }
            match call.arguments() {
                Some(args) => args.arguments().iter().all(|a| is_recursive_basic_literal(&a)),
                None => true,
            }
        }

        Node::ParenthesesNode { .. } => {
            let p = node.as_parentheses_node().unwrap();
            match p.body() {
                Some(b) => is_recursive_basic_literal(&b),
                None => true,
            }
        }

        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => true,

        _ => false,
    }
}

fn is_constant_ref(node: &Node) -> bool {
    matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })
}
