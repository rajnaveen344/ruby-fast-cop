//! Lint/UnusedMethodArgument - Checks for unused method arguments.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/unused_method_argument.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::variable_force::{
    Scope, Variable, VariableForceDispatcher, VariableForceHook,
};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct UnusedMethodArgument {
    allow_unused_keyword_arguments: bool,
    ignore_empty_methods: bool,
    ignore_not_implemented_methods: bool,
    not_implemented_exceptions: Vec<String>,
}

impl UnusedMethodArgument {
    pub fn new() -> Self {
        Self {
            allow_unused_keyword_arguments: false,
            ignore_empty_methods: true,
            ignore_not_implemented_methods: true,
            not_implemented_exceptions: vec!["NotImplementedError".to_string()],
        }
    }

    pub fn with_config(
        allow_unused_keyword_arguments: bool,
        ignore_empty_methods: bool,
        ignore_not_implemented_methods: bool,
        not_implemented_exceptions: Vec<String>,
    ) -> Self {
        Self {
            allow_unused_keyword_arguments,
            ignore_empty_methods,
            ignore_not_implemented_methods,
            not_implemented_exceptions,
        }
    }
}

impl Cop for UnusedMethodArgument {
    fn name(&self) -> &'static str {
        "Lint/UnusedMethodArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut hook = UnusedMethodArgumentHook {
            ctx,
            offenses: Vec::new(),
            allow_unused_keyword_arguments: self.allow_unused_keyword_arguments,
            ignore_empty_methods: self.ignore_empty_methods,
            ignore_not_implemented_methods: self.ignore_not_implemented_methods,
            not_implemented_exceptions: &self.not_implemented_exceptions,
        };
        let mut dispatcher = VariableForceDispatcher::new(&mut hook, ctx.source);
        dispatcher.investigate(node);
        hook.offenses
    }
}

struct UnusedMethodArgumentHook<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    allow_unused_keyword_arguments: bool,
    ignore_empty_methods: bool,
    ignore_not_implemented_methods: bool,
    not_implemented_exceptions: &'a [String],
}

impl<'a> UnusedMethodArgumentHook<'a> {
    fn check_argument(&mut self, variable: &Variable, scope: &Scope) {
        // Only check method arguments
        if !variable.is_method_argument {
            return;
        }

        // Skip _ prefixed
        if variable.should_be_unused() {
            return;
        }

        // Skip unused keyword arguments if configured
        if variable.is_keyword_argument && self.allow_unused_keyword_arguments {
            return;
        }

        // Skip if referenced
        if variable.referenced() || variable.captured_by_block {
            return;
        }

        // Check ignored method patterns
        if self.is_ignored_method(scope) {
            return;
        }

        // Check if block argument (&block) with yield in method body
        if variable.is_block_arg_type && self.method_has_yield(scope) {
            return;
        }

        // Check for implicit references (super without args, binding without args)
        if self.has_implicit_reference(scope) {
            return;
        }

        let message = self.build_message(variable, scope);

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/UnusedMethodArgument",
            &message,
            Severity::Warning,
            variable.declaration_start,
            variable.declaration_end,
        ));
    }

    fn is_ignored_method(&self, scope: &Scope) -> bool {
        if self.ignore_empty_methods && scope.body_is_empty {
            return true;
        }

        if self.ignore_not_implemented_methods {
            return self.is_not_implemented_method(scope);
        }

        false
    }

    fn is_not_implemented_method(&self, scope: &Scope) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut checker = NotImplementedChecker {
            scope_start: scope.node_offset,
            scope_end: scope.node_end_offset,
            not_implemented_exceptions: self.not_implemented_exceptions,
            found: false,
        };
        for stmt in program.statements().body().iter() {
            checker.visit(&stmt);
        }
        checker.found
    }

    fn method_has_yield(&self, scope: &Scope) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut finder = YieldFinder {
            scope_start: scope.node_offset,
            scope_end: scope.node_end_offset,
            found: false,
            depth: 0,
        };
        for stmt in program.statements().body().iter() {
            finder.visit(&stmt);
        }
        finder.found
    }

    fn has_implicit_reference(&self, scope: &Scope) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut finder = ImplicitRefFinder {
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
        let mut message = format!("Unused method argument - `{}`.", variable.name);

        // Keyword arguments don't get the underscore prefix suggestion
        if !variable.is_keyword_argument {
            message.push_str(&format!(
                " If it's necessary, use `_` or `_{}` as an argument name to indicate that it won't be used. If it's unnecessary, remove it.",
                variable.name
            ));
        }

        // If all arguments are unused, suggest method(*)
        let all_unused = scope
            .variables
            .values()
            .filter(|v| v.is_method_argument)
            .all(|v| !v.referenced() && !v.captured_by_block);

        if all_unused {
            if let Some(ref method_name) = scope.name {
                message.push_str(&format!(
                    " You can also write as `{}(*)` if you want the method to accept any arguments but don't care about them.",
                    method_name
                ));
            }
        }

        message
    }
}

impl<'a> VariableForceHook for UnusedMethodArgumentHook<'a> {
    fn after_leaving_scope(&mut self, scope: &Scope, _source: &str) {
        let variables: Vec<_> = scope.variables.values().collect();
        for variable in variables {
            self.check_argument(variable, scope);
        }
    }
}

/// Checks if a method body is just `raise NotImplementedError` or `fail`.
struct NotImplementedChecker<'a> {
    scope_start: usize,
    scope_end: usize,
    not_implemented_exceptions: &'a [String],
    found: bool,
}

impl<'a> Visit<'_> for NotImplementedChecker<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let start = node.location().start_offset();
        if start != self.scope_start {
            return;
        }

        if let Some(body) = node.body() {
            // Body should be a single statement
            if let ruby_prism::Node::StatementsNode { .. } = &body {
                let stmts = body.as_statements_node().unwrap();
                let body_stmts: Vec<_> = stmts.body().iter().collect();
                if body_stmts.len() == 1 {
                    self.check_not_implemented(&body_stmts[0]);
                }
            }
        }
    }
}

impl<'a> NotImplementedChecker<'a> {
    fn check_not_implemented(&mut self, node: &ruby_prism::Node) {
        if let ruby_prism::Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            let name = node_name!(call).to_string();

            if call.receiver().is_none() {
                if name == "raise" {
                    // Check if first arg is an allowed exception class
                    if let Some(args) = call.arguments() {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if !arg_list.is_empty() {
                            if self.is_allowed_exception(&arg_list[0]) {
                                self.found = true;
                            }
                        }
                    }
                } else if name == "fail" {
                    // `fail` with or without message is always accepted
                    self.found = true;
                }
            }
        }
    }

    fn is_allowed_exception(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let c = node.as_constant_read_node().unwrap();
                let name = node_name!(c).to_string();
                self.not_implemented_exceptions.contains(&name)
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                // Handle ::NotImplementedError or Library::AbstractMethodError
                let full_name = self.constant_path_name(node);
                // Check against full name and also without leading ::
                let stripped = full_name.trim_start_matches("::");
                self.not_implemented_exceptions.contains(&full_name)
                    || self.not_implemented_exceptions.contains(&stripped.to_string())
            }
            _ => false,
        }
    }

    fn constant_path_name(&self, node: &ruby_prism::Node) -> String {
        match node {
            ruby_prism::Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                let child_name =
                    String::from_utf8_lossy(cp.name().unwrap().as_slice()).to_string();
                if let Some(parent) = cp.parent() {
                    format!("{}::{}", self.constant_path_name(&parent), child_name)
                } else {
                    format!("::{}", child_name)
                }
            }
            ruby_prism::Node::ConstantReadNode { .. } => {
                let c = node.as_constant_read_node().unwrap();
                node_name!(c).to_string()
            }
            _ => String::new(),
        }
    }
}

/// Finds yield nodes within a method scope.
struct YieldFinder {
    scope_start: usize,
    scope_end: usize,
    found: bool,
    depth: usize,
}

impl Visit<'_> for YieldFinder {
    fn visit_yield_node(&mut self, node: &ruby_prism::YieldNode) {
        let offset = node.location().start_offset();
        if self.depth == 0 && offset >= self.scope_start && offset < self.scope_end {
            self.found = true;
        }
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if node.location().start_offset() == self.scope_start {
            ruby_prism::visit_def_node(self, node);
        }
        // Don't descend into nested defs
    }
}

/// Finds implicit references (bare super, bare binding) in method scope.
struct ImplicitRefFinder {
    scope_start: usize,
    scope_end: usize,
    found: bool,
    scope_depth: usize,
}

impl Visit<'_> for ImplicitRefFinder {
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        let offset = node.location().start_offset();
        if self.scope_depth == 0 && offset >= self.scope_start && offset < self.scope_end {
            self.found = true;
        }
    }

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

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if node.location().start_offset() == self.scope_start {
            ruby_prism::visit_def_node(self, node);
        } else {
            self.scope_depth += 1;
            ruby_prism::visit_def_node(self, node);
            self.scope_depth -= 1;
        }
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.scope_depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.scope_depth -= 1;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.scope_depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.scope_depth -= 1;
    }
}
