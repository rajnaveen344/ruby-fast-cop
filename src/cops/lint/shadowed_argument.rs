//! Lint/ShadowedArgument - Checks for shadowed arguments.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/shadowed_argument.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::variable_force::{
    AssignmentKind, Scope, Variable, VariableForceDispatcher, VariableForceHook,
};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct ShadowedArgument {
    ignore_implicit_references: bool,
}

impl ShadowedArgument {
    pub fn new() -> Self {
        Self {
            ignore_implicit_references: false,
        }
    }

    pub fn with_config(ignore_implicit_references: bool) -> Self {
        Self {
            ignore_implicit_references,
        }
    }
}

impl Cop for ShadowedArgument {
    fn name(&self) -> &'static str {
        "Lint/ShadowedArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut hook = ShadowedArgumentHook {
            ctx,
            offenses: Vec::new(),
            ignore_implicit_references: self.ignore_implicit_references,
        };
        let mut dispatcher = VariableForceDispatcher::new(&mut hook, ctx.source);
        dispatcher.investigate(node);
        hook.offenses
    }
}

struct ShadowedArgumentHook<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    ignore_implicit_references: bool,
}

impl<'a> ShadowedArgumentHook<'a> {
    fn check_argument(&mut self, variable: &Variable, scope: &Scope) {
        // Only check method and block arguments
        if !variable.is_method_argument && !variable.is_block_argument() {
            return;
        }
        // Skip block local variables
        if variable.is_block_local_variable {
            return;
        }

        // Must have been referenced to be shadowed
        if !variable.referenced() {
            return;
        }

        // Find the shadowing assignment using RuboCop's algorithm:
        // Walk assignments in order, find first unconditional non-shorthand one
        // that doesn't use the argument in its RHS.
        let (shadow_offset, location_known) = match self.find_shadowing_assignment(variable, scope) {
            Some(result) => result,
            None => return, // No shadowing found
        };

        // Check if argument was referenced before the shadowing point
        let check_offset = shadow_offset;
        if self.argument_referenced_before(&variable.name, scope, check_offset) {
            return;
        }

        // Check implicit references (super without args, binding without args)
        // When IgnoreImplicitReferences is true, implicit refs PREVENT shadowing
        if self.ignore_implicit_references {
            let check_super = variable.is_method_argument;
            if self.has_implicit_reference_in_scope(scope, check_super) {
                return;
            }
        }

        // For block arguments: super in a block doesn't implicitly pass block args,
        // but binding does. When IgnoreImplicitReferences is false (default), we
        // don't check implicit refs at all (they don't prevent shadowing).
        // When true, only binding prevents shadowing for block args.

        let message = format!(
            "Argument `{}` was shadowed by a local variable before it was used.",
            variable.name
        );

        let (start, end) = if location_known {
            self.find_assignment_range(shadow_offset)
        } else {
            (variable.declaration_start, variable.declaration_end)
        };

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/ShadowedArgument",
            &message,
            Severity::Warning,
            start,
            end,
        ));
    }

    /// Find the first shadowing assignment, following RuboCop's reduce algorithm.
    /// Returns (assignment_name_start, location_known) or None if no shadow found.
    fn find_shadowing_assignment(
        &self,
        variable: &Variable,
        scope: &Scope,
    ) -> Option<(usize, bool)> {
        let mut location_known = true;

        for assignment in &variable.assignments {
            // Shorthand assignments (||=, &&=) always use the argument
            if matches!(
                assignment.kind,
                AssignmentKind::OrAssignment | AssignmentKind::AndAssignment
            ) {
                location_known = false;
                continue;
            }

            // Check if this assignment is inside a conditional context
            // (if/case/rescue/block relative to the scope)
            let is_conditional = self.is_conditional_assignment(
                assignment.node_offset,
                scope.node_offset,
            );

            // Check if the assignment uses the argument in its RHS
            let uses_arg = self.assignment_uses_variable(
                assignment.node_offset,
                &variable.name,
            );

            if !uses_arg {
                if is_conditional {
                    // Conditional shadow - can't determine exact location
                    location_known = false;
                    continue;
                }

                // Unconditional shadow found!
                return Some((assignment.name_start, location_known));
            }

            // Assignment uses the argument - location_known stays as-is
        }

        // No unconditional shadow found
        None
    }

    /// Check if an assignment at `node_offset` is inside a conditional context
    /// (if/case/rescue/block) between it and the scope at `scope_offset`.
    fn is_conditional_assignment(&self, node_offset: usize, scope_offset: usize) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut checker = ConditionalChecker {
            target_offset: node_offset,
            scope_offset,
            found_conditional: false,
        };
        for stmt in program.statements().body().iter() {
            checker.visit(&stmt);
        }
        checker.found_conditional
    }

    /// Check if the assignment node's RHS references the given variable name.
    fn assignment_uses_variable(&self, assignment_offset: usize, var_name: &str) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut checker = AssignmentRhsChecker {
            assignment_offset,
            var_name,
            found: false,
        };
        for stmt in program.statements().body().iter() {
            checker.visit(&stmt);
        }
        checker.found
    }

    /// Check if the argument was explicitly referenced before the given byte offset.
    fn argument_referenced_before(
        &self,
        var_name: &str,
        scope: &Scope,
        before_offset: usize,
    ) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut finder = RefBeforeOffsetFinder {
            var_name,
            before_offset,
            scope_start: scope.node_offset,
            scope_end: scope.node_end_offset,
            found: false,
        };
        for stmt in program.statements().body().iter() {
            finder.visit(&stmt);
        }
        finder.found
    }

    /// Check if scope has implicit references (super without args, binding without args).
    fn has_implicit_reference_in_scope(&self, scope: &Scope, check_super: bool) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut finder = ImplicitRefFinder {
            scope_start: scope.node_offset,
            scope_end: scope.node_end_offset,
            check_super,
            found: false,
            nested_scope_depth: 0,
        };
        for stmt in program.statements().body().iter() {
            finder.visit(&stmt);
        }
        finder.found
    }

    /// Find the full assignment expression range (e.g., `foo = 42`).
    fn find_assignment_range(&self, name_start: usize) -> (usize, usize) {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut finder = AssignmentRangeFinder {
            target_offset: name_start,
            range: None,
        };
        for stmt in program.statements().body().iter() {
            finder.visit(&stmt);
        }
        finder.range.unwrap_or((name_start, name_start + 1))
    }
}

impl<'a> VariableForceHook for ShadowedArgumentHook<'a> {
    fn after_leaving_scope(&mut self, scope: &Scope, _source: &str) {
        let variables: Vec<_> = scope.variables.values().collect();
        for variable in variables {
            self.check_argument(variable, scope);
        }
    }
}

/// Checks if an assignment at target_offset is inside a conditional/block/rescue
/// between itself and the scope node at scope_offset.
struct ConditionalChecker {
    target_offset: usize,
    scope_offset: usize,
    found_conditional: bool,
}

impl ConditionalChecker {
    fn contains_target(&self, node: &ruby_prism::Node) -> bool {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.target_offset >= start && self.target_offset < end
    }

    fn is_scope_node(&self, node: &ruby_prism::Node) -> bool {
        node.location().start_offset() == self.scope_offset
    }
}

impl Visit<'_> for ConditionalChecker {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if !self.is_scope_node(&node.as_node()) && self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_if_node(self, node);
    }
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if !self.is_scope_node(&node.as_node()) && self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_unless_node(self, node);
    }
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_case_node(self, node);
    }
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        if self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_case_match_node(self, node);
    }
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        // begin/rescue is conditional
        if node.rescue_clause().is_some() && self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_begin_node(self, node);
    }
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        if self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_rescue_node(self, node);
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if !self.is_scope_node(&node.as_node()) && self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_block_node(self, node);
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        if !self.is_scope_node(&node.as_node()) && self.contains_target(&node.as_node()) {
            self.found_conditional = true;
        }
        ruby_prism::visit_lambda_node(self, node);
    }
}

/// Checks if an assignment at `assignment_offset` uses `var_name` in its RHS.
struct AssignmentRhsChecker<'a> {
    assignment_offset: usize,
    var_name: &'a str,
    found: bool,
}

impl<'a> Visit<'_> for AssignmentRhsChecker<'a> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        if node.location().start_offset() == self.assignment_offset {
            let mut var_finder = VarRefFinder {
                name: self.var_name,
                found: false,
            };
            var_finder.visit(&node.value());
            if var_finder.found {
                self.found = true;
            }
            return;
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        if node.location().start_offset() == self.assignment_offset {
            let mut var_finder = VarRefFinder {
                name: self.var_name,
                found: false,
            };
            var_finder.visit(&node.value());
            if var_finder.found {
                self.found = true;
            }
            return;
        }
        ruby_prism::visit_multi_write_node(self, node);
    }
}

struct VarRefFinder<'a> {
    name: &'a str,
    found: bool,
}

impl<'a> Visit<'_> for VarRefFinder<'a> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if name == self.name {
            self.found = true;
        }
    }
}

/// Finds if there's a reference to `var_name` before `before_offset` within the scope.
struct RefBeforeOffsetFinder<'a> {
    var_name: &'a str,
    before_offset: usize,
    scope_start: usize,
    scope_end: usize,
    found: bool,
}

impl<'a> Visit<'_> for RefBeforeOffsetFinder<'a> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let offset = node.location().start_offset();
        if offset >= self.scope_start
            && offset < self.scope_end
            && offset < self.before_offset
        {
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            if name == self.var_name {
                self.found = true;
            }
        }
    }
}

/// Finds implicit references (bare super, bare binding) within a scope.
struct ImplicitRefFinder {
    scope_start: usize,
    scope_end: usize,
    check_super: bool,
    found: bool,
    nested_scope_depth: usize,
}

impl ImplicitRefFinder {
    fn in_scope(&self, node: &ruby_prism::Node) -> bool {
        let offset = node.location().start_offset();
        offset >= self.scope_start && offset < self.scope_end && self.nested_scope_depth == 0
    }
}

impl Visit<'_> for ImplicitRefFinder {
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        if self.check_super && self.in_scope(&node.as_node()) {
            self.found = true;
        }
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.in_scope(&node.as_node()) {
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            if name == "binding" && node.arguments().is_none() && node.receiver().is_none() {
                self.found = true;
                return;
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Don't increment depth for the scope's own def node
        if node.location().start_offset() == self.scope_start {
            ruby_prism::visit_def_node(self, node);
        } else {
            self.nested_scope_depth += 1;
            ruby_prism::visit_def_node(self, node);
            self.nested_scope_depth -= 1;
        }
    }
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.nested_scope_depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.nested_scope_depth -= 1;
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.nested_scope_depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.nested_scope_depth -= 1;
    }
    // Block/lambda nodes are transparent - implicit refs inside blocks still count
    // (e.g., `def foo(bar); something { binding }; ... end` - binding is still in scope)
}

/// Finds the range for a shadowing assignment.
/// For simple writes (foo = 42): returns the full expression range.
/// For multi-writes (*items, last = ...): returns just the variable name range.
struct AssignmentRangeFinder {
    target_offset: usize,
    range: Option<(usize, usize)>,
}

impl Visit<'_> for AssignmentRangeFinder {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        if node.name_loc().start_offset() == self.target_offset {
            self.range = Some((
                node.location().start_offset(),
                node.location().end_offset(),
            ));
            return;
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        // For multi-writes, the offense is on the variable name, not the full expression
        let check_target = |target: &ruby_prism::Node| -> Option<(usize, usize)> {
            if let ruby_prism::Node::LocalVariableTargetNode { .. } = target {
                if target.location().start_offset() == self.target_offset {
                    return Some((target.location().start_offset(), target.location().end_offset()));
                }
            }
            if let ruby_prism::Node::SplatNode { .. } = target {
                let s = target.as_splat_node().unwrap();
                if let Some(expr) = s.expression() {
                    if expr.location().start_offset() == self.target_offset {
                        return Some((expr.location().start_offset(), expr.location().end_offset()));
                    }
                }
            }
            None
        };

        for target in node.lefts().iter() {
            if let Some(range) = check_target(&target) {
                self.range = Some(range);
                return;
            }
        }
        if let Some(rest) = node.rest() {
            if let Some(range) = check_target(&rest) {
                self.range = Some(range);
                return;
            }
        }
        for target in node.rights().iter() {
            if let Some(range) = check_target(&target) {
                self.range = Some(range);
                return;
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }
}
