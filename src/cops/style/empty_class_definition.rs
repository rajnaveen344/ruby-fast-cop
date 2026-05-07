//! Style/EmptyClassDefinition cop
//!
//! Enforces use of `class` keyword vs `Class.new` for empty class definitions.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Clone, PartialEq)]
enum EnforcedStyle {
    /// `Class.new(Parent)` → `class Foo < Parent; end`
    ClassKeyword,
    /// `class Foo < Parent; end` → `Foo = Class.new(Parent)`
    ClassNew,
}

pub struct EmptyClassDefinition {
    style: EnforcedStyle,
}

impl Default for EmptyClassDefinition {
    fn default() -> Self {
        Self { style: EnforcedStyle::ClassKeyword }
    }
}

impl EmptyClassDefinition {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Cop for EmptyClassDefinition {
    fn name(&self) -> &'static str {
        "Style/EmptyClassDefinition"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = EmptyClassDefinitionVisitor {
            ctx,
            style: self.style.clone(),
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EmptyClassDefinitionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
}

/// Check if a node represents `Class` constant (simple or namespaced).
/// Returns true only for bare `Class` (ConstantReadNode with name "Class").
fn is_class_constant(node: &Node) -> bool {
    node.as_constant_read_node()
        .map(|c| c.name().as_slice() == b"Class")
        .unwrap_or(false)
}

/// Check if a parent (superclass) node is a "constant path" (i.e. constant or :: path).
/// Returns true for ConstantReadNode, ConstantPathNode.
/// Returns false for local vars, instance vars, class vars, global vars, self, method calls.
fn is_constant_parent(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => true,
        Node::ConstantPathNode { .. } => true,
        _ => false,
    }
}

impl<'a> EmptyClassDefinitionVisitor<'a> {
    fn check_class_keyword_style(&mut self, node: &ruby_prism::ClassNode) {
        // Flag: `class Foo < Parent; end` (empty body, has superclass)
        let superclass = match node.superclass() {
            Some(s) => s,
            None => return, // No inheritance → not flagged
        };

        // Body must be empty (nil or empty statements)
        if let Some(body) = node.body() {
            if let Some(stmts) = body.as_statements_node() {
                if stmts.body().iter().count() > 0 {
                    return; // Has body → not an empty class
                }
            }
        }

        // Superclass must be a constant (not a variable/expression)
        if !is_constant_parent(&superclass) {
            return;
        }

        let source = self.ctx.source;
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        // Build class name from constant_path (the class name node)
        let name_node = node.constant_path();
        let name_src = &source[name_node.location().start_offset()..name_node.location().end_offset()];
        let parent_src = &source[superclass.location().start_offset()..superclass.location().end_offset()];

        let corrected = format!("{} = Class.new({})", name_src, parent_src);
        let msg = "Use `Class.new` instead of the `class` keyword to define an empty class.";

        // Offense range: from start of `class` keyword to end of first line's `end` position
        // (the class keyword location starts at `class`)
        let keyword_loc = node.class_keyword_loc();
        let off_start = keyword_loc.start_offset();
        // Find the end of the class header (end of first line)
        let off_end = {
            // RuboCop uses the first line: `class Foo < Bar` → end of that line
            // which is the end of the superclass expression (or `; end` for single-line)
            // Look at end of the `class X < Y` part (before any `;` or `\n`)
            let src_from_start = &source[off_start..];
            let line_end = src_from_start.find('\n').unwrap_or(src_from_start.len());
            // Trim trailing whitespace/semicolons
            let line = src_from_start[..line_end].trim_end_matches(|c: char| c == ';' || c.is_whitespace());
            off_start + line.len()
        };

        let offense = self.ctx.offense_with_range(
            "Style/EmptyClassDefinition",
            msg,
            Severity::Convention,
            off_start,
            off_end,
        );
        let correction = Correction::replace(start, end, corrected);
        self.offenses.push(offense.with_correction(correction));
    }

    fn check_class_new_style(&mut self, node: &ruby_prism::ConstantWriteNode) {
        // Flag: `FooError = Class.new(Parent)` (parent must be constant, no block, no chaining)
        let value = node.value();

        // value must be a CallNode: Class.new(Parent)
        let call = match value.as_call_node() {
            Some(c) => c,
            None => return,
        };

        // Must be `new` method on `Class` constant
        if call.name().as_slice() != b"new" {
            return;
        }
        let receiver = match call.receiver() {
            Some(r) => r,
            None => return,
        };
        if !is_class_constant(&receiver) {
            return;
        }

        // Must have exactly 1 argument (the parent class), which must be a constant
        let args = match call.arguments() {
            Some(a) => a,
            None => return, // Class.new without parent → not flagged
        };
        let args_vec: Vec<_> = args.arguments().iter().collect();
        if args_vec.len() != 1 {
            return;
        }
        let parent = &args_vec[0];
        if !is_constant_parent(parent) {
            return;
        }

        // Must have no block
        if call.block().is_some() {
            return;
        }

        // Must be at statement level (the ConstantWriteNode itself is a statement)
        // This is checked by the fact that we're inside ConstantWriteNode visitor.
        // However we also need to make sure the whole expression isn't chained:
        // `Class.new(X).tap { }` — call's location ends at `new(X)`, not `.tap { }`
        // Since we pattern-match on ConstantWriteNode and its value is the CallNode directly,
        // any chaining would produce a different node structure.

        let source = self.ctx.source;
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        let name_src = &source[node.name_loc().start_offset()..node.name_loc().end_offset()];
        let parent_src = &source[parent.location().start_offset()..parent.location().end_offset()];

        let indent = " ".repeat(self.ctx.col_of(start));
        let corrected = format!("class {} < {}\n{}end", name_src, parent_src, indent);

        // Offense range: entire ConstantWriteNode
        let off_start = start;
        let off_end = end;

        let msg = "Use the `class` keyword instead of `Class.new` to define an empty class.";
        let offense = self.ctx.offense_with_range(
            "Style/EmptyClassDefinition",
            msg,
            Severity::Convention,
            off_start,
            off_end,
        );
        let correction = Correction::replace(start, end, corrected);
        self.offenses.push(offense.with_correction(correction));
    }
}

impl<'a> Visit<'_> for EmptyClassDefinitionVisitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if self.style == EnforcedStyle::ClassNew {
            self.check_class_keyword_style(node);
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        if self.style == EnforcedStyle::ClassKeyword {
            self.check_class_new_style(node);
        }
        ruby_prism::visit_constant_write_node(self, node);
    }
}

crate::register_cop!("Style/EmptyClassDefinition", |cfg| {
    let style_str = cfg
        .get_cop_config("Style/EmptyClassDefinition")
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str())
        .unwrap_or("class_keyword");

    let style = match style_str {
        "class_new" => EnforcedStyle::ClassNew,
        // "class_keyword" and "class_definition" (deprecated alias) → ClassKeyword
        _ => EnforcedStyle::ClassKeyword,
    };

    Some(Box::new(EmptyClassDefinition::new(style)))
});
