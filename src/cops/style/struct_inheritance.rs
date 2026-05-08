//! Style/StructInheritance cop
//!
//! Don't extend an instance initialized by Struct.new.

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_start_offset;
use crate::offense::{Correction, Edit, Offense, Severity};
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
        let correction = build_correction(node, &superclass, self.ctx.source);
        let offense = self.ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, start, end);
        self.offenses.push(if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        });
    }
}

impl Visit<'_> for StructInheritanceVisitor<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.check_class(node);
        ruby_prism::visit_class_node(self, node);
    }
}

/// Build correction: `class Person < Struct.new(...)` → `Person = Struct.new(...) [do...end]`
fn build_correction(class_node: &ruby_prism::ClassNode, superclass: &Node, source: &str) -> Option<Correction> {
    let class_kw = class_node.class_keyword_loc();
    let op_loc = class_node.inheritance_operator_loc()?;
    let end_kw = class_node.end_keyword_loc();
    let has_body = class_node.body().is_some();

    let mut edits: Vec<Edit> = Vec::new();

    // Op1: remove `class ` (class keyword + trailing space)
    // range_with_surrounding_space(class_kw, newlines:false) → expand right past spaces
    let kw_start = class_kw.start_offset();
    let mut kw_end = class_kw.end_offset();
    let src_bytes = source.as_bytes();
    while kw_end < src_bytes.len() && src_bytes[kw_end] == b' ' {
        kw_end += 1;
    }
    edits.push(Edit { start_offset: kw_start, end_offset: kw_end, replacement: String::new() });

    // Op2: replace `<` with `=`
    edits.push(Edit {
        start_offset: op_loc.start_offset(),
        end_offset: op_loc.end_offset(),
        replacement: "=".into(),
    });

    // Op3: correct_parent
    match superclass {
        Node::CallNode { .. } => {
            let call = superclass.as_call_node().unwrap();
            if let Some(block_node_enum) = call.block() {
                if let Some(block) = block_node_enum.as_block_node() {
                    // parent.block_type? → remove ` end` from block closing (the inline `end`)
                    // range_with_surrounding_space(parent.loc.end, newlines:false)
                    let closing = block.closing_loc();
                    let close_start = closing.start_offset();
                    // expand left past spaces (not newlines)
                    let mut close_left = close_start;
                    while close_left > 0 && src_bytes[close_left - 1] == b' ' {
                        close_left -= 1;
                    }
                    edits.push(Edit {
                        start_offset: close_left,
                        end_offset: closing.end_offset(),
                        replacement: String::new(),
                    });
                    // The outer class `end` becomes the block's `end` — no removal needed.
                }
            } else if !has_body {
                // No body — remove from struct_end to class end
                let struct_end = superclass.location().end_offset();
                let class_end = class_node.location().end_offset();
                let single_line = !source[struct_end..class_end].contains('\n');
                if single_line {
                    // remove from struct_end to class_end (e.g. `; end`)
                    edits.push(Edit {
                        start_offset: struct_end,
                        end_offset: class_end,
                        replacement: String::new(),
                    });
                } else {
                    // remove `\nend` (the whole end line including leading newline)
                    let end_line_start = line_start_offset(source, end_kw.start_offset());
                    let end_line_end = if end_kw.end_offset() < source.len() && src_bytes[end_kw.end_offset()] == b'\n' {
                        end_kw.end_offset() + 1
                    } else {
                        end_kw.end_offset()
                    };
                    edits.push(Edit {
                        start_offset: end_line_start,
                        end_offset: end_line_end,
                        replacement: String::new(),
                    });
                }
            } else {
                // Has body — insert ` do` after parent (Struct.new call)
                // The class `end` stays and becomes the block's `end`
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
