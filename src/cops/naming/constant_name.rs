//! Naming/ConstantName cop
//!
//! Checks that constants use SCREAMING_SNAKE_CASE.
//! SCREAMING_SNAKE_CASE = /^[[:digit:][:upper:]_]+$/  (POSIX upper = any uppercase Unicode)
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/naming/constant_name.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Use SCREAMING_SNAKE_CASE for constants.";

#[derive(Default)]
pub struct ConstantName;

impl ConstantName {
    pub fn new() -> Self {
        Self
    }

    /// SCREAMING_SNAKE_CASE: all chars must be uppercase letters (Unicode), digits, or underscores.
    fn is_screaming_snake(name: &str) -> bool {
        name.chars().all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_')
    }

    /// RuboCop's `allowed_assignment?`:
    /// Skip if value is: block (CallNode with block), const(ant), casgn (constant write),
    /// allowed method call (nil receiver or non-literal receiver), Class/Struct.new,
    /// or conditional where at least one branch is a constant.
    fn should_skip(value: &Node) -> bool {
        match value {
            // block: CallNode where the call has a block attached
            Node::CallNode { .. } => {
                let call = value.as_call_node().unwrap();
                // If the call has a block → skip (block type)
                if call.block().is_some() {
                    return true;
                }
                // allowed_method_call_on_rhs?: receiver is nil OR receiver is not a literal
                // NOT allowed if receiver IS a literal (number, string, range, etc.)
                match call.receiver() {
                    None => true, // no receiver → bare method call → skip
                    Some(recv) => !Self::is_literal_receiver(&recv),
                }
            }
            // const type
            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => true,
            // casgn type (chained const write: Bar = Foo = 4 → RHS is another ConstantWriteNode)
            Node::ConstantWriteNode { .. } => true,
            // conditional: if at least one branch is a constant → skip
            Node::IfNode { .. } => Self::any_branch_is_const(value),
            Node::UnlessNode { .. } => true, // conservative
            _ => false,
        }
    }

    /// RuboCop's `literal_receiver?`: the receiver is a literal (integer, float, string, range, etc.)
    fn is_literal_receiver(node: &Node) -> bool {
        matches!(
            node,
            Node::IntegerNode { .. }
                | Node::FloatNode { .. }
                | Node::StringNode { .. }
                | Node::InterpolatedStringNode { .. }
                | Node::SymbolNode { .. }
                | Node::ArrayNode { .. }
                | Node::HashNode { .. }
                | Node::NilNode { .. }
                | Node::TrueNode { .. }
                | Node::FalseNode { .. }
                | Node::RangeNode { .. }
        ) || {
            // (begin literal?) — parenthesized literal
            if let Some(p) = node.as_parentheses_node() {
                if let Some(body) = p.body() {
                    if let Some(stmts) = body.as_statements_node() {
                        let items: Vec<_> = stmts.body().iter().collect();
                        if items.len() == 1 {
                            return Self::is_literal_receiver(&items[0]);
                        }
                    }
                }
            }
            false
        }
    }

    /// Returns true if any branch of a conditional contains a constant.
    fn any_branch_is_const(node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                let then_has_const = if_node
                    .statements()
                    .map(|s| Self::stmts_has_const(&s))
                    .unwrap_or(false);
                let else_has_const = if_node
                    .subsequent()
                    .map(|s| Self::any_branch_is_const(&s))
                    .unwrap_or(false);
                then_has_const || else_has_const
            }
            Node::ElseNode { .. } => {
                let else_node = node.as_else_node().unwrap();
                else_node
                    .statements()
                    .map(|s| Self::stmts_has_const(&s))
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    fn stmts_has_const(stmts: &ruby_prism::StatementsNode) -> bool {
        stmts.body().iter().any(|n| {
            matches!(n, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })
        })
    }
}

struct ConstantNameVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> ConstantNameVisitor<'a> {
    fn check_name(&mut self, name: &str, start: usize, end: usize) {
        if !ConstantName::is_screaming_snake(name) {
            self.offenses.push(self.ctx.offense_with_range(
                "Naming/ConstantName",
                MSG,
                Severity::Convention,
                start,
                end,
            ));
        }
    }
}

impl Visit<'_> for ConstantNameVisitor<'_> {
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let value = node.value();
        if !ConstantName::should_skip(&value) {
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            let name_loc = node.name_loc();
            self.check_name(&name, name_loc.start_offset(), name_loc.end_offset());
        }
        // Recurse to catch nested constant writes (chained: Bar = Foo = 4)
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        // ||= assignment: check same rules as regular assignment
        let value = node.value();
        if !ConstantName::should_skip(&value) {
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            let name_loc = node.name_loc();
            self.check_name(&name, name_loc.start_offset(), name_loc.end_offset());
        }
        ruby_prism::visit_constant_or_write_node(self, node);
    }

    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode) {
        let value = node.value();
        if !ConstantName::should_skip(&value) {
            let target = node.target();
            if let Some(name_node) = target.name() {
                let name = String::from_utf8_lossy(name_node.as_slice()).to_string();
                let name_loc = target.name_loc();
                self.check_name(&name, name_loc.start_offset(), name_loc.end_offset());
            }
        }
        ruby_prism::visit_constant_path_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        // In multi-assign, check each target that is a constant
        for target in node.lefts().iter() {
            if let Some(ct) = target.as_constant_target_node() {
                let name = String::from_utf8_lossy(ct.name().as_slice()).to_string();
                let loc = ct.location();
                self.check_name(&name, loc.start_offset(), loc.end_offset());
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }
}

impl Cop for ConstantName {
    fn name(&self) -> &'static str {
        "Naming/ConstantName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ConstantNameVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Naming/ConstantName", |_cfg| {
    Some(Box::new(ConstantName::new()))
});
