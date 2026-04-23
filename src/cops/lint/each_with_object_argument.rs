//! Lint/EachWithObjectArgument cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/each_with_object_argument.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct EachWithObjectArgument;

impl EachWithObjectArgument {
    pub fn new() -> Self { Self }
}

/// Mirror RuboCop's `immutable_literal?` — integer, float, symbol, true, false, nil
fn is_immutable_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. }
        | Node::SymbolNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::NilNode { .. }
    )
}

impl Cop for EachWithObjectArgument {
    fn name(&self) -> &'static str { "Lint/EachWithObjectArgument" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "each_with_object" {
            return vec![];
        }
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }
        if !is_immutable_literal(&arg_list[0]) {
            return vec![];
        }
        // Offense spans from start of receiver (or method) to closing paren / end of args
        let start = node.location().start_offset();
        // end = closing_loc if present, else last arg end
        let end = if let Some(cl) = node.closing_loc() {
            cl.end_offset()
        } else {
            arg_list.last().map(|a| a.location().end_offset()).unwrap_or(node.location().end_offset())
        };
        vec![ctx.offense_with_range(
            "Lint/EachWithObjectArgument",
            "The argument to each_with_object cannot be immutable.",
            Severity::Warning,
            start,
            end,
        )]
    }
}

crate::register_cop!("Lint/EachWithObjectArgument", |_cfg| {
    Some(Box::new(EachWithObjectArgument::new()))
});
