//! Style/NegativeArrayIndex - Identifies usages of `arr[arr.length - n]` and suggests `arr[-n]`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/negative_array_index.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

/// Preserving methods that don't change the identity of the array for matching purposes.
const PRESERVING_METHODS: &[&str] = &["sort", "reverse", "shuffle", "rotate"];

/// Methods that return a length-like value.
const LENGTH_METHODS: &[&str] = &["length", "size", "count"];

pub struct NegativeArrayIndex;

impl NegativeArrayIndex {
    pub fn new() -> Self {
        Self
    }

    /// Get the source text for a node by its location.
    fn src<'a>(source: &'a str, node: &Node) -> &'a str {
        let loc = node.location();
        &source[loc.start_offset()..loc.end_offset()]
    }

    /// Check if a node is a "preserving method" chain.
    /// A preserving chain consists of:
    /// - A base: local/instance/class/global var, constant, self, or a bare method call (no receiver, no args)
    /// - Optionally chained with preserving methods: sort, reverse, shuffle, rotate
    fn is_preserving_chain(node: &Node) -> bool {
        match node {
            Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::ConstantReadNode { .. }
            | Node::SelfNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let receiver = match call.receiver() {
                    Some(r) => r,
                    None => {
                        // A bare method call with no receiver (e.g., `arr` parsed as a method call)
                        // is treated as a base variable. Must have no args and no block.
                        if let Some(args) = call.arguments() {
                            if args.arguments().iter().count() > 0 {
                                return false;
                            }
                        }
                        if call.block().is_some() {
                            return false;
                        }
                        return true;
                    }
                };
                let method = String::from_utf8_lossy(call.name().as_slice());
                if !PRESERVING_METHODS.contains(&method.as_ref()) {
                    return false;
                }
                if let Some(args) = call.arguments() {
                    if args.arguments().iter().count() > 0 {
                        return false;
                    }
                }
                if call.block().is_some() {
                    return false;
                }
                Self::is_preserving_chain(&receiver)
            }
            _ => false,
        }
    }

    /// Check if two receivers match for the purposes of this cop.
    /// This implements RuboCop's `receivers_match?` logic:
    /// 1. If length_receiver is None (implicit receiver), array_receiver must be `self`
    /// 2. Both must be preserving chains
    /// 3. If their sources match exactly, it's a match
    /// 4. Otherwise, only match if the array_receiver has at least one chained preserving method
    ///    (i.e., it's not a bare variable) - this ensures that e.g., `arr.sort[arr.length - 2]`
    ///    matches but `arr[arr.sort.length - 2]` does not
    fn receivers_match(source: &str, array_receiver: &Node, length_receiver: Option<&Node>) -> bool {
        match length_receiver {
            None => {
                matches!(array_receiver, Node::SelfNode { .. })
            }
            Some(len_recv) => {
                if !Self::is_preserving_chain(array_receiver) || !Self::is_preserving_chain(len_recv) {
                    return false;
                }
                let arr_src = Self::src(source, array_receiver);
                let len_src = Self::src(source, len_recv);
                if arr_src == len_src {
                    return true;
                }
                // Only match when the array receiver has a chained preserving method.
                // extract_base_receiver returns nil (None) for bare receivers without a chained method.
                Self::has_chained_receiver(array_receiver)
            }
        }
    }

    /// Check if a node is a "preserving expression" for range start validation.
    /// This mirrors RuboCop's `preserving_method?` which returns true if:
    /// - The node has no receiver (literals, variables, bare method calls)
    /// - The node is a call with a preserving method name and preserving receiver
    fn is_preserving_expression(node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                match call.receiver() {
                    None => true, // bare method call / variable
                    Some(receiver) => {
                        let method = String::from_utf8_lossy(call.name().as_slice());
                        if !PRESERVING_METHODS.contains(&method.as_ref()) {
                            return false;
                        }
                        Self::is_preserving_expression(&receiver)
                    }
                }
            }
            // Any non-call node is trivially preserving (integer, variable, etc.)
            _ => true,
        }
    }

    /// Check if a node has a chained receiver (i.e., is a call with a receiver).
    /// This corresponds to RuboCop's `extract_base_receiver` returning non-nil.
    fn has_chained_receiver(node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                call.receiver().is_some()
            }
            _ => false,
        }
    }

    /// Strict receiver match for range patterns.
    fn receivers_match_strict(source: &str, array_receiver: &Node, length_receiver: &Node) -> bool {
        if !Self::is_preserving_chain(array_receiver) {
            return false;
        }
        Self::src(source, array_receiver) == Self::src(source, length_receiver)
    }

    /// Parse a positive integer value from the source text of an IntegerNode.
    fn parse_int_from_source(source: &str, node: &Node) -> Option<i64> {
        match node {
            Node::IntegerNode { .. } => {
                let src = Self::src(source, node);
                let clean: String = src.chars().filter(|c| *c != '_').collect();
                clean.parse::<i64>().ok()
            }
            _ => None,
        }
    }

    /// Check if a node matches `receiver.length - N` and the receiver matches the array receiver.
    /// Returns the positive integer N if matched.
    fn check_length_subtraction_match(
        source: &str,
        node: &Node,
        array_receiver: &Node,
    ) -> Option<i64> {
        let call = match node {
            Node::CallNode { .. } => node.as_call_node().unwrap(),
            _ => return None,
        };

        let method = String::from_utf8_lossy(call.name().as_slice());
        if method.as_ref() != "-" {
            return None;
        }

        let recv = call.receiver()?;
        let length_call = match &recv {
            Node::CallNode { .. } => recv.as_call_node().unwrap(),
            _ => return None,
        };

        let length_method = String::from_utf8_lossy(length_call.name().as_slice());
        if !LENGTH_METHODS.contains(&length_method.as_ref()) {
            return None;
        }

        if let Some(args) = length_call.arguments() {
            if args.arguments().iter().count() > 0 {
                return None;
            }
        }
        if length_call.block().is_some() {
            return None;
        }

        let args = call.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return None;
        }

        let value = Self::parse_int_from_source(source, &arg_list[0])?;
        if value <= 0 {
            return None;
        }

        let length_receiver = length_call.receiver();
        if !Self::receivers_match(source, array_receiver, length_receiver.as_ref()) {
            return None;
        }

        Some(value)
    }

    /// Check if a node matches `receiver.length - N` with strict receiver match.
    fn check_length_subtraction_strict(
        source: &str,
        node: &Node,
        array_receiver: &Node,
    ) -> Option<i64> {
        let call = match node {
            Node::CallNode { .. } => node.as_call_node().unwrap(),
            _ => return None,
        };

        let method = String::from_utf8_lossy(call.name().as_slice());
        if method.as_ref() != "-" {
            return None;
        }

        let recv = call.receiver()?;
        let length_call = match &recv {
            Node::CallNode { .. } => recv.as_call_node().unwrap(),
            _ => return None,
        };

        let length_method = String::from_utf8_lossy(length_call.name().as_slice());
        if !LENGTH_METHODS.contains(&length_method.as_ref()) {
            return None;
        }

        if let Some(args) = length_call.arguments() {
            if args.arguments().iter().count() > 0 {
                return None;
            }
        }
        if length_call.block().is_some() {
            return None;
        }

        let args = call.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return None;
        }

        let value = Self::parse_int_from_source(source, &arg_list[0])?;
        if value <= 0 {
            return None;
        }

        let length_receiver = length_call.receiver()?;
        if !Self::receivers_match_strict(source, array_receiver, &length_receiver) {
            return None;
        }

        Some(value)
    }

    /// Check if a `CallNode` invocation is the `[]` pattern.
    fn is_bracket_call(call: &ruby_prism::CallNode) -> bool {
        let method = String::from_utf8_lossy(call.name().as_slice());
        method.as_ref() == "[]"
    }

    /// Check a call node for simple index or range patterns.
    fn check_simple_index(
        &self,
        call: &ruby_prism::CallNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let receiver = match call.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        let args = match call.arguments() {
            Some(a) => a,
            None => return vec![],
        };

        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }

        let index_arg = &arg_list[0];

        // Check for range pattern (range inside parentheses inside brackets)
        if let Some(offenses) = self.check_range_pattern(&receiver, index_arg, ctx) {
            return offenses;
        }

        // Check for simple subtraction pattern
        let neg_index = match Self::check_length_subtraction_match(ctx.source, index_arg, &receiver) {
            Some(v) => v,
            None => return vec![],
        };

        let receiver_source = Self::src(ctx.source, &receiver);
        let index_source = Self::src(ctx.source, index_arg);
        let current = format!("{}[{}]", receiver_source, index_source);
        let message = format!(
            "Use `{}[-{}]` instead of `{}`.",
            receiver_source, neg_index, current
        );

        let start = index_arg.location().start_offset();
        let end = index_arg.location().end_offset();
        let offense = ctx.offense_with_range(self.name(), &message, self.severity(), start, end);
        let replacement = format!("-{}", neg_index);
        let correction = Correction::replace(start, end, &replacement);

        vec![offense.with_correction(correction)]
    }

    /// Check range patterns: `arr[(0..(arr.length - 2))]` or `arr[(0..arr.length - 2)]`
    fn check_range_pattern(
        &self,
        array_receiver: &Node,
        index_arg: &Node,
        ctx: &CheckContext,
    ) -> Option<Vec<Offense>> {
        // The index_arg must be wrapped in ParenthesesNode
        let pn = match index_arg {
            Node::ParenthesesNode { .. } => index_arg.as_parentheses_node().unwrap(),
            _ => return None,
        };

        let body = pn.body()?;

        // Body may be StatementsNode wrapping a single expression, or the expression directly.
        // Extract the inner range node from either wrapping.
        let range_node = match &body {
            Node::StatementsNode { .. } => {
                let stmts = body.as_statements_node().unwrap();
                let first = stmts.body().iter().next()?;
                match &first {
                    Node::RangeNode { .. } => first.as_range_node().unwrap(),
                    _ => return None,
                }
            }
            Node::RangeNode { .. } => body.as_range_node().unwrap(),
            _ => return None,
        };

        let range_start = range_node.left()?;
        let range_end = range_node.right()?;

        // The range start must be a "preserving method" expression.
        // In RuboCop, `preserving_method?` returns true for any node without a receiver
        // (literals, variables), and for method calls that are preserving (sort/reverse/shuffle/rotate).
        if !Self::is_preserving_expression(&range_start) {
            return None;
        }

        let op = range_node.operator_loc();
        let range_op = &ctx.source[op.start_offset()..op.end_offset()];

        // The range end may be wrapped in parentheses or not.
        // Try to extract length subtraction from the range_end or its inner expression.
        let neg_index = self.extract_range_end_index(ctx.source, &range_end, array_receiver)?;

        let receiver_source = Self::src(ctx.source, array_receiver);
        let range_start_source = Self::src(ctx.source, &range_start);

        // Offense is on the range_end node
        let offense_start = range_end.location().start_offset();
        let offense_end = range_end.location().end_offset();

        // Build message
        let range_end_src = Self::src(ctx.source, &range_end);
        let current = format!("{}[({}{}{})]", receiver_source, range_start_source, range_op, range_end_src);
        let message = format!(
            "Use `{}[({}{}-{})]` instead of `{}`.",
            receiver_source, range_start_source, range_op, neg_index, current
        );

        let offense = ctx.offense_with_range(self.name(), &message, self.severity(), offense_start, offense_end);

        // Correction: replace the entire index_arg (the outer parenthesized expression)
        let replacement = format!("({}{}-{})", range_start_source, range_op, neg_index);
        let index_start = index_arg.location().start_offset();
        let index_end = index_arg.location().end_offset();
        let correction = Correction::replace(index_start, index_end, &replacement);

        Some(vec![offense.with_correction(correction)])
    }

    /// Extract the negative index from a range end expression.
    /// The range_end may be:
    /// 1. A direct subtraction: `arr.length - 2`
    /// 2. Wrapped in parens: `(arr.length - 2)`
    fn extract_range_end_index(
        &self,
        source: &str,
        range_end: &Node,
        array_receiver: &Node,
    ) -> Option<i64> {
        // Try direct subtraction
        if let Some(v) = Self::check_length_subtraction_strict(source, range_end, array_receiver) {
            return Some(v);
        }

        // Try unwrapping parens
        match range_end {
            Node::ParenthesesNode { .. } => {
                let pn = range_end.as_parentheses_node().unwrap();
                if let Some(body) = pn.body() {
                    match &body {
                        Node::StatementsNode { .. } => {
                            let stmts = body.as_statements_node().unwrap();
                            for inner in stmts.body().iter() {
                                if let Some(v) = Self::check_length_subtraction_strict(source, &inner, array_receiver) {
                                    return Some(v);
                                }
                            }
                        }
                        _ => {
                            if let Some(v) = Self::check_length_subtraction_strict(source, &body, array_receiver) {
                                return Some(v);
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl Cop for NegativeArrayIndex {
    fn name(&self) -> &'static str {
        "Style/NegativeArrayIndex"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_bracket_call(node) {
            return vec![];
        }

        self.check_simple_index(node, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check(source: &str) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(NegativeArrayIndex::new());
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    #[test]
    fn basic_simple_index() {
        let offenses = check("arr[arr.length - 2]");
        assert_eq!(offenses.len(), 1, "offenses: {:?}", offenses);
        assert!(offenses[0].message.contains("arr[-2]"), "msg: {}", offenses[0].message);
    }

    #[test]
    fn no_offense_different_receiver() {
        let offenses = check("arr[other.length - 2]");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn no_offense_plain_index() {
        let offenses = check("arr[1]");
        assert_eq!(offenses.len(), 0);
    }
}
