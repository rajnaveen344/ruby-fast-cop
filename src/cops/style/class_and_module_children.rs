//! Style/ClassAndModuleChildren cop
//!
//! Checks that namespaced classes and modules are defined with a consistent style.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const NESTED_MSG: &str = "Use nested module/class definitions instead of compact style.";
const COMPACT_MSG: &str = "Use compact module/class definition instead of nested style.";

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Nested,
    Compact,
}

pub struct ClassAndModuleChildren {
    style: EnforcedStyle,
    style_for_classes: Option<EnforcedStyle>,
    style_for_modules: Option<EnforcedStyle>,
}

impl ClassAndModuleChildren {
    pub fn new(
        style: EnforcedStyle,
        style_for_classes: Option<EnforcedStyle>,
        style_for_modules: Option<EnforcedStyle>,
    ) -> Self {
        Self { style, style_for_classes, style_for_modules }
    }

    fn style_for_classes(&self) -> &EnforcedStyle {
        self.style_for_classes.as_ref().unwrap_or(&self.style)
    }

    fn style_for_modules(&self) -> &EnforcedStyle {
        self.style_for_modules.as_ref().unwrap_or(&self.style)
    }
}

impl Default for ClassAndModuleChildren {
    fn default() -> Self {
        Self::new(EnforcedStyle::Nested, None, None)
    }
}

impl Cop for ClassAndModuleChildren {
    fn name(&self) -> &'static str {
        "Style/ClassAndModuleChildren"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ClassAndModuleChildrenVisitor { ctx, cop: self, offenses: Vec::new(), depth: 0, parent_is_compact_eligible: false };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct ClassAndModuleChildrenVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a ClassAndModuleChildren,
    offenses: Vec<Offense>,
    depth: usize, // how many class/module nodes we're inside
    parent_is_compact_eligible: bool, // whether the immediately containing class/module is compactable
}

impl<'a> ClassAndModuleChildrenVisitor<'a> {
    /// Check if constant name contains `::` (compact style)
    fn is_compact_name(&self, node: &Node) -> bool {
        match node {
            Node::ConstantPathNode { .. } => {
                // ConstantPathNode represents Foo::Bar
                true
            }
            _ => false,
        }
    }

    /// Check if this node's name starts with `::` (cbase)
    fn is_cbase_name(&self, node: &Node) -> bool {
        match node {
            Node::ConstantPathNode { .. } => {
                let path = node.as_constant_path_node().unwrap();
                // parent is None means it's relative, parent is ConstantPathNode or similar
                // cbase is when parent is absent (::Foo) - check if parent is a ConstantReadNode absence
                // Actually ::Foo in Prism: parent = None means absolute from root
                // No wait - ::Foo is ConstantPathNode with parent=None
                match path.parent() {
                    None => true, // ::Foo — absolute, cbase
                    Some(_) => false,
                }
            }
            _ => false,
        }
    }

    fn is_inside_class_or_module(&self, _node: &Node, parent: Option<bool>) -> bool {
        parent.unwrap_or(false)
    }

    fn check_nested_style_class(&mut self, node: &ruby_prism::ClassNode, inside_class_or_module: bool) {
        // Must have compact name (Foo::Bar)
        let name = node.constant_path();
        if !self.is_compact_name(&name) {
            return;
        }
        // Skip if cbase (::Foo)
        if self.is_cbase_name(&name) {
            return;
        }
        // Skip if inside another class/module
        if inside_class_or_module {
            return;
        }
        // Offense: on the name range
        let start = name.location().start_offset();
        let end = name.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/ClassAndModuleChildren",
            NESTED_MSG,
            Severity::Convention,
            start,
            end,
        ));
    }

    fn check_nested_style_module(&mut self, node: &ruby_prism::ModuleNode, inside_class_or_module: bool) {
        let name = node.constant_path();
        if !self.is_compact_name(&name) {
            return;
        }
        if self.is_cbase_name(&name) {
            return;
        }
        if inside_class_or_module {
            return;
        }
        let start = name.location().start_offset();
        let end = name.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/ClassAndModuleChildren",
            NESTED_MSG,
            Severity::Convention,
            start,
            end,
        ));
    }

    fn check_compact_style_class(&mut self, node: &ruby_prism::ClassNode) -> bool {
        // Skip if has superclass
        if node.superclass().is_some() {
            return false;
        }
        // Check if body is a single class or module
        let body = match node.body() {
            Some(b) => b,
            None => return false,
        };
        let is_compact_eligible = self.body_is_single_class_or_module(&body);
        // Only flag if parent is NOT compact-eligible (or we're at top level)
        if is_compact_eligible && !self.parent_is_compact_eligible {
            let name = node.constant_path();
            let start = name.location().start_offset();
            let end = name.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Style/ClassAndModuleChildren",
                COMPACT_MSG,
                Severity::Convention,
                start,
                end,
            ));
        }
        is_compact_eligible
    }

    fn check_compact_style_module(&mut self, node: &ruby_prism::ModuleNode) -> bool {
        let body = match node.body() {
            Some(b) => b,
            None => return false,
        };
        let is_compact_eligible = self.body_is_single_class_or_module(&body);
        if is_compact_eligible && !self.parent_is_compact_eligible {
            let name = node.constant_path();
            let start = name.location().start_offset();
            let end = name.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Style/ClassAndModuleChildren",
                COMPACT_MSG,
                Severity::Convention,
                start,
                end,
            ));
        }
        is_compact_eligible
    }

    fn body_is_single_class_or_module(&self, body: &Node) -> bool {
        // Body may be StatementsNode wrapping a single class/module
        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            if children.len() != 1 { return false; }
            matches!(children[0], Node::ClassNode { .. } | Node::ModuleNode { .. })
        } else {
            matches!(body, Node::ClassNode { .. } | Node::ModuleNode { .. })
        }
    }
}

impl<'a> Visit<'_> for ClassAndModuleChildrenVisitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let inside = self.depth > 0;
        let prev_parent_eligible = self.parent_is_compact_eligible;
        let this_eligible = match self.cop.style_for_classes() {
            EnforcedStyle::Nested => {
                self.check_nested_style_class(node, inside);
                false
            }
            EnforcedStyle::Compact => self.check_compact_style_class(node),
        };
        self.depth += 1;
        self.parent_is_compact_eligible = this_eligible;
        ruby_prism::visit_class_node(self, node);
        self.depth -= 1;
        self.parent_is_compact_eligible = prev_parent_eligible;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let inside = self.depth > 0;
        let prev_parent_eligible = self.parent_is_compact_eligible;
        let this_eligible = match self.cop.style_for_modules() {
            EnforcedStyle::Nested => {
                self.check_nested_style_module(node, inside);
                false
            }
            EnforcedStyle::Compact => self.check_compact_style_module(node),
        };
        self.depth += 1;
        self.parent_is_compact_eligible = this_eligible;
        ruby_prism::visit_module_node(self, node);
        self.depth -= 1;
        self.parent_is_compact_eligible = prev_parent_eligible;
    }
}

fn parse_style(s: &str) -> Option<EnforcedStyle> {
    match s {
        "nested" => Some(EnforcedStyle::Nested),
        "compact" => Some(EnforcedStyle::Compact),
        "" => None,
        _ => None,
    }
}

crate::register_cop!("Style/ClassAndModuleChildren", |cfg| {
    use crate::config::Config;
    let style_str = cfg.get_cop_config("Style/ClassAndModuleChildren")
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str())
        .unwrap_or("nested");
    let style = parse_style(style_str).unwrap_or(EnforcedStyle::Nested);

    let style_for_classes_str = cfg.get_cop_config("Style/ClassAndModuleChildren")
        .and_then(|c| c.raw.get("EnforcedStyleForClasses"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let style_for_classes = parse_style(style_for_classes_str);

    let style_for_modules_str = cfg.get_cop_config("Style/ClassAndModuleChildren")
        .and_then(|c| c.raw.get("EnforcedStyleForModules"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let style_for_modules = parse_style(style_for_modules_str);

    Some(Box::new(ClassAndModuleChildren::new(style, style_for_classes, style_for_modules)))
});
