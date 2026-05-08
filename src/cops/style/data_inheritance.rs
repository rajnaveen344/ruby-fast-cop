//! Style/DataInheritance cop
//!
//! Don't extend an instance initialized by `Data.define`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_start_offset;
use crate::offense::{Correction, Edit, Offense, Severity};
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
                let correction = build_correction(node, &parent, self.ctx.source);
                let offense = self.ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, start, end);
                self.offenses.push(if let Some(c) = correction {
                    offense.with_correction(c)
                } else {
                    offense
                });
            }
        }
        ruby_prism::visit_class_node(self, node);
    }
}

/// Build correction: same shape as StructInheritance.
fn build_correction(class_node: &ruby_prism::ClassNode, superclass: &Node, source: &str) -> Option<Correction> {
    let class_kw = class_node.class_keyword_loc();
    let op_loc = class_node.inheritance_operator_loc()?;
    let end_kw = class_node.end_keyword_loc();
    let has_body = class_node.body().is_some();
    let src_bytes = source.as_bytes();

    let mut edits: Vec<Edit> = Vec::new();

    // Remove `class ` keyword + trailing space
    let kw_start = class_kw.start_offset();
    let mut kw_end = class_kw.end_offset();
    while kw_end < src_bytes.len() && src_bytes[kw_end] == b' ' {
        kw_end += 1;
    }
    edits.push(Edit { start_offset: kw_start, end_offset: kw_end, replacement: String::new() });

    // Replace `<` with `=`
    edits.push(Edit {
        start_offset: op_loc.start_offset(),
        end_offset: op_loc.end_offset(),
        replacement: "=".into(),
    });

    match superclass {
        Node::CallNode { .. } => {
            let call = superclass.as_call_node().unwrap();
            if let Some(block_node_enum) = call.block() {
                if let Some(block) = block_node_enum.as_block_node() {
                    // Remove ` end` from inline block closing
                    let closing = block.closing_loc();
                    let mut close_left = closing.start_offset();
                    while close_left > 0 && src_bytes[close_left - 1] == b' ' {
                        close_left -= 1;
                    }
                    edits.push(Edit {
                        start_offset: close_left,
                        end_offset: closing.end_offset(),
                        replacement: String::new(),
                    });
                    // Outer class `end` stays as block's `end`
                }
            } else if !has_body {
                let struct_end = superclass.location().end_offset();
                let class_end = class_node.location().end_offset();
                let single_line = !source[struct_end..class_end].contains('\n');
                if single_line {
                    edits.push(Edit { start_offset: struct_end, end_offset: class_end, replacement: String::new() });
                } else {
                    let end_line_start = line_start_offset(source, end_kw.start_offset());
                    let end_line_end = if end_kw.end_offset() < source.len() && src_bytes[end_kw.end_offset()] == b'\n' {
                        end_kw.end_offset() + 1
                    } else {
                        end_kw.end_offset()
                    };
                    edits.push(Edit { start_offset: end_line_start, end_offset: end_line_end, replacement: String::new() });
                }
            } else {
                // Insert ` do` after superclass; class `end` stays
                edits.push(Edit {
                    start_offset: superclass.location().end_offset(),
                    end_offset: superclass.location().end_offset(),
                    replacement: " do".into(),
                });
            }
        }
        _ => {}
    }

    Some(Correction { edits })
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
