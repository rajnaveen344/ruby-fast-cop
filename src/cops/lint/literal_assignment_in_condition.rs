//! Lint/LiteralAssignmentInCondition cop
//!
//! Translates RuboCop's LiteralAssignmentInCondition. Flags `if/while/until`
//! conditions containing an assignment whose RHS is a literal value.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct LiteralAssignmentInCondition;

impl LiteralAssignmentInCondition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for LiteralAssignmentInCondition {
    fn name(&self) -> &'static str {
        "Lint/LiteralAssignmentInCondition"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_condition(&mut self, condition: &Node) {
        // Walk node + descendants, but DO NOT cross into block bodies (matches RuboCop).
        if let Some((op_start, rhs)) = assignment_with_operator(condition) {
            let rhs_loc = rhs.location();
            if all_literals(&rhs) && !parallel_assignment_with_splat(&rhs) {
                let rhs_text = &self.ctx.source[rhs_loc.start_offset()..rhs_loc.end_offset()];
                let msg = format!(
                    "Don't use literal assignment `= {}` in conditional, should be `==` or non-literal operand.",
                    rhs_text
                );
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/LiteralAssignmentInCondition",
                    &msg,
                    Severity::Warning,
                    op_start,
                    rhs_loc.end_offset(),
                ));
            }
        }

        // Recurse into children — but skip BlockNode bodies.
        for child in iter_children(condition) {
            if matches!(child, Node::BlockNode { .. }) {
                continue;
            }
            self.check_condition(&child);
        }
    }
}

/// If `node` is an equals-assignment, return (operator_start_offset, rhs_node).
/// `loc.operator` exists only for `=` style assignments — not `||=` / `&&=` / op-asgn.
fn assignment_with_operator<'a>(node: &Node<'a>) -> Option<(usize, Node<'a>)> {
    match node {
        Node::LocalVariableWriteNode { .. } => {
            let n = node.as_local_variable_write_node().unwrap();
            Some((n.operator_loc().start_offset(), n.value()))
        }
        Node::InstanceVariableWriteNode { .. } => {
            let n = node.as_instance_variable_write_node().unwrap();
            Some((n.operator_loc().start_offset(), n.value()))
        }
        Node::ClassVariableWriteNode { .. } => {
            let n = node.as_class_variable_write_node().unwrap();
            Some((n.operator_loc().start_offset(), n.value()))
        }
        Node::GlobalVariableWriteNode { .. } => {
            let n = node.as_global_variable_write_node().unwrap();
            Some((n.operator_loc().start_offset(), n.value()))
        }
        Node::ConstantWriteNode { .. } => {
            let n = node.as_constant_write_node().unwrap();
            Some((n.operator_loc().start_offset(), n.value()))
        }
        // ConstantPathWriteNode also has operator_loc + value
        Node::ConstantPathWriteNode { .. } => {
            let n = node.as_constant_path_write_node().unwrap();
            Some((n.operator_loc().start_offset(), n.value()))
        }
        // Multi-assignment is not flagged here; RuboCop checks `loc.operator` on each
        _ => None,
    }
}

fn all_literals(node: &Node) -> bool {
    match node {
        // dstr / xstr → not a literal
        Node::InterpolatedStringNode { .. }
        | Node::InterpolatedSymbolNode { .. }
        | Node::InterpolatedXStringNode { .. }
        | Node::XStringNode { .. } => false,
        Node::ArrayNode { .. } => {
            let arr = node.as_array_node().unwrap();
            arr.elements().iter().all(|el| all_literals(&el))
        }
        Node::HashNode { .. } => {
            let h = node.as_hash_node().unwrap();
            h.elements().iter().all(|el| match el {
                Node::AssocNode { .. } => {
                    let a = el.as_assoc_node().unwrap();
                    all_literals(&a.key()) && all_literals(&a.value())
                }
                // AssocSplatNode etc — not literal
                _ => false,
            })
        }
        _ => is_literal(node),
    }
}

fn is_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::StringNode { .. }
            | Node::SymbolNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::SourceFileNode { .. }
            | Node::SourceLineNode { .. }
            | Node::SourceEncodingNode { .. }
            | Node::RangeNode { .. }
    )
}

fn parallel_assignment_with_splat(node: &Node) -> bool {
    if let Node::ArrayNode { .. } = node {
        let arr = node.as_array_node().unwrap();
        if let Some(first) = arr.elements().iter().next() {
            return matches!(first, Node::SplatNode { .. });
        }
    }
    false
}

/// Iterate immediate child nodes (used for traversal). Falls back to nothing for leaves.
fn iter_children<'a>(node: &Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    match node {
        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            out.push(n.left());
            out.push(n.right());
        }
        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            out.push(n.left());
            out.push(n.right());
        }
        Node::ParenthesesNode { .. } => {
            let n = node.as_parentheses_node().unwrap();
            if let Some(b) = n.body() {
                out.push(b);
            }
        }
        Node::StatementsNode { .. } => {
            let n = node.as_statements_node().unwrap();
            for s in n.body().iter() {
                out.push(s);
            }
        }
        Node::CallNode { .. } => {
            let n = node.as_call_node().unwrap();
            if let Some(r) = n.receiver() {
                out.push(r);
            }
            if let Some(args) = n.arguments() {
                for a in args.arguments().iter() {
                    out.push(a);
                }
            }
            if let Some(blk) = n.block() {
                out.push(blk);
            }
        }
        // Equality assignment RHS (used to recurse into nested cond): skip — it's
        // checked via assignment_with_operator above; we still recurse RHS for nested
        // assignments? RuboCop's traverse_node yields and continues into children.
        Node::LocalVariableWriteNode { .. } => {
            let n = node.as_local_variable_write_node().unwrap();
            out.push(n.value());
        }
        Node::InstanceVariableWriteNode { .. } => {
            let n = node.as_instance_variable_write_node().unwrap();
            out.push(n.value());
        }
        Node::ClassVariableWriteNode { .. } => {
            let n = node.as_class_variable_write_node().unwrap();
            out.push(n.value());
        }
        Node::GlobalVariableWriteNode { .. } => {
            let n = node.as_global_variable_write_node().unwrap();
            out.push(n.value());
        }
        Node::ConstantWriteNode { .. } => {
            let n = node.as_constant_write_node().unwrap();
            out.push(n.value());
        }
        _ => {}
    }
    out
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_condition(&node.predicate());
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_condition(&node.predicate());
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        self.check_condition(&node.predicate());
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.check_condition(&node.predicate());
        ruby_prism::visit_until_node(self, node);
    }
}

crate::register_cop!("Lint/LiteralAssignmentInCondition", |_cfg| {
    Some(Box::new(LiteralAssignmentInCondition::new()))
});
