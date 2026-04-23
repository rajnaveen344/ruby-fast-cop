//! Naming/ClassAndModuleCamelCase - Checks for class and module names with underscore.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/naming/class_and_module_camel_case.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};
use ruby_prism::Visit;

pub struct ClassAndModuleCamelCase {
    allowed_names: Vec<String>,
}

impl ClassAndModuleCamelCase {
    pub fn new(allowed_names: Vec<String>) -> Self {
        Self { allowed_names }
    }

    fn check_name(&self, name: &str, name_start: usize, source: &str, filename: &str) -> Option<Offense> {
        if !name.contains('_') {
            return None;
        }

        // Build pattern from allowed names — strip them from the name before checking
        let mut cleaned = name.to_string();
        for allowed in &self.allowed_names {
            // Remove all occurrences (allowed may appear as namespace component)
            // RuboCop uses gsub(allowed_regex, '')
            cleaned = cleaned.replace(allowed.as_str(), "");
        }

        if !cleaned.contains('_') {
            return None;
        }

        let name_end = name_start + name.len();
        Some(Offense::new(
            "Naming/ClassAndModuleCamelCase",
            "Use CamelCase for classes and modules.",
            Severity::Convention,
            Location::from_offsets(source, name_start, name_end),
            filename,
        ))
    }
}

struct Visitor<'a> {
    cop: &'a ClassAndModuleCamelCase,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'a>) {
        let source = self.ctx.source;
        // constant_path() gives the full name node (e.g. `Top::My_Class` or just `My_Class`)
        let name_node = node.constant_path();
        let name_start = name_node.location().start_offset();
        let name_end = name_node.location().end_offset();
        let name = &source[name_start..name_end];

        if let Some(offense) = self.cop.check_name(name, name_start, source, self.ctx.filename) {
            self.offenses.push(offense);
        }

        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'a>) {
        let source = self.ctx.source;
        let name_node = node.constant_path();
        let name_start = name_node.location().start_offset();
        let name_end = name_node.location().end_offset();
        let name = &source[name_start..name_end];

        if let Some(offense) = self.cop.check_name(name, name_start, source, self.ctx.filename) {
            self.offenses.push(offense);
        }

        ruby_prism::visit_module_node(self, node);
    }
}

impl Cop for ClassAndModuleCamelCase {
    fn name(&self) -> &'static str {
        "Naming/ClassAndModuleCamelCase"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_names: Vec<String>,
}

crate::register_cop!("Naming/ClassAndModuleCamelCase", |cfg| {
    let c: Cfg = cfg.typed("Naming/ClassAndModuleCamelCase");
    Some(Box::new(ClassAndModuleCamelCase::new(c.allowed_names)))
});
