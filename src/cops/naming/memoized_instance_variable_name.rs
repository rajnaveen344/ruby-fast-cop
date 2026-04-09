//! Naming/MemoizedInstanceVariableName cop
//!
//! Checks that memoized methods use instance variable names that match the method name.
//! Supports both `@ivar ||= ...` and `return @ivar if defined?(@ivar); @ivar = ...` patterns.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const INITIALIZE_METHODS: &[&str] = &["initialize", "initialize_clone", "initialize_copy", "initialize_dup"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LeadingUnderscoreStyle {
    Disallowed,
    Required,
    Optional,
}

pub struct MemoizedInstanceVariableName {
    style: LeadingUnderscoreStyle,
}

impl MemoizedInstanceVariableName {
    pub fn new() -> Self {
        Self { style: LeadingUnderscoreStyle::Disallowed }
    }

    pub fn with_style(style: LeadingUnderscoreStyle) -> Self {
        Self { style }
    }
}

impl Default for MemoizedInstanceVariableName {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip !?= from method name for matching
fn strip_special(method_name: &str) -> String {
    method_name.chars().filter(|c| *c != '!' && *c != '?' && *c != '=').collect()
}

/// Generate candidates that are acceptable variable names for a given method name
fn variable_name_candidates(style: LeadingUnderscoreStyle, method_name: &str) -> Vec<String> {
    let no_underscore = method_name.strip_prefix('_').unwrap_or(method_name).to_string();
    let with_underscore = format!("_{}", method_name);
    match style {
        LeadingUnderscoreStyle::Required => {
            let mut v = vec![with_underscore];
            if method_name.starts_with('_') {
                v.push(method_name.to_string());
            }
            v
        }
        LeadingUnderscoreStyle::Disallowed => {
            vec![method_name.to_string(), no_underscore]
        }
        LeadingUnderscoreStyle::Optional => {
            vec![method_name.to_string(), with_underscore, no_underscore]
        }
    }
}

/// Check if a variable name matches for the given method name
fn matches(style: LeadingUnderscoreStyle, method_name: &str, ivar_name: &str) -> bool {
    if INITIALIZE_METHODS.contains(&method_name) {
        return true;
    }
    let clean_method = strip_special(method_name);
    // ivar_name is like "@foo", strip the @
    let var_name = ivar_name.strip_prefix('@').unwrap_or(ivar_name);
    variable_name_candidates(style, &clean_method).contains(&var_name.to_string())
}

/// Generate the suggested variable name
fn suggested_var(style: LeadingUnderscoreStyle, method_name: &str) -> String {
    let suggestion = strip_special(method_name);
    match style {
        LeadingUnderscoreStyle::Required => format!("_{}", suggestion),
        _ => suggestion,
    }
}

/// Generate the message
fn make_message(style: LeadingUnderscoreStyle, ivar_name: &str, method_name: &str, suggested_var: &str) -> String {
    let var_without_at = ivar_name.strip_prefix('@').unwrap_or(ivar_name);
    if style == LeadingUnderscoreStyle::Required && !var_without_at.starts_with('_') {
        format!(
            "Memoized variable `{}` does not start with `_`. Use `@{}` instead.",
            ivar_name, suggested_var
        )
    } else {
        format!(
            "Memoized variable `{}` does not match method name `{}`. Use `@{}` instead.",
            ivar_name, method_name, suggested_var
        )
    }
}

/// Context info for the enclosing method
struct MethodContext {
    name: String,
    /// Byte offset of the last statement in the method body (for memoization position check)
    last_stmt_offset: Option<usize>,
}

struct MemoizedVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: LeadingUnderscoreStyle,
    offenses: Vec<Offense>,
    method_stack: Vec<MethodContext>,
}

impl<'a> MemoizedVisitor<'a> {
    fn new(ctx: &'a CheckContext<'a>, style: LeadingUnderscoreStyle) -> Self {
        Self {
            ctx,
            style,
            offenses: Vec::new(),
            method_stack: Vec::new(),
        }
    }

    fn current_method(&self) -> Option<&MethodContext> {
        self.method_stack.last()
    }

    /// Get the last statement offset from a body node
    fn last_stmt_offset(body: &Node) -> Option<usize> {
        if let Some(stmts) = body.as_statements_node() {
            let items: Vec<_> = stmts.body().iter().collect();
            items.last().map(|n| n.location().start_offset())
        } else {
            Some(body.location().start_offset())
        }
    }

    /// Check an `@ivar ||= expr` pattern
    fn check_or_asgn(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        let method_ctx = match self.current_method() {
            Some(c) => c,
            None => return,
        };

        // The ||= must be the last statement in the body (memoization position)
        let node_offset = node.location().start_offset();
        if let Some(last_offset) = method_ctx.last_stmt_offset {
            if node_offset != last_offset {
                return;
            }
        }

        let method_name = method_ctx.name.clone();
        // name() returns the name WITH @ prefix, e.g. "@my_var"
        let ivar_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        if matches(self.style, &method_name, &ivar_name) {
            return;
        }

        let suggested = suggested_var(self.style, &method_name);
        let msg = make_message(self.style, &ivar_name, &method_name, &suggested);

        let name_loc = node.name_loc();
        self.offenses.push(self.ctx.offense_with_range(
            "Naming/MemoizedInstanceVariableName",
            &msg,
            Severity::Convention,
            name_loc.start_offset(),
            name_loc.end_offset(),
        ));
    }

    /// Check `return @ivar if defined?(@ivar); @ivar = value` pattern in method body
    fn check_defined_memoization(&mut self, stmts: &ruby_prism::StatementsNode, method_name: &str) {
        let items: Vec<_> = stmts.body().iter().collect();

        for (i, stmt) in items.iter().enumerate() {
            if let Some(if_node) = stmt.as_if_node() {
                if let Some((ivar_name, defined_ivar_loc, return_ivar_loc)) = self.extract_defined_pattern(&if_node) {
                    // Look for @ivar = ... assignment after this if
                    for j in (i + 1)..items.len() {
                        if let Some(ivar_write) = items[j].as_instance_variable_write_node() {
                            let write_name = String::from_utf8_lossy(ivar_write.name().as_slice()).to_string();
                            if write_name == ivar_name {
                                // Code after assignment => not a memoization pattern
                                if j != items.len() - 1 {
                                    return;
                                }

                                if matches(self.style, method_name, &ivar_name) {
                                    return;
                                }

                                let suggested = suggested_var(self.style, method_name);
                                let msg = make_message(self.style, &ivar_name, method_name, &suggested);

                                // Report 3 offenses: return @ivar, defined?(@ivar), @ivar =
                                self.offenses.push(self.ctx.offense_with_range(
                                    "Naming/MemoizedInstanceVariableName",
                                    &msg,
                                    Severity::Convention,
                                    return_ivar_loc.0,
                                    return_ivar_loc.1,
                                ));
                                self.offenses.push(self.ctx.offense_with_range(
                                    "Naming/MemoizedInstanceVariableName",
                                    &msg,
                                    Severity::Convention,
                                    defined_ivar_loc.0,
                                    defined_ivar_loc.1,
                                ));

                                let write_name_loc = ivar_write.name_loc();
                                self.offenses.push(self.ctx.offense_with_range(
                                    "Naming/MemoizedInstanceVariableName",
                                    &msg,
                                    Severity::Convention,
                                    write_name_loc.start_offset(),
                                    write_name_loc.end_offset(),
                                ));

                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Extract `return @ivar if defined?(@ivar)` pattern from an if node
    /// Returns (ivar_name, defined_ivar_location, return_ivar_location)
    fn extract_defined_pattern(&self, if_node: &ruby_prism::IfNode) -> Option<(String, (usize, usize), (usize, usize))> {
        // The predicate should be a DefinedNode with an ivar argument
        let predicate = if_node.predicate();
        let defined_node = predicate.as_defined_node()?;
        let defined_arg = defined_node.value();
        let ivar_read = defined_arg.as_instance_variable_read_node()?;
        let ivar_name = String::from_utf8_lossy(ivar_read.name().as_slice()).to_string();

        let defined_ivar_loc = (ivar_read.location().start_offset(), ivar_read.location().end_offset());

        // The then-branch should be a return with the same ivar
        let then_stmts = if_node.statements()?;
        let then_body: Vec<_> = then_stmts.body().iter().collect();
        if then_body.len() != 1 {
            return None;
        }
        let return_node = then_body[0].as_return_node()?;
        let return_args = return_node.arguments()?;
        let return_args_list: Vec<_> = return_args.arguments().iter().collect();
        if return_args_list.len() != 1 {
            return None;
        }
        let return_ivar = return_args_list[0].as_instance_variable_read_node()?;
        let return_ivar_name = String::from_utf8_lossy(return_ivar.name().as_slice()).to_string();
        if return_ivar_name != ivar_name {
            return None;
        }
        let return_ivar_loc = (return_ivar.location().start_offset(), return_ivar.location().end_offset());

        // Must have no else branch
        if if_node.subsequent().is_some() {
            return None;
        }

        Some((ivar_name, defined_ivar_loc, return_ivar_loc))
    }

    /// Extract method name from a call to define_method/define_singleton_method
    fn extract_dynamic_method_name(&self, call: &ruby_prism::CallNode) -> Option<String> {
        let method = String::from_utf8_lossy(call.name().as_slice());
        if method != "define_method" && method != "define_singleton_method" {
            return None;
        }
        let args = call.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return None;
        }
        if let Some(sym) = arg_list[0].as_symbol_node() {
            return Some(String::from_utf8_lossy(sym.unescaped().as_ref()).to_string());
        }
        if let Some(s) = arg_list[0].as_string_node() {
            return Some(String::from_utf8_lossy(s.unescaped().as_ref()).to_string());
        }
        None
    }

    fn enter_method(&mut self, name: String, body: Option<Node>) {
        let last_offset = body.as_ref().and_then(|b| Self::last_stmt_offset(b));
        self.method_stack.push(MethodContext { name: name.clone(), last_stmt_offset: last_offset });

        // Check defined? pattern
        if let Some(b) = &body {
            if let Some(stmts) = b.as_statements_node() {
                self.check_defined_memoization(&stmts, &name);
            }
        }
    }
}

impl Visit<'_> for MemoizedVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        self.enter_method(name, node.body());
        ruby_prism::visit_def_node(self, node);
        self.method_stack.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check for define_method(:name) do ... end with a block
        if let Some(block) = node.block() {
            if let Some(block_node) = block.as_block_node() {
                if let Some(name) = self.extract_dynamic_method_name(node) {
                    self.enter_method(name, block_node.body());
                    ruby_prism::visit_block_node(self, &block_node);
                    self.method_stack.pop();
                    return;
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        self.check_or_asgn(node);
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }
}

impl Cop for MemoizedInstanceVariableName {
    fn name(&self) -> &'static str {
        "Naming/MemoizedInstanceVariableName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MemoizedVisitor::new(ctx, self.style);
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
