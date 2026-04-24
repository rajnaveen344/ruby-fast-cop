//! Style/RedundantConstantBase
//!
//! Flags `::Foo` (cbase-prefixed constant) where `Module.nesting` is empty so
//! the `::` prefix is redundant. Allowed inside `class Foo < ::Bar` super-class
//! position, and suppressed entirely when `Lint/ConstantResolution` is enabled.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{ConstantPathNode, Node, ProgramNode, Visit};

const MSG: &str = "Remove redundant `::`.";

#[derive(Default)]
pub struct RedundantConstantBase {
    lint_constant_resolution_enabled: bool,
}

impl RedundantConstantBase {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(lint_constant_resolution_enabled: bool) -> Self {
        Self { lint_constant_resolution_enabled }
    }
}

impl Cop for RedundantConstantBase {
    fn name(&self) -> &'static str {
        "Style/RedundantConstantBase"
    }

    fn check_program(&self, node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if self.lint_constant_resolution_enabled {
            return vec![];
        }
        let mut v = Finder {
            ctx,
            cop_name: self.name(),
            offenses: Vec::new(),
            // Stack of class/module contexts: (is_class, super_class_ids).
            // `super_class_ids` holds the pointer-ids of ConstantPathNode descendants
            // living inside the parent class's super-class expression — those are
            // whitelisted (not flagged).
            in_class_or_module_depth: 0,
            super_whitelist: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Finder<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    cop_name: &'static str,
    offenses: Vec<Offense>,
    in_class_or_module_depth: usize,
    /// Byte-offsets of ConstantPathNode locations that appear inside a class's
    /// superclass expression and are therefore allowed.
    super_whitelist: Vec<(usize, usize)>,
}

impl<'a, 'b> Finder<'a, 'b> {
    fn is_whitelisted(&self, node: &ConstantPathNode) -> bool {
        let s = node.location().start_offset();
        let e = node.location().end_offset();
        self.super_whitelist.iter().any(|(ws, we)| *ws == s && *we == e)
    }

    fn collect_cbase_paths(&mut self, node: &Node) {
        // Record every ConstantPathNode with `parent() == None` (i.e. `::X`)
        // found within `node`.
        match node {
            Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                if cp.parent().is_none() {
                    let s = cp.location().start_offset();
                    let e = cp.location().end_offset();
                    self.super_whitelist.push((s, e));
                }
                if let Some(parent) = cp.parent() {
                    self.collect_cbase_paths(&parent);
                }
            }
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                if let Some(recv) = c.receiver() {
                    self.collect_cbase_paths(&recv);
                }
                if let Some(args) = c.arguments() {
                    for a in args.arguments().iter() {
                        self.collect_cbase_paths(&a);
                    }
                }
            }
            _ => {}
        }
    }
}

impl<'pr> Visit<'pr> for Finder<'_, '_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        // Visit the superclass expression BEFORE incrementing depth so that
        // `::Bar` in `class Foo < ::Bar` is still flagged at the top level.
        // But when already nested inside another class/module, `::Bar` in
        // the super position is meaningful and should be whitelisted.
        let added_len = self.super_whitelist.len();
        if self.in_class_or_module_depth > 0 {
            if let Some(ref sc) = node.superclass() {
                self.collect_cbase_paths(sc);
            }
        }
        // Visit superclass at current (pre-increment) depth.
        if let Some(ref sc) = node.superclass() {
            self.visit(sc);
        }
        self.in_class_or_module_depth += 1;
        // Visit body only.
        if let Some(body) = node.body() {
            self.visit(&body);
        }
        self.in_class_or_module_depth -= 1;
        self.super_whitelist.truncate(added_len);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        self.in_class_or_module_depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.in_class_or_module_depth -= 1;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'pr>) {
        // `class << self` is NOT treated as class/module nesting for this cop.
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_constant_path_node(&mut self, node: &ConstantPathNode<'pr>) {
        // Flag only when `parent()` is None (the `::Foo` case, no LHS).
        if node.parent().is_none()
            && self.in_class_or_module_depth == 0
            && !self.is_whitelisted(node)
        {
            let start = node.location().start_offset();
            // The `::` spans the first two bytes of the node.
            let end = start + 2;
            let correction = Correction::delete(start, end);
            self.offenses.push(
                self.ctx
                    .offense_with_range(self.cop_name, MSG, Severity::Convention, start, end)
                    .with_correction(correction),
            );
        }
        ruby_prism::visit_constant_path_node(self, node);
    }
}

crate::register_cop!("Style/RedundantConstantBase", |cfg| {
    let lint_cr = cfg.is_cop_enabled("Lint/ConstantResolution");
    Some(Box::new(RedundantConstantBase::with_config(lint_cr)))
});
