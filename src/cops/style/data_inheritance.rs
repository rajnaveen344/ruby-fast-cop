//! Style/DataInheritance cop
//!
//! Don't extend an instance initialized by `Data.define`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/DataInheritance";
const MSG: &str =
    "Don't extend an instance initialized by `Data.define`. Use a block to customize the class.";

#[derive(Default)]
pub struct DataInheritance;

impl DataInheritance {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for DataInheritance {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 2) {
            return vec![];
        }
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(parent) = node.superclass() {
            if is_data_define(&parent) {
                let start = parent.location().start_offset();
                let end = parent.location().end_offset();
                self.offenses.push(
                    self.ctx
                        .offense_with_range(COP_NAME, MSG, Severity::Convention, start, end),
                );
            }
        }
        ruby_prism::visit_class_node(self, node);
    }
}

/// Matches `Data.define(...)` or `::Data.define(...)`, optionally with a block.
fn is_data_define(node: &Node) -> bool {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            is_data_define_call(&call)
        }
        _ => false,
    }
}

fn is_data_define_call(call: &ruby_prism::CallNode) -> bool {
    let method = String::from_utf8_lossy(call.name().as_slice());
    if method != "define" {
        return false;
    }
    match call.receiver() {
        Some(recv) => is_data_const(&recv),
        None => false,
    }
}

fn is_data_const(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            let n = node.as_constant_read_node().unwrap();
            String::from_utf8_lossy(n.name().as_slice()) == "Data"
        }
        Node::ConstantPathNode { .. } => {
            // ::Data — parent is None (rooted), name should be "Data"
            let path = node.as_constant_path_node().unwrap();
            if path.parent().is_some() {
                return false;
            }
            path.name().is_some_and(|id| {
                String::from_utf8_lossy(id.as_slice()) == "Data"
            })
        }
        _ => false,
    }
}

crate::register_cop!("Style/DataInheritance", |_cfg| {
    Some(Box::new(DataInheritance::new()))
});
