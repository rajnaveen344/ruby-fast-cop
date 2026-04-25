//! Lint/NumericOperationWithConstantResult cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Numeric operation with a constant result detected.";

#[derive(Default)]
pub struct NumericOperationWithConstantResult;

impl NumericOperationWithConstantResult {
    pub fn new() -> Self { Self }
}

impl Cop for NumericOperationWithConstantResult {
    fn name(&self) -> &'static str { "Lint/NumericOperationWithConstantResult" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    out: Vec<Offense>,
}

fn lhs_text(n: &ruby_prism::Node, src: &str) -> Option<String> {
    if let Some(c) = n.as_call_node() {
        if c.receiver().is_none() && c.arguments().is_none() && c.block().is_none() {
            let loc = c.location();
            return Some(src[loc.start_offset()..loc.end_offset()].to_string());
        }
    }
    if let Some(lv) = n.as_local_variable_read_node() {
        return Some(String::from_utf8_lossy(lv.name().as_slice()).into_owned());
    }
    None
}

fn rhs_text(n: &ruby_prism::Node, src: &str) -> Option<String> {
    if n.as_integer_node().is_some() {
        let loc = n.location();
        return Some(src[loc.start_offset()..loc.end_offset()].to_string());
    }
    if let Some(c) = n.as_call_node() {
        if c.receiver().is_none() && c.arguments().is_none() && c.block().is_none() {
            let loc = c.location();
            return Some(src[loc.start_offset()..loc.end_offset()].to_string());
        }
    }
    if let Some(lv) = n.as_local_variable_read_node() {
        return Some(String::from_utf8_lossy(lv.name().as_slice()).into_owned());
    }
    None
}

fn constant_result(lhs: &str, op: &str, rhs: &str) -> Option<&'static str> {
    if rhs == "0" {
        if op == "*" { return Some("0"); }
        if op == "**" { return Some("1"); }
    }
    if rhs == lhs && op == "/" { return Some("1"); }
    None
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let op = node_name!(node).into_owned();
        if matches!(op.as_str(), "*" | "/" | "**") {
            if let Some(recv) = node.receiver() {
                if let Some(lhs) = lhs_text(&recv, self.ctx.source) {
                    if let Some(args) = node.arguments() {
                        let arg_vec: Vec<_> = args.arguments().iter().collect();
                        if arg_vec.len() == 1 && node.block().is_none() {
                            if let Some(rhs) = rhs_text(&arg_vec[0], self.ctx.source) {
                                if let Some(result) = constant_result(&lhs, &op, &rhs) {
                                    let loc = node.location();
                                    let off = self.ctx.offense_with_range(
                                        "Lint/NumericOperationWithConstantResult", MSG, Severity::Warning,
                                        loc.start_offset(), loc.end_offset(),
                                    ).with_correction(Correction::replace(loc.start_offset(), loc.end_offset(), result));
                                    self.out.push(off);
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let op = String::from_utf8_lossy(node.binary_operator().as_slice()).into_owned();
        if matches!(op.as_str(), "*" | "/" | "**") {
            let lhs = String::from_utf8_lossy(node.name().as_slice()).into_owned();
            if let Some(rhs) = rhs_text(&node.value(), self.ctx.source) {
                if let Some(result) = constant_result(&lhs, &op, &rhs) {
                    let loc = node.location();
                    let rep = format!("{} = {}", lhs, result);
                    let off = self.ctx.offense_with_range(
                        "Lint/NumericOperationWithConstantResult", MSG, Severity::Warning,
                        loc.start_offset(), loc.end_offset(),
                    ).with_correction(Correction::replace(loc.start_offset(), loc.end_offset(), &rep));
                    self.out.push(off);
                }
            }
        }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
}

crate::register_cop!("Lint/NumericOperationWithConstantResult", |_cfg| Some(Box::new(NumericOperationWithConstantResult::new())));
