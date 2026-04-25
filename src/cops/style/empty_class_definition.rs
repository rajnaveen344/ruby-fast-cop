//! Style/EmptyClassDefinition cop
//!
//! Enforces consistent style for empty class definitions: `class Foo; end` vs
//! `Foo = Class.new`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/EmptyClassDefinition";
const MSG_CLASS_KEYWORD: &str =
    "Use the `class` keyword instead of `Class.new` to define an empty class.";
const MSG_CLASS_NEW: &str =
    "Use `Class.new` instead of the `class` keyword to define an empty class.";

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    ClassKeyword,
    ClassNew,
}

pub struct EmptyClassDefinition {
    style: EnforcedStyle,
    allowed_parent_classes: Vec<String>,
}

impl EmptyClassDefinition {
    pub fn new(style: EnforcedStyle, allowed_parent_classes: Vec<String>) -> Self {
        Self { style, allowed_parent_classes }
    }
}

impl Cop for EmptyClassDefinition {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    cop: &'a EmptyClassDefinition,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_class_keyword(&mut self, node: &ruby_prism::ConstantWriteNode) {
        // matcher: (casgn _ _ (send (const _ :Class) :new $arg?))
        let value = node.value();
        let call = match value.as_call_node() {
            Some(c) => c,
            None => return,
        };
        // Method must be `new`
        let method = String::from_utf8_lossy(call.name().as_slice());
        if method != "new" {
            return;
        }
        // No block
        if call.block().is_some() {
            return;
        }
        // Receiver must be a constant `Class` (not ::Class — RuboCop pattern uses `(const _ :Class)`
        // which matches ConstantReadNode `Class` only when nested const isn't cbase).
        let recv = match call.receiver() {
            Some(r) => r,
            None => return,
        };
        if !is_class_constant(&recv) {
            return;
        }
        // RuboCop matcher requires exactly one arg: `(send (const _ :Class) :new _)`.
        // Zero args (`Class.new`) does NOT match → don't flag.
        let args = call.arguments();
        let first_arg: Option<Node> = args
            .as_ref()
            .and_then(|a| a.arguments().iter().next());
        let first_arg = match first_arg {
            Some(a) => a,
            None => return,
        };
        if !is_constant_arg(&first_arg) {
            return;
        }
        // AllowedParentClasses
        let src = self
            .ctx
            .src(first_arg.location().start_offset(), first_arg.location().end_offset());
        if self.cop.allowed_parent_classes.iter().any(|c| c == src) {
            return;
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG_CLASS_KEYWORD,
            Severity::Convention,
            start,
            end,
        ));
    }

    fn check_class_new(&mut self, node: &ruby_prism::ClassNode) {
        // Skip if no superclass
        let parent = match node.superclass() {
            Some(p) => p,
            None => return,
        };
        // Skip if body has children
        if let Some(body) = node.body() {
            if let Some(stmts) = body.as_statements_node() {
                if stmts.body().iter().next().is_some() {
                    return;
                }
            } else {
                // Single non-statements body = non-empty
                return;
            }
        }
        // AllowedParentClasses
        let psrc = self.ctx.src(parent.location().start_offset(), parent.location().end_offset());
        if self.cop.allowed_parent_classes.iter().any(|c| c == psrc) {
            return;
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG_CLASS_NEW,
            Severity::Convention,
            start,
            end,
        ));
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        if matches!(self.cop.style, EnforcedStyle::ClassKeyword) {
            self.check_class_keyword(node);
        }
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if matches!(self.cop.style, EnforcedStyle::ClassNew) {
            self.check_class_new(node);
        }
        ruby_prism::visit_class_node(self, node);
    }
}

fn is_class_constant(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            let n = node.as_constant_read_node().unwrap();
            String::from_utf8_lossy(n.name().as_slice()) == "Class"
        }
        _ => false,
    }
}

/// RuboCop matcher requires parent class be `const_type?`. In Prism: ConstantReadNode
/// or ConstantPathNode. Self/local-vars/instance-vars/class-vars/global-vars are not.
fn is_constant_arg(node: &Node) -> bool {
    matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })
}

crate::register_cop!("Style/EmptyClassDefinition", |cfg| {
    let cc = cfg.get_cop_config("Style/EmptyClassDefinition");
    let style_str = cc
        .and_then(|c| c.enforced_style.clone())
        .unwrap_or_else(|| "class_keyword".to_string());
    let style = match style_str.as_str() {
        "class_new" => EnforcedStyle::ClassNew,
        // "class_keyword", "class_definition" (deprecated alias) → ClassKeyword
        _ => EnforcedStyle::ClassKeyword,
    };
    let allowed = cc
        .and_then(|c| c.raw.get("AllowedParentClasses"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(Box::new(EmptyClassDefinition::new(style, allowed)))
});
