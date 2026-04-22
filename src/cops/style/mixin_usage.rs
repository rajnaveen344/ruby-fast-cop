//! Style/MixinUsage cop
//!
//! Checks that include/extend/prepend statements appear inside classes/modules.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Node, Visit};

const MIXIN_METHODS: &[&str] = &["include", "extend", "prepend"];

#[derive(Default)]
pub struct MixinUsage;

impl MixinUsage {
    pub fn new() -> Self {
        Self
    }
}

struct MixinUsageVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    depth: usize,     // depth in class/module/block scope
    in_args: bool,    // inside method arguments
}

impl MixinUsageVisitor<'_> {
    fn is_mixin_call(node: &CallNode) -> bool {
        if node.receiver().is_some() {
            return false; // has explicit receiver — not a bare mixin call
        }
        let name = node_name!(node);
        MIXIN_METHODS.contains(&name.as_ref())
    }
}

impl<'a> Visit<'_> for MixinUsageVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.depth == 0 && !self.in_args && Self::is_mixin_call(node) {
            let name = node_name!(node);
            let msg = format!(
                "`{}` is used at the top level. Use inside `class` or `module`.",
                name
            );
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Style/MixinUsage",
                &msg,
                Severity::Convention,
                start,
                end,
            ));
        }
        // Visit arguments with in_args=true
        if let Some(args) = node.arguments() {
            let prev = self.in_args;
            self.in_args = true;
            ruby_prism::visit_arguments_node(self, &args);
            self.in_args = prev;
        }
        // Visit other children (receiver, block) without in_args
        if let Some(recv) = node.receiver() {
            self.visit(&recv);
        }
        if let Some(block) = node.block() {
            // block args don't count as method args context for mixin
            let prev = self.in_args;
            self.in_args = false;
            self.visit(&block);
            self.in_args = prev;
        }
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.depth -= 1;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.depth -= 1;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.depth += 1;
        ruby_prism::visit_block_node(self, node);
        self.depth -= 1;
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // def at top level: mixin inside is still top-level (depth stays)
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // if at top level: treat as top-level scope
        ruby_prism::visit_if_node(self, node);
    }
}

impl Cop for MixinUsage {
    fn name(&self) -> &'static str {
        "Style/MixinUsage"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MixinUsageVisitor {
            ctx,
            offenses: vec![],
            depth: 0,
            in_args: false,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Style/MixinUsage", |_cfg| {
    Some(Box::new(MixinUsage::new()))
});
