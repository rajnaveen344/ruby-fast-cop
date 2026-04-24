//! Style/OpenStructUse cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.";

#[derive(Default)]
pub struct OpenStructUse;

impl OpenStructUse {
    pub fn new() -> Self { Self }
}

impl Cop for OpenStructUse {
    fn name(&self) -> &'static str { "Style/OpenStructUse" }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, offenses: Vec::new(), skip_offset: None };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Offset of class/module name node we want to skip checking.
    skip_offset: Option<usize>,
}

impl<'a> V<'a> {
    fn check_const_node(&mut self, n: &Node<'a>) {
        if self.skip_offset == Some(n.location().start_offset()) { return; }
        let is_open_struct = if let Some(cr) = n.as_constant_read_node() {
            node_name!(cr) == "OpenStruct"
        } else if let Some(cp) = n.as_constant_path_node() {
            // Only flag top-level: `::OpenStruct` (parent=None + cbase form).
            // Prism ConstantPathNode with parent=None IS `::OpenStruct` (cbase).
            cp.parent().is_none()
                && cp.name().map(|nm| String::from_utf8_lossy(nm.as_slice()) == "OpenStruct").unwrap_or(false)
        } else {
            false
        };
        if !is_open_struct { return; }
        let start = n.location().start_offset();
        let end = n.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/OpenStructUse", MSG, Severity::Convention, start, end,
        ));
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'a>) {
        // Skip class name itself.
        let name = node.constant_path();
        let prev = self.skip_offset;
        self.skip_offset = Some(name.location().start_offset());
        // visit name (skipped), superclass, body
        self.visit(&name);
        self.skip_offset = prev;
        if let Some(sup) = node.superclass() { self.visit(&sup); }
        if let Some(body) = node.body() { self.visit(&body); }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'a>) {
        let name = node.constant_path();
        let prev = self.skip_offset;
        self.skip_offset = Some(name.location().start_offset());
        self.visit(&name);
        self.skip_offset = prev;
        if let Some(body) = node.body() { self.visit(&body); }
    }

    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode<'a>) {
        self.check_const_node(&node.as_node());
        ruby_prism::visit_constant_read_node(self, node);
    }

    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode<'a>) {
        self.check_const_node(&node.as_node());
        ruby_prism::visit_constant_path_node(self, node);
    }
}

crate::register_cop!("Style/OpenStructUse", |_cfg| Some(Box::new(OpenStructUse::new())));
