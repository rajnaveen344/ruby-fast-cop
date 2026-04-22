//! Lint/FloatComparison - Avoid equality comparisons of floats as they are unreliable.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_EQ: &str = "Avoid equality comparisons of floats as they are unreliable.";
const MSG_NEQ: &str = "Avoid inequality comparisons of floats as they are unreliable.";
const MSG_CASE: &str = "Avoid float literal comparisons in case statements as they are unreliable.";

/// Methods that return integer (not float) when called without ndigits argument
const RETURNS_INTEGER_METHODS: &[&str] = &["ceil", "floor", "round", "truncate"];

/// Kernel methods that return Float
const FLOAT_CONVERSION_METHODS: &[&str] = &["Float"];

/// Instance methods that return Float
const FLOAT_RETURNING_METHODS: &[&str] = &["to_f", "fdiv", "div"];

/// Arithmetic operators that may propagate float-ness
const ARITHMETIC_OPS: &[&str] = &["+", "-", "*", "/", "%", "**"];

#[derive(Default)]
pub struct FloatComparison;

impl FloatComparison {
    pub fn new() -> Self { Self }
}

impl Cop for FloatComparison {
    fn name(&self) -> &'static str { "Lint/FloatComparison" }
    fn severity(&self) -> Severity { Severity::Warning }

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

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        let method_str = method.as_ref();

        match method_str {
            "==" | "!=" => {
                self.check_equality_op(node, method_str);
            }
            "eql?" | "equal?" => {
                self.check_eql_method(node);
            }
            _ => {}
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode) {
        for cond in node.conditions().iter() {
            if let Some(float_node) = cond.as_float_node() {
                let val_str = String::from_utf8_lossy(float_node.location().as_slice());
                let val: f64 = val_str.parse().unwrap_or(f64::NAN);
                if val != 0.0 {
                    let loc = float_node.location();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Lint/FloatComparison",
                        MSG_CASE,
                        Severity::Warning,
                        loc.start_offset(),
                        loc.end_offset(),
                    ));
                }
            }
        }
        ruby_prism::visit_when_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn check_equality_op(&mut self, node: &ruby_prism::CallNode, op: &str) {
        // Must have exactly 1 argument
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return;
        }

        let lhs = match node.receiver() {
            Some(r) => r,
            None => return,
        };
        let rhs = &arg_list[0];

        // Neither side should be nil
        if matches!(lhs, Node::NilNode { .. }) || matches!(rhs, Node::NilNode { .. }) {
            return;
        }

        // If either side is a zero literal, no offense (comparing against zero is OK)
        if is_zero_literal(&lhs) || is_zero_literal(rhs) {
            return;
        }

        // One side must be float-like
        if is_float_like(&lhs) || is_float_like(rhs) {
            let loc = node.location();
            let msg = if op == "!=" { MSG_NEQ } else { MSG_EQ };
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/FloatComparison",
                msg,
                Severity::Warning,
                loc.start_offset(),
                loc.end_offset(),
            ));
        }
    }

    fn check_eql_method(&mut self, node: &ruby_prism::CallNode) {
        // Must have exactly 1 argument
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return;
        }

        let arg = &arg_list[0];

        // Check receiver: if receiver is float-like OR argument is float-like
        let recv_is_float = if let Some(r) = node.receiver() { is_float_like(&r) } else { false };
        let arg_is_float = is_float_like(arg);

        // Zero check
        let recv_is_zero = if let Some(r) = node.receiver() { is_zero_literal(&r) } else { false };
        let arg_is_zero = is_zero_literal(arg);
        if recv_is_zero || arg_is_zero {
            return;
        }

        if recv_is_float || arg_is_float {
            let loc = node.location();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/FloatComparison",
                MSG_EQ,
                Severity::Warning,
                loc.start_offset(),
                loc.end_offset(),
            ));
        }
    }
}

/// Returns true if the node is a zero literal (integer 0 or float 0.0).
fn is_zero_literal(node: &Node) -> bool {
    match node {
        Node::IntegerNode { .. } => {
            let n = node.as_integer_node().unwrap();
            let src = String::from_utf8_lossy(n.location().as_slice());
            src.trim() == "0"
        }
        Node::FloatNode { .. } => {
            let n = node.as_float_node().unwrap();
            let src = String::from_utf8_lossy(n.location().as_slice());
            let val: f64 = src.parse().unwrap_or(f64::NAN);
            val == 0.0
        }
        _ => false,
    }
}

/// Returns true if this node produces a float value (not zero, not rational).
fn is_float_like(node: &Node) -> bool {
    match node {
        Node::FloatNode { .. } => {
            // Zero floats are OK
            let n = node.as_float_node().unwrap();
            let src = String::from_utf8_lossy(n.location().as_slice());
            let val: f64 = src.parse().unwrap_or(f64::NAN);
            val != 0.0
        }
        Node::RationalNode { .. } => {
            // Rational literals (e.g., 0.2r) are OK
            false
        }
        Node::IntegerNode { .. } => false,
        Node::NilNode { .. } => false,
        Node::TrueNode { .. } | Node::FalseNode { .. } => false,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            is_float_returning_call(&call)
        }
        Node::ParenthesesNode { .. } => {
            // Unwrap parentheses and check the inner expression
            let paren = node.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Some(stmts) = body.as_statements_node() {
                    let items: Vec<_> = stmts.body().iter().collect();
                    if let Some(last) = items.last() {
                        return is_float_like(last);
                    }
                }
            }
            false
        }
        _ => false,
    }
}

fn is_float_returning_call(call: &ruby_prism::CallNode) -> bool {
    let method = String::from_utf8_lossy(call.name().as_slice());
    let method_str = method.as_ref();

    // Kernel/module level float conversion: Float(x)
    if FLOAT_CONVERSION_METHODS.contains(&method_str) && call.receiver().is_none() {
        return true;
    }

    // Instance methods that return float
    if FLOAT_RETURNING_METHODS.contains(&method_str) {
        if call.receiver().is_some() {
            return true;
        }
    }

    // Methods that return integer when called without ndigits, but float when called with ndigits
    // e.g., 1.1.ceil → Integer; 1.1.ceil(1) → Float
    if RETURNS_INTEGER_METHODS.contains(&method_str) {
        if let Some(recv) = call.receiver() {
            // Only relevant if receiver is float-like
            if is_float_like(&recv) {
                // Check if called with arguments (ndigits > 0 → returns float)
                let has_args = if let Some(args) = call.arguments() {
                    args.arguments().iter().count() > 0
                } else {
                    false
                };
                if has_args {
                    return true; // ceil(1) etc. → float
                }
                // No args → returns integer
                return false;
            }
        }
    }

    // Arithmetic operators: if any operand is float-like
    if ARITHMETIC_OPS.contains(&method_str) {
        if let Some(recv) = call.receiver() {
            if is_float_like(&recv) {
                return true;
            }
        }
        if let Some(args) = call.arguments() {
            for arg in args.arguments().iter() {
                if is_float_like(&arg) {
                    return true;
                }
            }
        }
    }

    // Method called on a float-like receiver (e.g., 0.1.abs)
    if let Some(recv) = call.receiver() {
        if is_float_like(&recv) {
            // Not a returns-integer method (already handled above)
            if !RETURNS_INTEGER_METHODS.contains(&method_str) {
                return true;
            }
        }
    }

    false
}

crate::register_cop!("Lint/FloatComparison", |_cfg| Some(Box::new(FloatComparison::new())));
