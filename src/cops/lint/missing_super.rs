//! Lint/MissingSuper cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const STATELESS_CLASSES: &[&str] = &["BasicObject", "Object"];

const CLASS_LIFECYCLE_CALLBACKS: &[&str] = &["inherited"];
const METHOD_LIFECYCLE_CALLBACKS: &[&str] = &[
    "method_added",
    "method_removed",
    "method_undefined",
    "singleton_method_added",
    "singleton_method_removed",
    "singleton_method_undefined",
];

pub struct MissingSuper {
    allowed_parent_classes: Vec<String>,
}

impl MissingSuper {
    pub fn new(allowed_parent_classes: Vec<String>) -> Self {
        Self { allowed_parent_classes }
    }
}

impl Default for MissingSuper {
    fn default() -> Self {
        Self::new(vec![])
    }
}

impl Cop for MissingSuper {
    fn name(&self) -> &'static str {
        "Lint/MissingSuper"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MissingSuperVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            class_stack: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Context pushed when entering a class/sclass/Class.new block
#[derive(Debug, Clone)]
enum ClassCtx {
    /// `class Foo < Parent` — parent const name
    Named { parent: Option<String> },
    /// `Class.new(Parent)` block
    ClassNew { parent: Option<String> },
    /// `class << self`
    Singleton,
}

struct MissingSuperVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a MissingSuper,
    offenses: Vec<Offense>,
    class_stack: Vec<ClassCtx>,
}

impl<'a> MissingSuperVisitor<'a> {
    fn is_allowed_class(&self, name: &str) -> bool {
        STATELESS_CLASSES.contains(&name)
            || self.cop.allowed_parent_classes.iter().any(|a| a == name)
    }

    fn const_name(node: &Node) -> Option<String> {
        if let Some(cr) = node.as_constant_read_node() {
            return Some(String::from_utf8_lossy(cr.name().as_slice()).to_string());
        }
        if let Some(cp) = node.as_constant_path_node() {
            return cp.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string());
        }
        None
    }

    fn def_contains_super(node: &ruby_prism::DefNode) -> bool {
        struct Finder(bool);
        impl Visit<'_> for Finder {
            fn visit_super_node(&mut self, _: &ruby_prism::SuperNode) { self.0 = true; }
            fn visit_forwarding_super_node(&mut self, _: &ruby_prism::ForwardingSuperNode) { self.0 = true; }
            fn visit_def_node(&mut self, _: &ruby_prism::DefNode) {} // skip nested
            fn visit_class_node(&mut self, _: &ruby_prism::ClassNode) {} // skip nested
        }
        let mut f = Finder(false);
        f.visit_def_node(node);
        f.0
    }

    fn is_callback(name: &str) -> bool {
        CLASS_LIFECYCLE_CALLBACKS.contains(&name) || METHOD_LIFECYCLE_CALLBACKS.contains(&name)
    }

    fn inside_class_with_stateful_parent(&self) -> bool {
        match self.class_stack.last() {
            Some(ClassCtx::Named { parent: Some(p) }) => !self.is_allowed_class(p),
            Some(ClassCtx::ClassNew { parent: Some(p) }) => !self.is_allowed_class(p),
            _ => false,
        }
    }

    /// True if we are directly inside a class or singleton class (not just a module).
    fn inside_any_class(&self) -> bool {
        self.class_stack.iter().any(|c| matches!(c, ClassCtx::Named { .. } | ClassCtx::Singleton | ClassCtx::ClassNew { .. }))
    }

    fn check_def_node(&mut self, node: &ruby_prism::DefNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let has_receiver = node.receiver().is_some();

        if has_receiver {
            // def self.foo — class-level method
            // Callbacks like `def self.inherited` — check
            if Self::is_callback(&method_name) && self.inside_any_class() {
                if !Self::def_contains_super(node) {
                    let start = node.location().start_offset();
                    let end = node.name_loc().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Lint/MissingSuper",
                        "Call `super` to invoke callback defined in the parent class.",
                        Severity::Warning,
                        start,
                        end,
                    ));
                }
            }
        } else {
            // instance method
            if method_name == "initialize" {
                if self.inside_class_with_stateful_parent() && !Self::def_contains_super(node) {
                    let start = node.location().start_offset();
                    let end = node.name_loc().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Lint/MissingSuper",
                        "Call `super` to initialize state of the parent class.",
                        Severity::Warning,
                        start,
                        end,
                    ));
                }
            } else if Self::is_callback(&method_name) && self.inside_any_class() {
                if !Self::def_contains_super(node) {
                    let start = node.location().start_offset();
                    let end = node.name_loc().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Lint/MissingSuper",
                        "Call `super` to invoke callback defined in the parent class.",
                        Severity::Warning,
                        start,
                        end,
                    ));
                }
            }
        }
    }
}

impl<'a> Visit<'_> for MissingSuperVisitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let parent_name = node.superclass().and_then(|p| Self::const_name(&p));
        self.class_stack.push(ClassCtx::Named { parent: parent_name });
        ruby_prism::visit_class_node(self, node);
        self.class_stack.pop();
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        self.class_stack.push(ClassCtx::Singleton);
        ruby_prism::visit_singleton_class_node(self, node);
        self.class_stack.pop();
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        // Inside a module, callbacks should NOT be flagged.
        // Save and clear the class stack.
        let saved = std::mem::take(&mut self.class_stack);
        ruby_prism::visit_module_node(self, node);
        self.class_stack = saved;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Detect `Class.new(Parent)` pattern — push ClassNew context for its block
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if method == "new" {
            if let Some(recv) = node.receiver() {
                if let Some(cr) = recv.as_constant_read_node() {
                    let cname = String::from_utf8_lossy(cr.name().as_slice()).to_string();
                    if cname == "Class" {
                        let parent_name = node.arguments().and_then(|args| {
                            let arg_list: Vec<_> = args.arguments().iter().collect();
                            // Filter out block-pass arguments
                            let positional: Vec<_> = arg_list.iter()
                                .filter(|a| a.as_block_argument_node().is_none())
                                .collect();
                            positional.first().and_then(|a| Self::const_name(a))
                        });
                        self.class_stack.push(ClassCtx::ClassNew { parent: parent_name });
                        ruby_prism::visit_call_node(self, node);
                        self.class_stack.pop();
                        return;
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.check_def_node(node);
        ruby_prism::visit_def_node(self, node);
    }
}

crate::register_cop!("Lint/MissingSuper", |cfg| {
    let allowed = cfg
        .get_cop_config("Lint/MissingSuper")
        .and_then(|c| c.raw.get("AllowedParentClasses"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(Box::new(MissingSuper::new(allowed)))
});
