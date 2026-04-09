//! Style/NegativeArrayIndex cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const PRESERVING_METHODS: &[&str] = &["sort", "reverse", "shuffle", "rotate"];
const LENGTH_METHODS: &[&str] = &["length", "size", "count"];

pub struct NegativeArrayIndex;

impl NegativeArrayIndex {
    pub fn new() -> Self {
        Self
    }

    fn src<'a>(source: &'a str, node: &Node) -> &'a str {
        let loc = node.location();
        &source[loc.start_offset()..loc.end_offset()]
    }

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
                let method = node_name!(call);
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

    fn is_preserving_expression(node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                match call.receiver() {
                    None => true, // bare method call / variable
                    Some(receiver) => {
                        let method = node_name!(call);
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

    fn has_chained_receiver(node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                call.receiver().is_some()
            }
            _ => false,
        }
    }

    fn receivers_match_strict(source: &str, array_receiver: &Node, length_receiver: &Node) -> bool {
        Self::is_preserving_chain(array_receiver) && Self::src(source, array_receiver) == Self::src(source, length_receiver)
    }

    fn parse_int_from_source(source: &str, node: &Node) -> Option<i64> {
        if !matches!(node, Node::IntegerNode { .. }) { return None; }
        Self::src(source, node).chars().filter(|c| *c != '_').collect::<String>().parse::<i64>().ok()
    }

    fn check_length_subtraction(source: &str, node: &Node, array_receiver: &Node, strict: bool) -> Option<i64> {
        let call = node.as_call_node()?;
        if node_name!(call) != "-" { return None; }

        let recv = call.receiver()?;
        let length_call = recv.as_call_node()?;
        if !LENGTH_METHODS.contains(&node_name!(length_call).as_ref()) { return None; }
        if length_call.arguments().map_or(false, |a| a.arguments().iter().count() > 0) { return None; }
        if length_call.block().is_some() { return None; }

        let args = call.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 { return None; }
        let value = Self::parse_int_from_source(source, &arg_list[0])?;
        if value <= 0 { return None; }

        if strict {
            let lr = length_call.receiver()?;
            if !Self::receivers_match_strict(source, array_receiver, &lr) { return None; }
        } else {
            if !Self::receivers_match(source, array_receiver, length_call.receiver().as_ref()) { return None; }
        }
        Some(value)
    }

    fn is_bracket_call(call: &ruby_prism::CallNode) -> bool {
        let method = node_name!(call);
        method.as_ref() == "[]"
    }

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
        let neg_index = match Self::check_length_subtraction(ctx.source, index_arg, &receiver, false) {
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

    fn check_range_pattern(
        &self,
        array_receiver: &Node,
        index_arg: &Node,
        ctx: &CheckContext,
    ) -> Option<Vec<Offense>> {
        let pn = index_arg.as_parentheses_node()?;
        let body = pn.body()?;
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
        if !Self::is_preserving_expression(&range_start) { return None; }

        let op = range_node.operator_loc();
        let range_op = &ctx.source[op.start_offset()..op.end_offset()];
        let neg_index = self.extract_range_end_index(ctx.source, &range_end, array_receiver)?;

        let receiver_source = Self::src(ctx.source, array_receiver);
        let range_start_source = Self::src(ctx.source, &range_start);
        let range_end_src = Self::src(ctx.source, &range_end);
        let current = format!("{}[({}{}{})]", receiver_source, range_start_source, range_op, range_end_src);
        let message = format!("Use `{}[({}{}-{})]` instead of `{}`.", receiver_source, range_start_source, range_op, neg_index, current);

        let offense = ctx.offense_with_range(self.name(), &message, self.severity(), range_end.location().start_offset(), range_end.location().end_offset());
        let replacement = format!("({}{}-{})", range_start_source, range_op, neg_index);
        let correction = Correction::replace(index_arg.location().start_offset(), index_arg.location().end_offset(), &replacement);

        Some(vec![offense.with_correction(correction)])
    }

    /// Extract the negative index from a range end expression.
    /// The range_end may be:
    /// 1. A direct subtraction: `arr.length - 2`
    /// 2. Wrapped in parens: `(arr.length - 2)`
    fn extract_range_end_index(&self, source: &str, range_end: &Node, array_receiver: &Node) -> Option<i64> {
        if let Some(v) = Self::check_length_subtraction(source, range_end, array_receiver, true) {
            return Some(v);
        }
        let pn = range_end.as_parentheses_node()?;
        let body = pn.body()?;
        match &body {
            Node::StatementsNode { .. } => {
                for inner in body.as_statements_node().unwrap().body().iter() {
                    if let Some(v) = Self::check_length_subtraction(source, &inner, array_receiver, true) {
                        return Some(v);
                    }
                }
                None
            }
            _ => Self::check_length_subtraction(source, &body, array_receiver, true),
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
