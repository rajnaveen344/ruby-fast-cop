//! Style/AndOr - Checks for uses of `and` and `or`, and suggests using `&&` and `||` instead.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/and_or.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

/// EnforcedStyle for Style/AndOr
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    /// Only flag `and`/`or` inside conditionals (if/while/until conditions)
    Conditionals,
    /// Flag all uses of `and`/`or`
    Always,
}

pub struct AndOr {
    style: EnforcedStyle,
}

impl AndOr {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Default for AndOr {
    fn default() -> Self {
        Self::new(EnforcedStyle::Conditionals)
    }
}

const COP_NAME: &str = "Style/AndOr";

impl Cop for AndOr {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = AndOrVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct AndOrVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
}

impl<'a> AndOrVisitor<'a> {
    /// Process a single and/or node, adding an offense if it uses keyword form
    fn process_logical_operator(&mut self, node: &Node) {
        let (operator_loc, op_text, prefer) = match node {
            Node::AndNode { .. } => {
                let n = node.as_and_node().unwrap();
                let ol = n.operator_loc();
                let op = self.ctx.src(ol.start_offset(), ol.end_offset());
                if op != "and" { return; }
                (ol, "and", "&&")
            }
            Node::OrNode { .. } => {
                let n = node.as_or_node().unwrap();
                let ol = n.operator_loc();
                let op = self.ctx.src(ol.start_offset(), ol.end_offset());
                if op != "or" { return; }
                (ol, "or", "||")
            }
            _ => return,
        };

        let message = format!("Use `{}` instead of `{}`.", prefer, op_text);
        let mut offense = self.ctx.offense(COP_NAME, &message, Severity::Convention, &operator_loc);

        // Build correction edits
        let mut edits = Vec::new();

        // Replace operator
        edits.push(Edit {
            start_offset: operator_loc.start_offset(),
            end_offset: operator_loc.end_offset(),
            replacement: prefer.to_string(),
        });

        // Correct child nodes (add parens where needed for precedence)
        let (left, right) = match node {
            Node::AndNode { .. } => {
                let n = node.as_and_node().unwrap();
                (n.left(), n.right())
            }
            Node::OrNode { .. } => {
                let n = node.as_or_node().unwrap();
                (n.left(), n.right())
            }
            _ => unreachable!(),
        };

        self.correct_child(&left, &mut edits);
        self.correct_child(&right, &mut edits);

        // Keep operator precedence (mirrors RuboCop's keep_operator_precedence)
        match node {
            Node::OrNode { .. } => {
                // If this `or` is inside an `and` parent, we can't know parent here.
                // But RuboCop wraps `or` when parent is `and`. We handle this from the
                // `and` side: if `and`'s lhs is a keyword `or`, wrap it.
            }
            Node::AndNode { .. } => {
                // If lhs is keyword `or` (will become ||), wrap it for precedence
                if let Node::OrNode { .. } = left {
                    let or_node = left.as_or_node().unwrap();
                    let or_op = self.ctx.src(
                        or_node.operator_loc().start_offset(),
                        or_node.operator_loc().end_offset(),
                    );
                    if or_op == "or" {
                        wrap_node(&left, &mut edits);
                    }
                }
                // If rhs is `||`, wrap it for precedence
                if let Node::OrNode { .. } = right {
                    let or_node = right.as_or_node().unwrap();
                    let or_op = self.ctx.src(
                        or_node.operator_loc().start_offset(),
                        or_node.operator_loc().end_offset(),
                    );
                    if or_op == "||" {
                        wrap_node(&right, &mut edits);
                    }
                }
            }
            _ => {}
        }

        if !edits.is_empty() {
            offense = offense.with_correction(Correction { edits });
        }

        self.offenses.push(offense);
    }

    /// Correct a child node by adding parens where needed
    fn correct_child(&self, node: &Node, edits: &mut Vec<Edit>) {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                self.correct_call(&call, edits);
            }
            Node::ReturnNode { .. } => {
                self.correct_other(node, edits);
            }
            // Assignment nodes
            Node::LocalVariableWriteNode { .. }
            | Node::InstanceVariableWriteNode { .. }
            | Node::ClassVariableWriteNode { .. }
            | Node::GlobalVariableWriteNode { .. }
            | Node::ConstantWriteNode { .. }
            | Node::MultiWriteNode { .. }
            | Node::LocalVariableOperatorWriteNode { .. }
            | Node::InstanceVariableOperatorWriteNode { .. }
            | Node::ClassVariableOperatorWriteNode { .. }
            | Node::GlobalVariableOperatorWriteNode { .. }
            | Node::ConstantOperatorWriteNode { .. }
            | Node::LocalVariableOrWriteNode { .. }
            | Node::InstanceVariableOrWriteNode { .. }
            | Node::ClassVariableOrWriteNode { .. }
            | Node::GlobalVariableOrWriteNode { .. }
            | Node::ConstantOrWriteNode { .. }
            | Node::LocalVariableAndWriteNode { .. }
            | Node::InstanceVariableAndWriteNode { .. }
            | Node::ClassVariableAndWriteNode { .. }
            | Node::GlobalVariableAndWriteNode { .. }
            | Node::ConstantAndWriteNode { .. }
            | Node::CallOperatorWriteNode { .. }
            | Node::CallOrWriteNode { .. }
            | Node::CallAndWriteNode { .. }
            | Node::IndexOperatorWriteNode { .. }
            | Node::IndexOrWriteNode { .. }
            | Node::IndexAndWriteNode { .. } => {
                self.correct_other(node, edits);
            }
            _ => {}
        }
    }

    /// Correct a call node - handle !, setter, comparison, and regular calls
    fn correct_call(&self, call: &ruby_prism::CallNode, edits: &mut Vec<Edit>) {
        let method_name = String::from_utf8_lossy(call.name().as_slice());

        // Handle `!` (bang/not) operator
        if method_name == "!" {
            if let Some(recv) = call.receiver() {
                // prefix_bang: `!expr`
                if call.message_loc().map_or(false, |ml| {
                    self.ctx.src(ml.start_offset(), ml.end_offset()) == "!"
                }) {
                    if let Node::CallNode { .. } = recv {
                        self.correct_call(&recv.as_call_node().unwrap(), edits);
                    }
                    return;
                }
                // prefix_not: `not expr`
                if call.message_loc().map_or(false, |ml| {
                    self.ctx.src(ml.start_offset(), ml.end_offset()) == "not"
                }) {
                    self.correct_other(&call.as_node(), edits);
                    return;
                }
            }
            return;
        }

        // Handle setter methods like `obj.method= arg`
        if method_name.ends_with('=') && !is_comparison_method(&method_name) {
            self.correct_setter(call, edits);
            return;
        }

        // Handle comparison methods: wrap in parens
        if is_comparison_method(&method_name) {
            self.correct_other(&call.as_node(), edits);
            return;
        }

        // Regular method call: add parens around args if bare call with arguments
        if !self.is_correctable_send(call) {
            return;
        }

        if let Some(msg_loc) = call.message_loc() {
            let begin_paren = msg_loc.end_offset();
            // For predicate methods like `is_a?String`, don't skip space
            let end_paren = if method_name.ends_with('?') {
                let next = self.ctx.source.as_bytes().get(begin_paren);
                if next.map_or(false, |&b| b != b' ' && b != b'\t' && b != b'\n') {
                    begin_paren
                } else {
                    begin_paren + 1
                }
            } else {
                begin_paren + 1
            };

            // Replace whitespace with `(`
            edits.push(Edit {
                start_offset: begin_paren,
                end_offset: end_paren,
                replacement: "(".to_string(),
            });

            // Insert `)` after last argument
            if let Some(args) = call.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let Some(last_arg) = arg_list.last() {
                    let end = last_arg.location().end_offset();
                    edits.push(Edit {
                        start_offset: end,
                        end_offset: end,
                        replacement: ")".to_string(),
                    });
                }
            }
        }
    }

    /// Correct a setter call: wrap receiver through last arg in parens
    fn correct_setter(&self, call: &ruby_prism::CallNode, edits: &mut Vec<Edit>) {
        if let Some(recv) = call.receiver() {
            let start = recv.location().start_offset();
            edits.push(Edit {
                start_offset: start,
                end_offset: start,
                replacement: "(".to_string(),
            });
            if let Some(args) = call.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let Some(last_arg) = arg_list.last() {
                    let end = last_arg.location().end_offset();
                    edits.push(Edit {
                        start_offset: end,
                        end_offset: end,
                        replacement: ")".to_string(),
                    });
                }
            }
        }
    }

    /// Correct non-send nodes by wrapping in parens
    fn correct_other(&self, node: &Node, edits: &mut Vec<Edit>) {
        // Skip if already parenthesized call
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if call.opening_loc().is_some() {
                return;
            }
        }
        wrap_node(node, edits);
    }

    /// Check if a call node is correctable (bare args, not subscript)
    fn is_correctable_send(&self, call: &ruby_prism::CallNode) -> bool {
        let method_name = String::from_utf8_lossy(call.name().as_slice());
        call.opening_loc().is_none()
            && call.arguments().map_or(false, |a| a.arguments().iter().count() > 0)
            && method_name != "[]"
    }

    /// Walk into a condition and find all keyword and/or nodes
    fn check_condition(&mut self, condition: &Node) {
        self.visit_condition(condition);
    }

    /// Recursively visit a condition to find and/or keyword nodes
    fn visit_condition(&mut self, node: &Node) {
        match node {
            Node::AndNode { .. } => {
                let n = node.as_and_node().unwrap();
                let op = self.ctx.src(n.operator_loc().start_offset(), n.operator_loc().end_offset());
                if op == "and" || op == "or" {
                    self.process_logical_operator(node);
                }
                self.visit_condition(&n.left());
                self.visit_condition(&n.right());
            }
            Node::OrNode { .. } => {
                let n = node.as_or_node().unwrap();
                let op = self.ctx.src(n.operator_loc().start_offset(), n.operator_loc().end_offset());
                if op == "or" || op == "and" {
                    self.process_logical_operator(node);
                }
                self.visit_condition(&n.left());
                self.visit_condition(&n.right());
            }
            // For other nodes, and/or can't be nested further (in condition context)
            _ => {}
        }
    }
}

impl Visit<'_> for AndOrVisitor<'_> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        if self.style == EnforcedStyle::Always {
            self.process_logical_operator(&node.as_node());
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        if self.style == EnforcedStyle::Always {
            self.process_logical_operator(&node.as_node());
        }
        ruby_prism::visit_or_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if self.style == EnforcedStyle::Conditionals {
            self.check_condition(&node.predicate());
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if self.style == EnforcedStyle::Conditionals {
            self.check_condition(&node.predicate());
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if self.style == EnforcedStyle::Conditionals {
            self.check_condition(&node.predicate());
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if self.style == EnforcedStyle::Conditionals {
            self.check_condition(&node.predicate());
        }
        ruby_prism::visit_until_node(self, node);
    }
}

/// Wrap a node in parens
fn wrap_node(node: &Node, edits: &mut Vec<Edit>) {
    let start = node.location().start_offset();
    let end = node.location().end_offset();
    edits.push(Edit {
        start_offset: start,
        end_offset: start,
        replacement: "(".to_string(),
    });
    edits.push(Edit {
        start_offset: end,
        end_offset: end,
        replacement: ")".to_string(),
    });
}

fn is_comparison_method(name: &str) -> bool {
    matches!(name, "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>" | "===" | "=~" | "!~")
}
