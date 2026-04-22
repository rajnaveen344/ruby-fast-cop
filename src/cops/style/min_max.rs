//! Style/MinMax cop
//!
//! Checks for [foo.min, foo.max] / return foo.min, foo.max patterns that can use minmax.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/MinMax";

#[derive(Default)]
pub struct MinMax;

impl MinMax {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for MinMax {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MinMaxVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct MinMaxVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> MinMaxVisitor<'a> {
    fn src(&self, node: &Node) -> &'a str {
        let loc = node.location();
        self.ctx.src(loc.start_offset(), loc.end_offset())
    }

    fn receiver_src(call: &ruby_prism::CallNode) -> Option<String> {
        let recv = call.receiver()?;
        Some(String::from_utf8_lossy(recv.location().as_slice()).to_string())
    }

    fn receiver_src_of_node(node: &Node) -> Option<String> {
        let call = node.as_call_node()?;
        let recv = call.receiver()?;
        Some(String::from_utf8_lossy(recv.location().as_slice()).to_string())
    }

    /// Check [foo.min, foo.max] array literal
    fn check_array(&mut self, node: &ruby_prism::ArrayNode) {
        let elements: Vec<Node> = node.elements().iter().collect();
        if elements.len() != 2 {
            return;
        }
        if let Some((recv_src, _)) = self.match_min_max_pair(&elements[0], &elements[1]) {
            let msg = format!(
                "Use `{recv_src}.minmax` instead of `[{}, {}]`.",
                self.src(&elements[0]),
                self.src(&elements[1])
            );
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, end));
        }
    }

    /// Check `return foo.min, foo.max` or `bar = foo.min, foo.max` (implicit array)
    fn check_implicit_array(&mut self, elements: &[Node], start: usize, end: usize) {
        if elements.len() != 2 {
            return;
        }
        if let Some((recv_src, _)) = self.match_min_max_pair(&elements[0], &elements[1]) {
            let lhs_src = self.src(&elements[0]);
            let rhs_src = self.src(&elements[1]);
            let msg = format!(
                "Use `{recv_src}.minmax` instead of `{lhs_src}, {rhs_src}`."
            );
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, end));
        }
    }

    /// Returns Some((receiver_src, receiver_node)) if the two nodes are foo.min + foo.max with same receiver
    fn match_min_max_pair(&self, first: &Node, second: &Node) -> Option<(String, ())> {
        let call1 = first.as_call_node()?;
        let call2 = second.as_call_node()?;
        let name1 = node_name!(call1);
        let name2 = node_name!(call2);
        if name1 != "min" || name2 != "max" {
            return None;
        }
        // Both must have no arguments
        if call1.arguments().is_some() || call2.arguments().is_some() {
            return None;
        }
        // Both must have explicit receivers
        let recv1_src = Self::receiver_src(&call1)?;
        let recv2_src = Self::receiver_src(&call2)?;
        if recv1_src.is_empty() || recv1_src != recv2_src {
            return None;
        }
        Some((recv1_src, ()))
    }
}

impl Visit<'_> for MinMaxVisitor<'_> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        // Only flag explicit bracket arrays like [foo.min, foo.max]
        let arr_src = self.ctx.src(node.location().start_offset(), node.location().end_offset());
        if arr_src.starts_with('[') {
            self.check_array(node);
        }
        ruby_prism::visit_array_node(self, node);
    }

    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        if let Some(args) = node.arguments() {
            let elements: Vec<Node> = args.arguments().iter().collect();
            if !elements.is_empty() {
                let start = elements.first().unwrap().location().start_offset();
                let end = elements.last().unwrap().location().end_offset();
                self.check_implicit_array(&elements, start, end);
            }
        }
        ruby_prism::visit_return_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        // bar = foo.min, foo.max — RHS is an implicit ArrayNode (no brackets)
        let value = node.value();
        if let Some(arr) = value.as_array_node() {
            let arr_src = self.ctx.src(arr.location().start_offset(), arr.location().end_offset());
            if !arr_src.starts_with('[') {
                let elements: Vec<Node> = arr.elements().iter().collect();
                if !elements.is_empty() {
                    let start = elements.first().unwrap().location().start_offset();
                    let end = elements.last().unwrap().location().end_offset();
                    self.check_implicit_array(&elements, start, end);
                }
            }
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
}

crate::register_cop!("Style/MinMax", |_cfg| {
    Some(Box::new(MinMax::new()))
});
