//! Lint/UnusedBlockArgument - Checks for unused block arguments.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/unused_block_argument.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::variable_force::{
    Scope, Variable, VariableForceDispatcher, VariableForceHook,
};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct UnusedBlockArgument {
    allow_unused_keyword_arguments: bool,
    ignore_empty_blocks: bool,
}

impl UnusedBlockArgument {
    pub fn new() -> Self {
        Self {
            allow_unused_keyword_arguments: false,
            ignore_empty_blocks: true,
        }
    }

    pub fn with_config(allow_unused_keyword_arguments: bool, ignore_empty_blocks: bool) -> Self {
        Self {
            allow_unused_keyword_arguments,
            ignore_empty_blocks,
        }
    }
}

impl Cop for UnusedBlockArgument {
    fn name(&self) -> &'static str {
        "Lint/UnusedBlockArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut hook = UnusedBlockArgumentHook {
            ctx,
            offenses: Vec::new(),
            allow_unused_keyword_arguments: self.allow_unused_keyword_arguments,
            ignore_empty_blocks: self.ignore_empty_blocks,
        };
        let mut dispatcher = VariableForceDispatcher::new(&mut hook, ctx.source);
        dispatcher.investigate(node);
        hook.offenses
    }
}

struct UnusedBlockArgumentHook<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    allow_unused_keyword_arguments: bool,
    ignore_empty_blocks: bool,
}

impl<'a> UnusedBlockArgumentHook<'a> {
    fn check_argument(&mut self, variable: &Variable, scope: &Scope) {
        // Skip if _ prefixed
        if variable.should_be_unused() {
            return;
        }

        // Skip if referenced or captured
        if variable.referenced() || variable.captured_by_block {
            return;
        }

        // Block local variables: only flag if no assignments
        if variable.is_block_local_variable {
            if !variable.assignments.is_empty() {
                return; // Used (assigned to)
            }
            // Flag unused block local variable
            let message = format!("Unused block local variable - `{}`.", variable.name);
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/UnusedBlockArgument",
                &message,
                Severity::Warning,
                variable.declaration_start,
                variable.declaration_end,
            ));
            return;
        }

        // Only check block arguments (not method arguments)
        if !variable.is_block_argument() {
            return;
        }

        // Skip empty blocks if configured
        if self.ignore_empty_blocks && scope.body_is_empty {
            return;
        }

        // Skip unused keyword arguments if configured
        if variable.is_keyword_argument && self.allow_unused_keyword_arguments {
            return;
        }

        // Check for implicit references (binding without args in block scope)
        if self.has_implicit_binding_reference(scope) {
            return;
        }

        let message = self.build_message(variable, scope);

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/UnusedBlockArgument",
            &message,
            Severity::Warning,
            variable.declaration_start,
            variable.declaration_end,
        ));
    }

    fn has_implicit_binding_reference(&self, scope: &Scope) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut finder = BindingRefFinder {
            scope_start: scope.node_offset,
            scope_end: scope.node_end_offset,
            found: false,
            scope_depth: 0,
        };
        for stmt in program.statements().body().iter() {
            finder.visit(&stmt);
        }
        finder.found
    }

    fn build_message(&self, variable: &Variable, scope: &Scope) -> String {
        let mut message = format!("Unused block argument - `{}`.", variable.name);

        let all_block_args: Vec<_> = scope
            .variables
            .values()
            .filter(|v| v.is_block_argument() && !v.is_block_local_variable)
            .collect();

        if scope.is_lambda {
            message.push_str(&self.message_for_lambda(variable, &all_block_args));
        } else {
            message.push_str(&self.message_for_normal_block(variable, &all_block_args, scope));
        }

        message
    }

    fn message_for_normal_block(
        &self,
        variable: &Variable,
        all_arguments: &[&Variable],
        scope: &Scope,
    ) -> String {
        let none_referenced = all_arguments
            .iter()
            .all(|v| !v.referenced() && !v.captured_by_block);

        if none_referenced && !self.is_define_method_call(scope) {
            if all_arguments.len() > 1 {
                " You can omit all the arguments if you don't care about them.".to_string()
            } else {
                " You can omit the argument if you don't care about it.".to_string()
            }
        } else {
            format!(
                " If it's necessary, use `_` or `_{}` as an argument name to indicate that it won't be used.",
                variable.name
            )
        }
    }

    fn message_for_lambda(&self, variable: &Variable, all_arguments: &[&Variable]) -> String {
        let underscore_msg = format!(
            " If it's necessary, use `_` or `_{}` as an argument name to indicate that it won't be used.",
            variable.name
        );

        let none_referenced = all_arguments
            .iter()
            .all(|v| !v.referenced() && !v.captured_by_block);

        if none_referenced {
            format!(
                "{} Also consider using a proc without arguments instead of a lambda if you want it to accept any arguments but don't care about them.",
                underscore_msg
            )
        } else {
            underscore_msg
        }
    }

    fn is_define_method_call(&self, scope: &Scope) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut finder = DefineMethodFinder {
            scope_start: scope.node_offset,
            found: false,
        };
        for stmt in program.statements().body().iter() {
            finder.visit(&stmt);
        }
        finder.found
    }
}

impl<'a> VariableForceHook for UnusedBlockArgumentHook<'a> {
    fn after_leaving_scope(&mut self, scope: &Scope, _source: &str) {
        let variables: Vec<_> = scope.variables.values().collect();
        for variable in variables {
            self.check_argument(variable, scope);
        }
    }
}

/// Finds bare `binding` calls within a block scope (not descending into nested scopes).
struct BindingRefFinder {
    scope_start: usize,
    scope_end: usize,
    found: bool,
    scope_depth: usize,
}

impl Visit<'_> for BindingRefFinder {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let offset = node.location().start_offset();
        if self.scope_depth == 0 && offset >= self.scope_start && offset < self.scope_end {
            let name = node_name!(node).to_string();
            if name == "binding" && node.arguments().is_none() && node.receiver().is_none() {
                self.found = true;
                return;
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {
        self.scope_depth += 1;
        // Don't visit - nested def is a different scope
        self.scope_depth -= 1;
    }

    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {
        self.scope_depth += 1;
        self.scope_depth -= 1;
    }

    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {
        self.scope_depth += 1;
        self.scope_depth -= 1;
    }
}

/// Checks if a block's receiver is a `define_method` call.
struct DefineMethodFinder {
    scope_start: usize,
    found: bool,
}

impl Visit<'_> for DefineMethodFinder {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check if this call has a block at our scope_start
        if let Some(block) = node.block() {
            if let ruby_prism::Node::BlockNode { .. } = &block {
                if block.location().start_offset() == self.scope_start {
                    let name = node_name!(node).to_string();
                    if name == "define_method" {
                        self.found = true;
                        return;
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_unused_keyword_arguments: bool,
    ignore_empty_blocks: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Self { allow_unused_keyword_arguments: false, ignore_empty_blocks: true }
    }
}

crate::register_cop!("Lint/UnusedBlockArgument", |cfg| {
    let c: Cfg = cfg.typed("Lint/UnusedBlockArgument");
    Some(Box::new(UnusedBlockArgument::with_config(c.allow_unused_keyword_arguments, c.ignore_empty_blocks)))
});
