//! Style/TopLevelMethodDefinition cop
//!
//! Flags method definitions at top level (not inside class/module/block).

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Do not define methods at the top-level.";

#[derive(Default)]
pub struct TopLevelMethodDefinition;

impl TopLevelMethodDefinition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for TopLevelMethodDefinition {
    fn name(&self) -> &'static str {
        "Style/TopLevelMethodDefinition"
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = TopVisitor {
            ctx,
            depth: 0,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct TopVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    /// Number of enclosing class/module/block/sclass scopes. 0 = top-level.
    depth: u32,
    offenses: Vec<Offense>,
}

impl<'a> TopVisitor<'a> {
    fn at_top_level(&self) -> bool {
        self.depth == 0
    }

    fn add(&mut self, start: usize, end: usize) {
        self.offenses.push(self.ctx.offense_with_range(
            "Style/TopLevelMethodDefinition",
            MSG,
            Severity::Convention,
            start,
            end,
        ));
    }
}

impl<'a> Visit<'_> for TopVisitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if self.at_top_level() {
            let loc = node.location();
            self.add(loc.start_offset(), loc.end_offset());
        }
        self.depth += 1;
        ruby_prism::visit_def_node(self, node);
        self.depth -= 1;
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

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        self.depth += 1;
        ruby_prism::visit_singleton_class_node(self, node);
        self.depth -= 1;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.depth += 1;
        ruby_prism::visit_block_node(self, node);
        self.depth -= 1;
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        self.depth += 1;
        ruby_prism::visit_lambda_node(self, node);
        self.depth -= 1;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Top-level `define_method(...)` (with or without block) is also flagged.
        // RuboCop: RESTRICT_ON_SEND = [:define_method] for `on_send`, and `on_block`
        // for `define_method` blocks. Both flag the *call* (or block) at top level.
        if self.at_top_level() && node_name!(node) == "define_method" {
            // If a block, the offense range is from call start to block end.
            let start = node.location().start_offset();
            let end = if let Some(block) = node.block() {
                block.location().end_offset()
            } else {
                node.location().end_offset()
            };
            self.add(start, end);
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/TopLevelMethodDefinition", |_cfg| Some(Box::new(
    TopLevelMethodDefinition::new()
)));
