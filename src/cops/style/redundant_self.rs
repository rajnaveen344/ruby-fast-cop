//! Style/RedundantSelf cop
//!
//! Checks for redundant uses of `self.` prefix on method calls.
//! Self is needed only for name clashes with local variables/arguments,
//! setter methods, operator methods, keyword method names, CamelCase methods,
//! Kernel methods, `self.it` in blocks without params, and `self.()` implicit calls.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const MSG: &str = "Redundant `self` detected.";

const KEYWORDS: &[&str] = &[
    "alias", "and", "begin", "break", "case", "class", "def", "defined?", "do",
    "else", "elsif", "end", "ensure", "false", "for", "if", "in", "module",
    "next", "nil", "not", "or", "redo", "rescue", "retry", "return", "self",
    "super", "then", "true", "undef", "unless", "until", "when", "while",
    "yield", "__FILE__", "__LINE__", "__ENCODING__",
];

/// Kernel methods (a subset that RuboCop considers)
const KERNEL_METHODS: &[&str] = &[
    "Array", "Complex", "Float", "Hash", "Integer", "Rational", "String",
    "__callee__", "__dir__", "__method__", "abort", "at_exit", "autoload",
    "autoload?", "binding", "block_given?", "callcc", "caller", "caller_locations",
    "catch", "chomp", "chop", "eval", "exec", "exit", "exit!", "fail", "fork",
    "format", "gets", "global_variables", "gsub", "iterator?", "lambda", "load",
    "local_variables", "loop", "open", "p", "pp", "print", "printf", "proc",
    "putc", "puts", "raise", "rand", "readline", "readlines", "require",
    "require_relative", "select", "set_trace_func", "sleep", "spawn", "sprintf",
    "srand", "sub", "syscall", "system", "test", "throw", "trace_var",
    "trap", "untrace_var", "warn",
];

#[derive(Default)]
pub struct RedundantSelf;

impl RedundantSelf {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantSelf {
    fn name(&self) -> &'static str {
        "Style/RedundantSelf"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = RedundantSelfVisitor::new(ctx);
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Scope entry: tracks parameter/variable names from enclosing def/block.
/// These names make `self.name` non-redundant.
struct Scope {
    /// Names from params (always visible in the entire body)
    param_names: HashSet<String>,
}

struct RedundantSelfVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Stack of scopes (def/block). Params in these scopes make `self.x` needed.
    scope_stack: Vec<Scope>,
    /// Temporary additional names that make `self.x` needed (from lvasgn RHS, if-condition, etc.)
    /// These are pushed/popped around specific subtree visits.
    temp_names: Vec<HashSet<String>>,
    /// Track whether we're inside a block with no explicit params (for `self.it` check)
    block_no_params_depth: usize,
}

impl<'a> RedundantSelfVisitor<'a> {
    fn new(ctx: &'a CheckContext<'a>) -> Self {
        Self {
            ctx,
            offenses: Vec::new(),
            scope_stack: vec![Scope { param_names: HashSet::new() }],
            temp_names: Vec::new(),
            block_no_params_depth: 0,
        }
    }

    /// Check if a name clashes with any parameter or temporary variable in scope.
    fn name_clashes(&self, name: &str) -> bool {
        for scope in &self.scope_stack {
            if scope.param_names.contains(name) {
                return true;
            }
        }
        for temps in &self.temp_names {
            if temps.contains(name) {
                return true;
            }
        }
        false
    }

    fn push_scope(&mut self) {
        self.scope_stack.push(Scope { param_names: HashSet::new() });
    }

    fn push_scope_with_params(&mut self, params: HashSet<String>) {
        self.scope_stack.push(Scope { param_names: params });
    }

    fn pop_scope(&mut self) {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
        }
    }

    fn add_param(&mut self, name: &str) {
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.param_names.insert(name.to_string());
        }
    }

    fn push_temp_names(&mut self, names: HashSet<String>) {
        self.temp_names.push(names);
    }

    fn pop_temp_names(&mut self) {
        self.temp_names.pop();
    }

    fn is_self_receiver_call(&self, node: &ruby_prism::CallNode) -> bool {
        if let Some(recv) = node.receiver() {
            matches!(recv, Node::SelfNode { .. })
        } else {
            false
        }
    }

    fn is_operator_method(name: &str) -> bool {
        matches!(
            name,
            "+" | "-" | "*" | "/" | "%" | "**" | "==" | "!=" | ">" | "<" | ">=" | "<="
                | "<=>" | "===" | "=~" | "!~" | "&" | "|" | "^" | "~" | "<<" | ">>"
                | "[]" | "[]=" | "+@" | "-@" | "!" | "!@"
        )
    }

    fn is_setter_method(name: &str) -> bool {
        name.ends_with('=') && !matches!(name, "==" | "!=" | "<=" | ">=" | "===" | "=~" | "!~")
    }

    fn is_camel_case(name: &str) -> bool {
        name.chars().next().map_or(false, |c| c.is_uppercase())
    }

    fn is_keyword(name: &str) -> bool {
        KEYWORDS.contains(&name)
    }

    fn is_kernel_method(name: &str) -> bool {
        KERNEL_METHODS.contains(&name)
    }

    fn is_implicit_call(node: &ruby_prism::CallNode) -> bool {
        // `self.()` — method name is "call" but no explicit method name in source
        let name = node_name!(node);
        name == "call" && node.message_loc().is_none()
    }

    fn is_regular_method_call(node: &ruby_prism::CallNode) -> bool {
        let name = node_name!(node);
        let name_str = name.as_ref();
        !(Self::is_operator_method(name_str)
            || Self::is_keyword(name_str)
            || Self::is_camel_case(name_str)
            || Self::is_setter_method(name_str)
            || Self::is_implicit_call(node))
    }

    fn is_it_method_in_block(&self, node: &ruby_prism::CallNode) -> bool {
        let name = node_name!(node);
        if name != "it" {
            return false;
        }
        if self.block_no_params_depth == 0 {
            return false;
        }
        // Must have no arguments and no block
        node.arguments().is_none() && node.block().is_none()
    }

    fn check_send(&mut self, node: &ruby_prism::CallNode) {
        if !self.is_self_receiver_call(node) {
            return;
        }
        if !Self::is_regular_method_call(node) {
            return;
        }
        if self.is_it_method_in_block(node) {
            return;
        }

        let method_name = node_name!(node);

        // Check if method name clashes with a local variable/argument
        if self.name_clashes(&method_name) {
            return;
        }

        // Check for Kernel methods
        if Self::is_kernel_method(&method_name) {
            return;
        }

        // Report offense on the `self` keyword (the receiver)
        let recv = node.receiver().unwrap();
        let loc = recv.location();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/RedundantSelf",
            MSG,
            Severity::Convention,
            loc.start_offset(),
            loc.end_offset(),
        ));
    }

    /// Collect argument names from parameter nodes.
    fn collect_params(&mut self, node: &Node) {
        match node {
            Node::RequiredParameterNode { .. } => {
                let param = node.as_required_parameter_node().unwrap();
                let name = String::from_utf8_lossy(param.name().as_slice()).to_string();
                self.add_param(&name);
            }
            Node::OptionalParameterNode { .. } => {
                let param = node.as_optional_parameter_node().unwrap();
                let name = String::from_utf8_lossy(param.name().as_slice()).to_string();
                self.add_param(&name);
            }
            Node::RestParameterNode { .. } => {
                let param = node.as_rest_parameter_node().unwrap();
                if let Some(name_loc) = param.name() {
                    let name = String::from_utf8_lossy(name_loc.as_slice()).to_string();
                    self.add_param(&name);
                }
            }
            Node::KeywordRestParameterNode { .. } => {
                let param = node.as_keyword_rest_parameter_node().unwrap();
                if let Some(name_loc) = param.name() {
                    let name = String::from_utf8_lossy(name_loc.as_slice()).to_string();
                    self.add_param(&name);
                }
            }
            Node::RequiredKeywordParameterNode { .. } => {
                let param = node.as_required_keyword_parameter_node().unwrap();
                let name = String::from_utf8_lossy(param.name().as_slice()).to_string();
                self.add_param(&name);
            }
            Node::OptionalKeywordParameterNode { .. } => {
                let param = node.as_optional_keyword_parameter_node().unwrap();
                let name = String::from_utf8_lossy(param.name().as_slice()).to_string();
                self.add_param(&name);
            }
            Node::BlockParameterNode { .. } => {
                let param = node.as_block_parameter_node().unwrap();
                if let Some(name_loc) = param.name() {
                    let name = String::from_utf8_lossy(name_loc.as_slice()).to_string();
                    self.add_param(&name);
                }
            }
            Node::MultiTargetNode { .. } => {
                let multi = node.as_multi_target_node().unwrap();
                for left in multi.lefts().iter() {
                    self.collect_params(&left);
                }
                if let Some(rest) = multi.rest() {
                    self.collect_params(&rest);
                }
                for right in multi.rights().iter() {
                    self.collect_params(&right);
                }
            }
            Node::BlockLocalVariableNode { .. } => {
                let blv = node.as_block_local_variable_node().unwrap();
                let name = String::from_utf8_lossy(blv.name().as_slice()).to_string();
                self.add_param(&name);
            }
            _ => {}
        }
    }

    fn collect_params_from_params_node(&mut self, params: &ruby_prism::ParametersNode) {
        for p in params.requireds().iter() {
            self.collect_params(&p);
        }
        for p in params.optionals().iter() {
            self.collect_params(&p);
        }
        if let Some(rest) = params.rest() {
            self.collect_params(&rest);
        }
        for p in params.keywords().iter() {
            self.collect_params(&p);
        }
        if let Some(kw_rest) = params.keyword_rest() {
            self.collect_params(&kw_rest);
        }
        if let Some(block) = params.block() {
            self.collect_params(&block.as_node());
        }
    }

    /// Collect all lvasgn and masgn variable names from a subtree.
    fn collect_all_lvasgn_names_in_subtree(node: &Node) -> HashSet<String> {
        let mut names = HashSet::new();
        let mut collector = LvasgnCollector { names: &mut names };
        collector.visit(node);
        names
    }

    /// Collect all match_var names from a pattern subtree.
    fn collect_match_var_names(node: &Node) -> HashSet<String> {
        let mut names = HashSet::new();
        let mut collector = MatchVarCollector { names: &mut names };
        collector.visit(node);
        names
    }

    /// Check if block has empty/no params (for `self.it` check).
    fn block_has_no_params(params_node: &Option<Node>) -> bool {
        match params_node {
            None => true,
            Some(node) => {
                if let Some(bp) = node.as_block_parameters_node() {
                    if let Some(_params) = bp.parameters() {
                        // Has params node → has explicit params (even if ||)
                        false
                    } else {
                        // BlockParametersNode without ParametersNode: `||` (empty pipes)
                        // This has delimiters → NOT "empty_and_without_delimiters"
                        false
                    }
                } else if node.as_numbered_parameters_node().is_some() {
                    false
                } else if node.as_it_parameters_node().is_some() {
                    false
                } else {
                    true
                }
            }
        }
    }
}

impl Visit<'_> for RedundantSelfVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.push_scope();
        if let Some(params) = node.parameters() {
            self.collect_params_from_params_node(&params);
        }
        if let Some(body) = node.body() {
            self.visit(&body);
        }
        self.pop_scope();
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // Block scope inherits enclosing scope's params (they're still on the stack)
        self.push_scope();

        let params = node.parameters();
        let no_params = Self::block_has_no_params(&params);

        if no_params {
            self.block_no_params_depth += 1;
        }

        // Collect block params
        if let Some(bp_node) = params {
            if let Some(bp) = bp_node.as_block_parameters_node() {
                if let Some(params) = bp.parameters() {
                    self.collect_params_from_params_node(&params);
                }
                for local in bp.locals().iter() {
                    self.collect_params(&local);
                }
            }
        }

        if let Some(body) = node.body() {
            self.visit(&body);
        }

        if no_params {
            self.block_no_params_depth -= 1;
        }
        self.pop_scope();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_send(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        // `a = self.a` — add `a` as temp name ONLY during RHS visit
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        // Check if RHS has arguments (method call with args)
        // RuboCop's add_lhs_to_local_variables_scopes: if rhs is send with args,
        // add to each argument's scope, not to rhs itself
        let value = node.value();
        if let Some(call) = value.as_call_node() {
            if let Some(args) = call.arguments() {
                // Add name to scope of arguments only
                let mut names = HashSet::new();
                names.insert(name.clone());
                self.push_temp_names(names);
                // Visit the full value (including the call and its args)
                self.visit(&value);
                self.pop_temp_names();
                return;
            }
        }

        // Simple case: add name to scope of the entire RHS
        let mut names = HashSet::new();
        names.insert(name);
        self.push_temp_names(names);
        self.visit(&value);
        self.pop_temp_names();
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let mut names = HashSet::new();
        names.insert(name);
        self.push_temp_names(names);
        self.visit(&node.value());
        self.pop_temp_names();
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let mut names = HashSet::new();
        names.insert(name);
        self.push_temp_names(names);
        self.visit(&node.value());
        self.pop_temp_names();
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let mut names = HashSet::new();
        names.insert(name);
        self.push_temp_names(names);
        self.visit(&node.value());
        self.pop_temp_names();
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        // `a, b = self.a` — add a, b as temp names during RHS visit
        let mut names = HashSet::new();
        for target in node.lefts().iter() {
            if let Some(lva) = target.as_local_variable_target_node() {
                let name = String::from_utf8_lossy(lva.name().as_slice()).to_string();
                names.insert(name);
            }
        }
        self.push_temp_names(names);
        self.visit(&node.value());
        self.pop_temp_names();
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // For if/unless/while/until: collect lvasgn names from body and add to condition scope.
        // This allows `a = self.a if self.a` — `self.a` in condition is needed.
        let mut body_lvasgn_names = HashSet::new();
        if let Some(stmts) = node.statements() {
            body_lvasgn_names.extend(Self::collect_all_lvasgn_names_in_subtree(&stmts.as_node()));
        }
        if let Some(cons) = node.subsequent() {
            body_lvasgn_names.extend(Self::collect_all_lvasgn_names_in_subtree(&cons));
        }

        // Visit condition with body's lvasgn names as temp
        self.push_temp_names(body_lvasgn_names);
        self.visit(&node.predicate());
        self.pop_temp_names();

        // Visit then-body
        if let Some(stmts) = node.statements() {
            self.visit(&stmts.as_node());
        }
        // Visit else
        if let Some(cons) = node.subsequent() {
            self.visit(&cons);
        }
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let mut body_lvasgn_names = HashSet::new();
        if let Some(stmts) = node.statements() {
            body_lvasgn_names.extend(Self::collect_all_lvasgn_names_in_subtree(&stmts.as_node()));
        }
        if let Some(ec) = node.else_clause() {
            if let Some(stmts) = ec.statements() {
                body_lvasgn_names.extend(Self::collect_all_lvasgn_names_in_subtree(&stmts.as_node()));
            }
        }

        self.push_temp_names(body_lvasgn_names);
        self.visit(&node.predicate());
        self.pop_temp_names();

        if let Some(stmts) = node.statements() {
            self.visit(&stmts.as_node());
        }
        if let Some(ec) = node.else_clause() {
            if let Some(stmts) = ec.statements() {
                self.visit(&stmts.as_node());
            }
        }
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let mut body_lvasgn_names = HashSet::new();
        if let Some(stmts) = node.statements() {
            body_lvasgn_names.extend(Self::collect_all_lvasgn_names_in_subtree(&stmts.as_node()));
        }

        self.push_temp_names(body_lvasgn_names);
        self.visit(&node.predicate());
        self.pop_temp_names();

        if let Some(stmts) = node.statements() {
            self.visit(&stmts.as_node());
        }
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let mut body_lvasgn_names = HashSet::new();
        if let Some(stmts) = node.statements() {
            body_lvasgn_names.extend(Self::collect_all_lvasgn_names_in_subtree(&stmts.as_node()));
        }

        self.push_temp_names(body_lvasgn_names);
        self.visit(&node.predicate());
        self.pop_temp_names();

        if let Some(stmts) = node.statements() {
            self.visit(&stmts.as_node());
        }
    }

    fn visit_in_node(&mut self, node: &ruby_prism::InNode) {
        // Pattern matching: `in Integer => bar` — add match_var names to scope for this branch
        let match_vars = Self::collect_match_var_names(&node.pattern());
        self.push_temp_names(match_vars);
        // Visit the pattern (for nested checks)
        self.visit(&node.pattern());
        // Visit the body/statements
        if let Some(stmts) = node.statements() {
            self.visit(&stmts.as_node());
        }
        self.pop_temp_names();
    }
}

/// Visitor to collect LocalVariableWriteNode and MultiWriteNode names.
struct LvasgnCollector<'a> {
    names: &'a mut HashSet<String>,
}

impl Visit<'_> for LvasgnCollector<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        self.names.insert(name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        for target in node.lefts().iter() {
            if let Some(lva) = target.as_local_variable_target_node() {
                let name = String::from_utf8_lossy(lva.name().as_slice()).to_string();
                self.names.insert(name);
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }
}

/// Visitor to collect match_var names from pattern matching.
struct MatchVarCollector<'a> {
    names: &'a mut HashSet<String>,
}

impl Visit<'_> for MatchVarCollector<'_> {
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        // match_var nodes in patterns — these create new bindings
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        self.names.insert(name);
        ruby_prism::visit_local_variable_target_node(self, node);
    }

    // Also capture CapturePatternNode targets (`Integer => bar`)
    fn visit_capture_pattern_node(&mut self, node: &ruby_prism::CapturePatternNode) {
        let target = node.target();
        let name = String::from_utf8_lossy(target.name().as_slice()).to_string();
        self.names.insert(name);
        ruby_prism::visit_capture_pattern_node(self, node);
    }

    // Don't descend into pinned patterns (`^foo`) — those are reads, not bindings
    fn visit_pinned_variable_node(&mut self, _node: &ruby_prism::PinnedVariableNode) {
        // Skip — pinned variables are reads, not new bindings
    }
}
