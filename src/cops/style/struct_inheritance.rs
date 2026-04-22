//! Style/StructInheritance cop
//!
//! Don't extend an instance initialized by Struct.new.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/StructInheritance";
const MSG: &str = "Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.";

#[derive(Default)]
pub struct StructInheritance;

impl StructInheritance {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for StructInheritance {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = StructInheritanceVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct StructInheritanceVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl StructInheritanceVisitor<'_> {
    fn check_class(&mut self, node: &ruby_prism::ClassNode) {
        let superclass = match node.superclass() {
            Some(s) => s,
            None => return,
        };

        if !is_struct_new(&superclass) {
            return;
        }

        let start = superclass.location().start_offset();
        let end = superclass.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, start, end));
    }
}

impl Visit<'_> for StructInheritanceVisitor<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.check_class(node);
        ruby_prism::visit_class_node(self, node);
    }
}

/// Returns true if node is `Struct.new(...)` or `::Struct.new(...)`,
/// possibly with a block (CallNode with block).
fn is_struct_new(node: &Node) -> bool {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            is_struct_new_call(&call)
        }
        _ => false,
    }
}

fn is_struct_new_call(call: &ruby_prism::CallNode) -> bool {
    let method = String::from_utf8_lossy(call.name().as_slice());
    if method != "new" {
        return false;
    }
    match call.receiver() {
        Some(recv) => is_struct_const(&recv),
        None => false,
    }
}

fn is_struct_const(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            let n = node.as_constant_read_node().unwrap();
            let name = String::from_utf8_lossy(n.name().as_slice());
            name == "Struct"
        }
        Node::ConstantPathNode { .. } => {
            // ::Struct — parent is None (rooted), name should be "Struct"
            let path = node.as_constant_path_node().unwrap();
            if path.parent().is_some() {
                return false; // Not root-scoped
            }
            path.name().map_or(false, |id| {
                let name = String::from_utf8_lossy(id.as_slice());
                name == "Struct"
            })
        }
        _ => false,
    }
}

crate::register_cop!("Style/StructInheritance", |_cfg| {
    Some(Box::new(StructInheritance::new()))
});
