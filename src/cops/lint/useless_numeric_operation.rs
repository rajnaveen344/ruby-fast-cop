//! Lint/UselessNumericOperation cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Do not apply inconsequential numeric operations to variables.";

#[derive(Default)]
pub struct UselessNumericOperation;

impl UselessNumericOperation {
    pub fn new() -> Self { Self }
}

impl Cop for UselessNumericOperation {
    fn name(&self) -> &'static str { "Lint/UselessNumericOperation" }
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

fn useless(op: &str, n: i64) -> bool {
    match (op, n) {
        ("+", 0) | ("-", 0) => true,
        ("*", 1) | ("/", 1) | ("**", 1) => true,
        _ => false,
    }
}

fn int_value(n: &ruby_prism::Node, src: &str) -> Option<i64> {
    let i = n.as_integer_node()?;
    let loc = i.location();
    src[loc.start_offset()..loc.end_offset()].parse().ok()
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let op = node_name!(node).into_owned();
        if matches!(op.as_str(), "+" | "-" | "*" | "/" | "**") {
            if let Some(_recv) = node.receiver() {
                if let Some(args) = node.arguments() {
                    let arg_vec: Vec<_> = args.arguments().iter().collect();
                    if arg_vec.len() == 1 && node.block().is_none() {
                        if let Some(n) = int_value(&arg_vec[0], self.ctx.source) {
                            if useless(&op, n) {
                                let recv_loc = node.receiver().unwrap().location();
                                let recv_text = self.ctx.source[recv_loc.start_offset()..recv_loc.end_offset()].to_string();
                                let loc = node.location();
                                let off = self.ctx.offense_with_range(
                                    "Lint/UselessNumericOperation", MSG, Severity::Warning,
                                    loc.start_offset(), loc.end_offset(),
                                ).with_correction(Correction::replace(loc.start_offset(), loc.end_offset(), &recv_text));
                                self.out.push(off);
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let op_id = node.binary_operator();
        let op = String::from_utf8_lossy(op_id.as_slice()).into_owned();
        if matches!(op.as_str(), "+" | "-" | "*" | "/" | "**") {
            if let Some(n) = int_value(&node.value(), self.ctx.source) {
                if useless(&op, n) {
                    let name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
                    let loc = node.location();
                    let rep = format!("{} = {}", name, name);
                    let off = self.ctx.offense_with_range(
                        "Lint/UselessNumericOperation", MSG, Severity::Warning,
                        loc.start_offset(), loc.end_offset(),
                    ).with_correction(Correction::replace(loc.start_offset(), loc.end_offset(), &rep));
                    self.out.push(off);
                }
            }
        }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
}

crate::register_cop!("Lint/UselessNumericOperation", |_cfg| Some(Box::new(UselessNumericOperation::new())));
