//! Lint/ConstantDefinitionInBlock cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct ConstantDefinitionInBlock {
    allowed_methods: Vec<String>,
}

impl ConstantDefinitionInBlock {
    pub fn new(allowed_methods: Vec<String>) -> Self {
        Self { allowed_methods }
    }
}

impl Default for ConstantDefinitionInBlock {
    fn default() -> Self {
        Self::new(vec!["enums".to_string()])
    }
}

impl Cop for ConstantDefinitionInBlock {
    fn name(&self) -> &'static str {
        "Lint/ConstantDefinitionInBlock"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ConstDefVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            block_method_stack: Vec::new(),
            pending_block_method: None,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct ConstDefVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a ConstantDefinitionInBlock,
    offenses: Vec<Offense>,
    /// Each entry: Some(method_name) or None for non-call blocks
    block_method_stack: Vec<Option<String>>,
    /// Set by visit_call_node before visiting block child
    pending_block_method: Option<String>,
}

impl<'a> ConstDefVisitor<'a> {
    fn in_block(&self) -> bool {
        !self.block_method_stack.is_empty()
    }

    fn is_allowed(&self) -> bool {
        // Check if the innermost block's method name is in the allowed list.
        if let Some(top) = self.block_method_stack.last() {
            if let Some(method) = top {
                return self.cop.allowed_methods.iter().any(|m| m == method);
            }
        }
        false
    }
}

impl<'a> Visit<'_> for ConstDefVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // When this call has a block, note its method name for the block visit.
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let prev_pending = self.pending_block_method.take();
        self.pending_block_method = Some(method_name);
        ruby_prism::visit_call_node(self, node);
        self.pending_block_method = prev_pending;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // Take the pending method name set by the enclosing call visit.
        let method_name = self.pending_block_method.take();
        self.block_method_stack.push(method_name);
        ruby_prism::visit_block_node(self, node);
        self.block_method_stack.pop();
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        // `FOO = ...` inside a block
        if self.in_block() && !self.is_allowed() {
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ConstantDefinitionInBlock",
                "Do not define constants this way within a block.",
                Severity::Warning,
                start,
                end,
            ));
        }
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode) {
        // `self::FOO = ...` or `::FOO = ...` — explicitly scoped, allowed.
        ruby_prism::visit_constant_path_write_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if self.in_block() && !self.is_allowed() {
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ConstantDefinitionInBlock",
                "Do not define constants this way within a block.",
                Severity::Warning,
                start,
                end,
            ));
        }
        // Inside a class body, we're no longer inside a block context — save and clear.
        let saved = std::mem::take(&mut self.block_method_stack);
        let saved_pending = self.pending_block_method.take();
        ruby_prism::visit_class_node(self, node);
        self.block_method_stack = saved;
        self.pending_block_method = saved_pending;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if self.in_block() && !self.is_allowed() {
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ConstantDefinitionInBlock",
                "Do not define constants this way within a block.",
                Severity::Warning,
                start,
                end,
            ));
        }
        let saved = std::mem::take(&mut self.block_method_stack);
        let saved_pending = self.pending_block_method.take();
        ruby_prism::visit_module_node(self, node);
        self.block_method_stack = saved;
        self.pending_block_method = saved_pending;
    }
}

crate::register_cop!("Lint/ConstantDefinitionInBlock", |cfg| {
    let allowed_methods = cfg
        .get_cop_config("Lint/ConstantDefinitionInBlock")
        .and_then(|c| c.raw.get("AllowedMethods"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(|| vec!["enums".to_string()]);
    Some(Box::new(ConstantDefinitionInBlock::new(allowed_methods)))
});
