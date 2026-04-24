//! Lint/ItWithoutArgumentsInBlock
//!
//! Emulates Ruby 3.3 warning: bare `it` inside a block with no explicit
//! parameters will refer to the first block parameter in Ruby 3.4.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Visit};

const MSG: &str = "`it` calls without arguments will refer to the first block param in Ruby 3.4; \
                   use `it()` or `self.it`.";

#[derive(Default)]
pub struct ItWithoutArgumentsInBlock;

impl ItWithoutArgumentsInBlock {
    pub fn new() -> Self { Self }
}

/// Block has no explicit parameters or delimiters (e.g. `foo { ... }` but not
/// `foo { || ... }` or `foo { |x| ... }`).
fn block_args_empty_and_without_delimiters(block: &BlockNode) -> bool {
    match block.parameters() {
        None => true,
        Some(p) => {
            // Prism synthesizes ItParametersNode when `it` appears in a block
            // that has no explicit params — treat as empty.
            if p.as_it_parameters_node().is_some() { return true; }
            if let Some(bp) = p.as_block_parameters_node() {
                return bp.opening_loc().is_none()
                    && bp.parameters().is_none()
                    && bp.locals().iter().count() == 0;
            }
            false
        }
    }
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    block_stack: Vec<bool>, // true = block with empty-no-delimiters params
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for V<'a> {
    fn visit_block_node(&mut self, node: &BlockNode) {
        let empty = block_args_empty_and_without_delimiters(node);
        self.block_stack.push(empty);
        ruby_prism::visit_block_node(self, node);
        self.block_stack.pop();
    }

    fn visit_call_node(&mut self, node: &CallNode) {
        // bare `it`: no receiver, no args, no parens, no block literal
        let name = node.name();
        if name.as_slice() == b"it"
            && node.receiver().is_none()
            && node.arguments().is_none()
            && node.opening_loc().is_none() // no `(`
            && node.block().is_none()
            && self.block_stack.last().copied().unwrap_or(false)
        {
            let loc = node.location();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ItWithoutArgumentsInBlock", MSG, Severity::Warning,
                loc.start_offset(), loc.end_offset(),
            ));
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_it_local_variable_read_node(&mut self, node: &ruby_prism::ItLocalVariableReadNode) {
        // Prism parses `it` as ItLocalVariableReadNode in Ruby 3.4+ grammar, regardless
        // of target version. For targets < 3.4 where `it` would be a plain method call,
        // this node still represents a bare `it` usage in a block.
        if self.block_stack.last().copied().unwrap_or(false) {
            let loc = node.location();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ItWithoutArgumentsInBlock", MSG, Severity::Warning,
                loc.start_offset(), loc.end_offset(),
            ));
        }
    }
}

impl Cop for ItWithoutArgumentsInBlock {
    fn name(&self) -> &'static str { "Lint/ItWithoutArgumentsInBlock" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // `maximum_target_ruby_version 3.3` in RuboCop: skip when >= 3.4.
        if ctx.ruby_version_at_least(3, 4) { return vec![] }
        let mut v = V { ctx, block_stack: Vec::new(), offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Lint/ItWithoutArgumentsInBlock", |_cfg| {
    Some(Box::new(ItWithoutArgumentsInBlock::new()))
});
