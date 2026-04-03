//! Variable liveness analysis (mirrors RuboCop's VariableForce).
//!
//! Forward-flow AST dispatcher that tracks local variable assignments and
//! references. Cops implement the `VariableForceHook` trait to receive
//! callbacks at scope boundaries and inspect variable state.
//!
//! ## Module structure
//!
//! - `scope` — Scope with variables map
//! - `variable` — Variable with assignments list
//! - `assignment` — Assignment with used? flag
//! - `variable_table` — Scope stack + variable lookup
//! - `branch` — Branch tracking for conditionals
//! - `suggestion` — "Did you mean?" logic using Levenshtein distance

pub mod assignment;
pub mod branch;
pub mod scope;
pub mod suggestion;
pub mod variable;
pub mod variable_table;

// Re-export the public API
pub use assignment::AssignmentKind;
pub use scope::{Scope, ScopeType};
pub use suggestion::levenshtein;
pub use variable::Variable;
pub use variable_table::VariableTable;

use branch::{Branch, BranchKind};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

/// Hook trait that cops implement to receive variable force events.
pub trait VariableForceHook {
    /// Called after leaving a scope. The cop can inspect all variables
    /// and their assignments to find issues.
    fn after_leaving_scope(&mut self, scope: &Scope, source: &str);
}

/// Main AST dispatcher: walks the AST forward, tracking variables.
pub struct VariableForceDispatcher<'a, H: VariableForceHook> {
    pub table: VariableTable,
    hook: &'a mut H,
    source: &'a str,
    /// Set of node offsets already scanned (for TWISTED_SCOPE handling)
    scanned_nodes: HashSet<usize>,
    /// Whether we're currently inside a modifier conditional's condition.
    in_modifier_condition: bool,
}

impl<'a, H: VariableForceHook> VariableForceDispatcher<'a, H> {
    pub fn new(hook: &'a mut H, source: &'a str) -> Self {
        Self {
            table: VariableTable::new(),
            hook,
            source,
            scanned_nodes: HashSet::new(),
            in_modifier_condition: false,
        }
    }

    /// Entry point: analyze a program node.
    pub fn investigate(&mut self, program: &ruby_prism::ProgramNode) {
        let offset = program.location().start_offset();
        let end_offset = program.location().end_offset();
        self.table.push_scope(offset, end_offset, ScopeType::TopLevel);
        let stmts = program.statements();
        for stmt in stmts.body().iter() {
            self.process_node(&stmt);
        }
        if let Some(scope) = self.table.pop_scope() {
            self.hook.after_leaving_scope(&scope, self.source);
        }
    }

    fn process_node(&mut self, node: &Node) {
        match node {
            // ── Variable assignment (lvasgn) ──
            Node::LocalVariableWriteNode { .. } => {
                self.process_variable_assignment(node);
            }

            // ── Operator assignments ──
            Node::LocalVariableOperatorWriteNode { .. } => {
                self.process_variable_operator_assignment(node);
            }
            Node::LocalVariableAndWriteNode { .. } => {
                self.process_variable_and_assignment(node);
            }
            Node::LocalVariableOrWriteNode { .. } => {
                self.process_variable_or_assignment(node);
            }

            // ── Variable reference (lvar) ──
            Node::LocalVariableReadNode { .. } => {
                let read = node.as_local_variable_read_node().unwrap();
                let name = name_str(&read.name());
                self.table.reference_variable(&name);
            }

            // ── Multiple assignment ──
            Node::MultiWriteNode { .. } => {
                self.process_multiple_assignment(node);
            }

            // ── Regexp named capture ──
            Node::MatchWriteNode { .. } => {
                self.process_regexp_named_captures(node);
            }

            // ── Argument declarations ──
            Node::RequiredParameterNode { .. } => {
                let p = node.as_required_parameter_node().unwrap();
                let name = name_str(&p.name());
                let loc = p.location();
                self.table.declare_argument(&name, self.is_in_method_scope(), false, false, false, loc.start_offset(), loc.end_offset());
            }
            Node::OptionalParameterNode { .. } => {
                let p = node.as_optional_parameter_node().unwrap();
                let name = name_str(&p.name());
                let name_loc = p.name_loc();
                self.table.declare_argument(&name, self.is_in_method_scope(), false, false, false, name_loc.start_offset(), name_loc.end_offset());
                // Process default value
                self.process_node(&p.value());
            }
            Node::RestParameterNode { .. } => {
                let p = node.as_rest_parameter_node().unwrap();
                if let Some(name_loc) = p.name_loc() {
                    let name = String::from_utf8_lossy(name_loc.as_slice()).to_string();
                    self.table.declare_argument(&name, self.is_in_method_scope(), false, false, false, name_loc.start_offset(), name_loc.end_offset());
                }
            }
            Node::RequiredKeywordParameterNode { .. } => {
                let p = node.as_required_keyword_parameter_node().unwrap();
                let name = name_str(&p.name()).trim_end_matches(':').to_string();
                let name_loc = p.name_loc();
                // name_loc includes trailing colon, subtract 1 for just the name
                self.table.declare_argument(&name, self.is_in_method_scope(), true, false, false, name_loc.start_offset(), name_loc.end_offset() - 1);
            }
            Node::OptionalKeywordParameterNode { .. } => {
                let p = node.as_optional_keyword_parameter_node().unwrap();
                let name = name_str(&p.name()).trim_end_matches(':').to_string();
                let name_loc = p.name_loc();
                // name_loc includes trailing colon, subtract 1 for just the name
                self.table.declare_argument(&name, self.is_in_method_scope(), true, false, false, name_loc.start_offset(), name_loc.end_offset() - 1);
                // Process default value
                self.process_node(&p.value());
            }
            Node::KeywordRestParameterNode { .. } => {
                let p = node.as_keyword_rest_parameter_node().unwrap();
                if let Some(name_loc) = p.name_loc() {
                    let name = String::from_utf8_lossy(name_loc.as_slice()).to_string();
                    self.table.declare_argument(&name, self.is_in_method_scope(), false, false, false, name_loc.start_offset(), name_loc.end_offset());
                }
            }
            Node::BlockParameterNode { .. } => {
                let p = node.as_block_parameter_node().unwrap();
                if let Some(name_loc) = p.name_loc() {
                    let name = String::from_utf8_lossy(name_loc.as_slice()).to_string();
                    self.table.declare_argument(&name, self.is_in_method_scope(), false, true, false, name_loc.start_offset(), name_loc.end_offset());
                }
            }
            Node::BlockLocalVariableNode { .. } => {
                let p = node.as_block_local_variable_node().unwrap();
                let name = name_str(&p.name());
                let loc = p.location();
                self.table.declare_argument(&name, false, false, false, true, loc.start_offset(), loc.end_offset());
            }

            // ── Scope-creating nodes ──
            Node::DefNode { .. } => {
                self.process_scope(node);
            }
            Node::ClassNode { .. } => {
                self.process_scope(node);
            }
            Node::ModuleNode { .. } => {
                self.process_scope(node);
            }
            Node::SingletonClassNode { .. } => {
                self.process_scope(node);
            }
            Node::BlockNode { .. } => {
                self.process_scope(node);
            }
            Node::LambdaNode { .. } => {
                self.process_scope(node);
            }

            // ── Loops ──
            Node::WhileNode { .. } => {
                self.process_loop(node);
            }
            Node::UntilNode { .. } => {
                self.process_loop(node);
            }
            Node::ForNode { .. } => {
                self.process_for_loop(node);
            }

            // ── Rescue ──
            Node::BeginNode { .. } => {
                self.process_begin(node);
            }

            // ── Zero-arity super ──
            Node::ForwardingSuperNode { .. } => {
                self.process_zero_arity_super();
                // Process block child if present (e.g., super { |bar| })
                let n = node.as_forwarding_super_node().unwrap();
                if let Some(block) = n.block() {
                    self.process_node(&block.as_node());
                }
            }

            // ── binding() call ──
            Node::CallNode { .. } => {
                self.process_call(node);
            }

            // ── If/Unless (for branching) ──
            Node::IfNode { .. } => {
                self.process_if(node);
            }
            Node::UnlessNode { .. } => {
                self.process_unless(node);
            }

            // ── Case/CaseMatch ──
            Node::CaseNode { .. } => {
                self.process_case(node);
            }
            Node::CaseMatchNode { .. } => {
                self.process_case_match(node);
            }

            // ── And/Or ──
            Node::AndNode { .. } => {
                self.process_and(node);
            }
            Node::OrNode { .. } => {
                self.process_or(node);
            }

            // ── Rescue node (as expression) ──
            Node::RescueNode { .. } => {
                self.process_rescue_node(node);
            }

            // ── Ensure ──
            Node::EnsureNode { .. } => {
                self.process_ensure(node);
            }

            // ── Pattern matching ──
            Node::MatchPredicateNode { .. } => {
                let mp = node.as_match_predicate_node().unwrap();
                self.process_node(&mp.value());
                self.process_pattern_variables(&mp.pattern());
            }
            Node::MatchRequiredNode { .. } => {
                let mr = node.as_match_required_node().unwrap();
                self.process_node(&mr.value());
                self.process_pattern_variables(&mr.pattern());
            }

            // ── Everything else: process children ──
            _ => {
                self.process_children(node);
            }
        }
    }

    fn process_children(&mut self, node: &Node) {
        match node {
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for stmt in stmts.body().iter() {
                    self.process_node(&stmt);
                }
            }
            Node::ParenthesesNode { .. } => {
                let p = node.as_parentheses_node().unwrap();
                if let Some(body) = p.body() {
                    self.process_node(&body);
                }
            }
            Node::InterpolatedStringNode { .. } => {
                let n = node.as_interpolated_string_node().unwrap();
                for part in n.parts().iter() {
                    self.process_node(&part);
                }
            }
            Node::InterpolatedSymbolNode { .. } => {
                let n = node.as_interpolated_symbol_node().unwrap();
                for part in n.parts().iter() {
                    self.process_node(&part);
                }
            }
            Node::InterpolatedRegularExpressionNode { .. } => {
                let n = node.as_interpolated_regular_expression_node().unwrap();
                for part in n.parts().iter() {
                    self.process_node(&part);
                }
            }
            Node::InterpolatedXStringNode { .. } => {
                let n = node.as_interpolated_x_string_node().unwrap();
                for part in n.parts().iter() {
                    self.process_node(&part);
                }
            }
            Node::EmbeddedStatementsNode { .. } => {
                let n = node.as_embedded_statements_node().unwrap();
                if let Some(stmts) = n.statements() {
                    self.process_node(&stmts.as_node());
                }
            }
            Node::ArrayNode { .. } => {
                let arr = node.as_array_node().unwrap();
                for elem in arr.elements().iter() {
                    self.process_node(&elem);
                }
            }
            Node::HashNode { .. } => {
                let hash = node.as_hash_node().unwrap();
                for elem in hash.elements().iter() {
                    self.process_node(&elem);
                }
            }
            Node::AssocNode { .. } => {
                let assoc = node.as_assoc_node().unwrap();
                self.process_node(&assoc.key());
                self.process_node(&assoc.value());
            }
            Node::AssocSplatNode { .. } => {
                let n = node.as_assoc_splat_node().unwrap();
                if let Some(value) = n.value() {
                    self.process_node(&value);
                }
            }
            Node::SplatNode { .. } => {
                let n = node.as_splat_node().unwrap();
                if let Some(expr) = n.expression() {
                    self.process_node(&expr);
                }
            }
            Node::KeywordHashNode { .. } => {
                let n = node.as_keyword_hash_node().unwrap();
                for elem in n.elements().iter() {
                    self.process_node(&elem);
                }
            }
            Node::RangeNode { .. } => {
                let n = node.as_range_node().unwrap();
                if let Some(left) = n.left() {
                    self.process_node(&left);
                }
                if let Some(right) = n.right() {
                    self.process_node(&right);
                }
            }
            Node::ReturnNode { .. } => {
                let n = node.as_return_node().unwrap();
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
            }
            Node::YieldNode { .. } => {
                let n = node.as_yield_node().unwrap();
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
            }
            Node::BreakNode { .. } => {
                let n = node.as_break_node().unwrap();
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
            }
            Node::NextNode { .. } => {
                let n = node.as_next_node().unwrap();
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
            }
            Node::ArgumentsNode { .. } => {
                let n = node.as_arguments_node().unwrap();
                for arg in n.arguments().iter() {
                    self.process_node(&arg);
                }
            }
            Node::DefinedNode { .. } => {
                let n = node.as_defined_node().unwrap();
                self.process_node(&n.value());
            }
            Node::FlipFlopNode { .. } => {
                let n = node.as_flip_flop_node().unwrap();
                if let Some(left) = n.left() {
                    self.process_node(&left);
                }
                if let Some(right) = n.right() {
                    self.process_node(&right);
                }
            }
            Node::SuperNode { .. } => {
                let n = node.as_super_node().unwrap();
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
                if let Some(block) = n.block() {
                    self.process_node(&block);
                }
            }
            Node::ConstantPathWriteNode { .. } => {
                let n = node.as_constant_path_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ConstantWriteNode { .. } => {
                let n = node.as_constant_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::InstanceVariableWriteNode { .. } => {
                let n = node.as_instance_variable_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ClassVariableWriteNode { .. } => {
                let n = node.as_class_variable_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::GlobalVariableWriteNode { .. } => {
                let n = node.as_global_variable_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::InstanceVariableOperatorWriteNode { .. } => {
                let n = node.as_instance_variable_operator_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ClassVariableOperatorWriteNode { .. } => {
                let n = node.as_class_variable_operator_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::GlobalVariableOperatorWriteNode { .. } => {
                let n = node.as_global_variable_operator_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::InstanceVariableAndWriteNode { .. } => {
                let n = node.as_instance_variable_and_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::InstanceVariableOrWriteNode { .. } => {
                let n = node.as_instance_variable_or_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ClassVariableAndWriteNode { .. } => {
                let n = node.as_class_variable_and_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ClassVariableOrWriteNode { .. } => {
                let n = node.as_class_variable_or_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::GlobalVariableAndWriteNode { .. } => {
                let n = node.as_global_variable_and_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::GlobalVariableOrWriteNode { .. } => {
                let n = node.as_global_variable_or_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ConstantOperatorWriteNode { .. } => {
                let n = node.as_constant_operator_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ConstantAndWriteNode { .. } => {
                let n = node.as_constant_and_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ConstantOrWriteNode { .. } => {
                let n = node.as_constant_or_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ConstantPathOperatorWriteNode { .. } => {
                let n = node.as_constant_path_operator_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ConstantPathAndWriteNode { .. } => {
                let n = node.as_constant_path_and_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::ConstantPathOrWriteNode { .. } => {
                let n = node.as_constant_path_or_write_node().unwrap();
                self.process_node(&n.value());
            }
            Node::IndexOperatorWriteNode { .. } => {
                let n = node.as_index_operator_write_node().unwrap();
                if let Some(recv) = n.receiver() {
                    self.process_node(&recv);
                }
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
                self.process_node(&n.value());
            }
            Node::IndexAndWriteNode { .. } => {
                let n = node.as_index_and_write_node().unwrap();
                if let Some(recv) = n.receiver() {
                    self.process_node(&recv);
                }
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
                self.process_node(&n.value());
            }
            Node::IndexOrWriteNode { .. } => {
                let n = node.as_index_or_write_node().unwrap();
                if let Some(recv) = n.receiver() {
                    self.process_node(&recv);
                }
                if let Some(args) = n.arguments() {
                    self.process_node(&args.as_node());
                }
                self.process_node(&n.value());
            }
            Node::CallOperatorWriteNode { .. } => {
                let n = node.as_call_operator_write_node().unwrap();
                if let Some(recv) = n.receiver() {
                    self.process_node(&recv);
                }
                self.process_node(&n.value());
            }
            Node::CallAndWriteNode { .. } => {
                let n = node.as_call_and_write_node().unwrap();
                if let Some(recv) = n.receiver() {
                    self.process_node(&recv);
                }
                self.process_node(&n.value());
            }
            Node::CallOrWriteNode { .. } => {
                let n = node.as_call_or_write_node().unwrap();
                if let Some(recv) = n.receiver() {
                    self.process_node(&recv);
                }
                self.process_node(&n.value());
            }
            Node::ElseNode { .. } => {
                let n = node.as_else_node().unwrap();
                if let Some(stmts) = n.statements() {
                    self.process_node(&stmts.as_node());
                }
            }
            Node::SingletonClassNode { .. } | Node::DefNode { .. } | Node::ClassNode { .. }
            | Node::ModuleNode { .. } | Node::BlockNode { .. } | Node::LambdaNode { .. } => {
                // Already handled above
            }
            Node::ParametersNode { .. } => {
                let params = node.as_parameters_node().unwrap();
                for p in params.requireds().iter() {
                    self.process_node(&p);
                }
                for p in params.optionals().iter() {
                    self.process_node(&p);
                }
                if let Some(rest) = params.rest() {
                    self.process_node(&rest);
                }
                for p in params.posts().iter() {
                    self.process_node(&p);
                }
                for p in params.keywords().iter() {
                    self.process_node(&p);
                }
                if let Some(kr) = params.keyword_rest() {
                    self.process_node(&kr);
                }
                if let Some(block) = params.block() {
                    self.process_node(&block.as_node());
                }
            }
            Node::BlockParametersNode { .. } => {
                let bp = node.as_block_parameters_node().unwrap();
                if let Some(params) = bp.parameters() {
                    self.process_node(&params.as_node());
                }
                for local in bp.locals().iter() {
                    self.process_node(&local);
                }
            }
            Node::PostExecutionNode { .. } => {
                let n = node.as_post_execution_node().unwrap();
                if let Some(stmts) = n.statements() {
                    self.process_node(&stmts.as_node());
                }
            }
            Node::PreExecutionNode { .. } => {
                let n = node.as_pre_execution_node().unwrap();
                if let Some(stmts) = n.statements() {
                    self.process_node(&stmts.as_node());
                }
            }
            _ => {
                // For any other nodes, use Visit to find variable refs
                let mut fallback = FallbackVisitor { dispatcher: self };
                fallback.visit(node);
            }
        }
    }

    // ── Variable assignment ──

    fn process_variable_assignment(&mut self, node: &Node) {
        let write = node.as_local_variable_write_node().unwrap();
        let name = name_str(&write.name());

        // Declare if new
        if !self.table.variable_exist(&name) {
            self.table.declare_variable(&name, false, false);
        }

        // Process RHS first (so we can reference the variable if rhs uses it)
        self.process_node(&write.value());

        // Now assign
        self.table.assign_to_variable(
            &name,
            write.name_loc().start_offset(),
            write.name_loc().end_offset(),
            AssignmentKind::Simple,
            None,
            node.location().start_offset(),
        );

        // Mark as modifier conditional if inside one
        if self.in_modifier_condition {
            if let Some(scope) = self.table.current_scope_mut() {
                if let Some(var) = scope.variables.get_mut(&name) {
                    if let Some(last) = var.assignments.last_mut() {
                        last.in_modifier_conditional = true;
                    }
                }
            }
        }
    }

    fn process_variable_operator_assignment(&mut self, node: &Node) {
        let write = node.as_local_variable_operator_write_node().unwrap();
        let name = name_str(&write.name());
        let op = String::from_utf8_lossy(write.binary_operator().as_slice()).to_string();

        if !self.table.variable_exist(&name) {
            self.table.declare_variable(&name, false, false);
        }

        // Reference first (op-assign reads the variable)
        self.table.reference_variable(&name);

        // RHS of op-assign is on a branch (OpAsgn right_body)
        let branch = Branch::new(
            node.location().start_offset(),
            1,
            BranchKind::OpAssign,
        );
        self.table.push_branch(branch);
        self.process_node(&write.value());
        self.table.pop_branch();

        // Then assign
        self.table.assign_to_variable(
            &name,
            write.name_loc().start_offset(),
            write.name_loc().end_offset(),
            AssignmentKind::OperatorAssignment,
            Some(op),
            node.location().start_offset(),
        );
    }

    fn process_variable_and_assignment(&mut self, node: &Node) {
        let write = node.as_local_variable_and_write_node().unwrap();
        let name = name_str(&write.name());

        if !self.table.variable_exist(&name) {
            self.table.declare_variable(&name, false, false);
        }

        // Reference first
        self.table.reference_variable(&name);

        // RHS of &&= is on a branch (AndAsgn right_body)
        let branch = Branch::new(
            node.location().start_offset(),
            1,
            BranchKind::AndAssign,
        );
        self.table.push_branch(branch);
        self.process_node(&write.value());
        self.table.pop_branch();

        // Then assign
        self.table.assign_to_variable(
            &name,
            write.name_loc().start_offset(),
            write.name_loc().end_offset(),
            AssignmentKind::AndAssignment,
            Some("&&".to_string()),
            node.location().start_offset(),
        );
    }

    fn process_variable_or_assignment(&mut self, node: &Node) {
        let write = node.as_local_variable_or_write_node().unwrap();
        let name = name_str(&write.name());

        if !self.table.variable_exist(&name) {
            self.table.declare_variable(&name, false, false);
        }

        // Reference first
        self.table.reference_variable(&name);

        // RHS of ||= is on a branch (OrAsgn right_body)
        let branch = Branch::new(
            node.location().start_offset(),
            1,
            BranchKind::OrAssign,
        );
        self.table.push_branch(branch);
        self.process_node(&write.value());
        self.table.pop_branch();

        // Then assign
        self.table.assign_to_variable(
            &name,
            write.name_loc().start_offset(),
            write.name_loc().end_offset(),
            AssignmentKind::OrAssignment,
            Some("||".to_string()),
            node.location().start_offset(),
        );
    }

    // ── Multiple assignment ──

    fn process_multiple_assignment(&mut self, node: &Node) {
        let multi = node.as_multi_write_node().unwrap();

        // Process RHS first (like RuboCop)
        self.process_node(&multi.value());

        // Then process LHS targets
        for target in multi.lefts().iter() {
            self.process_multi_target(&target, node.location().start_offset());
        }
        if let Some(rest) = multi.rest() {
            self.process_multi_target(&rest, node.location().start_offset());
        }
        for target in multi.rights().iter() {
            self.process_multi_target(&target, node.location().start_offset());
        }
    }

    fn process_multi_target(&mut self, target: &Node, parent_offset: usize) {
        match target {
            Node::LocalVariableTargetNode { .. } => {
                let lv = target.as_local_variable_target_node().unwrap();
                let name = name_str(&lv.name());
                if !self.table.variable_exist(&name) {
                    self.table.declare_variable(&name, false, false);
                }
                self.table.assign_to_variable(
                    &name,
                    lv.location().start_offset(),
                    lv.location().end_offset(),
                    AssignmentKind::MultipleAssignment,
                    None,
                    parent_offset,
                );
            }
            Node::SplatNode { .. } => {
                let splat = target.as_splat_node().unwrap();
                if let Some(expr) = splat.expression() {
                    self.process_multi_target(&expr, parent_offset);
                }
            }
            Node::MultiTargetNode { .. } => {
                let mt = target.as_multi_target_node().unwrap();
                for t in mt.lefts().iter() {
                    self.process_multi_target(&t, parent_offset);
                }
                if let Some(rest) = mt.rest() {
                    self.process_multi_target(&rest, parent_offset);
                }
                for t in mt.rights().iter() {
                    self.process_multi_target(&t, parent_offset);
                }
            }
            _ => {}
        }
    }

    // ── Regexp named captures ──

    fn process_regexp_named_captures(&mut self, node: &Node) {
        let mw = node.as_match_write_node().unwrap();
        let call = mw.call();

        // Process the RHS (the string being matched against)
        if let Some(args) = call.arguments() {
            for arg in args.arguments().iter() {
                self.process_node(&arg);
            }
        }
        // Process the receiver (the regexp)
        if let Some(recv) = call.receiver() {
            self.process_node(&recv);
        }

        let regexp_loc = call.receiver().map(|r| (r.location().start_offset(), r.location().end_offset()));

        // Declare and assign capture variables
        for target in mw.targets().iter() {
            if let Some(lv) = target.as_local_variable_target_node() {
                let name = name_str(&lv.name());
                if !self.table.variable_exist(&name) {
                    self.table.declare_variable(&name, false, false);
                }
                let (rs, re) = regexp_loc.unwrap_or((lv.location().start_offset(), lv.location().end_offset()));
                let name_start = lv.location().start_offset();
                let name_end = lv.location().end_offset();
                // Need to set regexp offsets on the assignment
                self.table.assign_to_variable(
                    &name,
                    name_start,
                    name_end,
                    AssignmentKind::RegexpNamedCapture,
                    None,
                    node.location().start_offset(),
                );
                // Set regexp location on the last assignment
                if let Some(scope) = self.table.current_scope_mut() {
                    if let Some(var) = scope.variables.get_mut(&name) {
                        if let Some(last) = var.assignments.last_mut() {
                            last.regexp_start = rs;
                            last.regexp_end = re;
                        }
                    }
                }
            }
        }
    }

    // ── Scope handling ──

    fn process_scope(&mut self, node: &Node) {
        // TWISTED_SCOPE_TYPES: block, class, sclass, defs, module
        // For these, some children belong to the outer scope.
        let is_twisted = matches!(
            node,
            Node::BlockNode { .. }
            | Node::LambdaNode { .. }
            | Node::ClassNode { .. }
            | Node::SingletonClassNode { .. }
            | Node::ModuleNode { .. }
        ) || {
            // defs (def on an object) is also twisted
            if let Some(def) = node.as_def_node() {
                def.receiver().is_some()
            } else {
                false
            }
        };

        if is_twisted {
            // Process outer-scope children first
            self.process_twisted_outer_children(node);
        }

        // Enter new scope
        self.inspect_variables_in_scope(node);
    }

    fn process_twisted_outer_children(&mut self, node: &Node) {
        match node {
            Node::BlockNode { .. } => {
                // The call expression (receiver of the block) is in outer scope
                // For `foo(bar) { }`, `foo(bar)` is outer scope
                // But in Prism, the call is the parent — we don't see it here
                // because BlockNode is a child. The call's args are processed
                // by the call node handler.
            }
            Node::LambdaNode { .. } => {
                // Lambda has no outer-scope children
            }
            Node::ClassNode { .. } => {
                let class = node.as_class_node().unwrap();
                // constant_path and superclass are in outer scope
                self.process_node(&class.constant_path());
                if let Some(superclass) = class.superclass() {
                    let offset = superclass.location().start_offset();
                    self.process_node(&superclass);
                    self.scanned_nodes.insert(offset);
                }
            }
            Node::ModuleNode { .. } => {
                let module = node.as_module_node().unwrap();
                self.process_node(&module.constant_path());
            }
            Node::SingletonClassNode { .. } => {
                let sc = node.as_singleton_class_node().unwrap();
                let offset = sc.expression().location().start_offset();
                self.process_node(&sc.expression());
                self.scanned_nodes.insert(offset);
            }
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                if let Some(recv) = def.receiver() {
                    let offset = recv.location().start_offset();
                    self.process_node(&recv);
                    self.scanned_nodes.insert(offset);
                }
            }
            _ => {}
        }
    }

    fn inspect_variables_in_scope(&mut self, node: &Node) {
        let offset = node.location().start_offset();
        let end_offset = node.location().end_offset();
        let scope_type = match node {
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                if def.receiver().is_some() {
                    ScopeType::Defs
                } else {
                    ScopeType::Def
                }
            }
            Node::ClassNode { .. } => ScopeType::Class,
            Node::ModuleNode { .. } => ScopeType::Module,
            Node::SingletonClassNode { .. } => ScopeType::SingletonClass,
            Node::BlockNode { .. } => ScopeType::Block,
            Node::LambdaNode { .. } => ScopeType::Lambda,
            _ => ScopeType::TopLevel,
        };

        self.table.push_scope(offset, end_offset, scope_type);

        // Process the scope's body and parameter declarations
        match node {
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                let method_name = name_str(&def.name());
                if let Some(scope) = self.table.current_scope_mut() {
                    scope.name = Some(method_name);
                    scope.body_is_empty = def.body().is_none();
                }
                if let Some(params) = def.parameters() {
                    self.process_node(&params.as_node());
                }
                if let Some(body) = def.body() {
                    self.process_node(&body);
                }
            }
            Node::ClassNode { .. } => {
                let class = node.as_class_node().unwrap();
                if let Some(body) = class.body() {
                    self.process_node(&body);
                }
            }
            Node::ModuleNode { .. } => {
                let module = node.as_module_node().unwrap();
                if let Some(body) = module.body() {
                    self.process_node(&body);
                }
            }
            Node::SingletonClassNode { .. } => {
                let sc = node.as_singleton_class_node().unwrap();
                if let Some(body) = sc.body() {
                    self.process_node(&body);
                }
            }
            Node::BlockNode { .. } => {
                let block = node.as_block_node().unwrap();
                if let Some(scope) = self.table.current_scope_mut() {
                    scope.body_is_empty = block.body().is_none();
                }
                if let Some(params) = block.parameters() {
                    self.process_node(&params);
                }
                if let Some(body) = block.body() {
                    self.process_node(&body);
                }
            }
            Node::LambdaNode { .. } => {
                let lambda = node.as_lambda_node().unwrap();
                if let Some(scope) = self.table.current_scope_mut() {
                    scope.body_is_empty = lambda.body().is_none();
                }
                if let Some(params) = lambda.parameters() {
                    self.process_node(&params);
                }
                if let Some(body) = lambda.body() {
                    self.process_node(&body);
                }
            }
            _ => {}
        }

        if let Some(scope) = self.table.pop_scope() {
            self.hook.after_leaving_scope(&scope, self.source);
        }
    }

    // ── Loops ──

    fn process_loop(&mut self, node: &Node) {
        let offset = node.location().start_offset();

        match node {
            Node::WhileNode { .. } => {
                let w = node.as_while_node().unwrap();
                let kw_loc = w.keyword_loc();
                let is_post_condition = if let Some(stmts) = w.statements() {
                    kw_loc.start_offset() > stmts.location().start_offset()
                } else {
                    false
                };

                if is_post_condition {
                    // Body first (in branch), then condition
                    let branch = Branch::new(offset, 1, BranchKind::WhilePost);
                    self.table.push_branch(branch);
                    if let Some(stmts) = w.statements() {
                        self.process_node(&stmts.as_node());
                    }
                    self.table.pop_branch();
                    self.process_node(&w.predicate());
                } else {
                    // Condition always runs
                    self.process_node(&w.predicate());
                    // Body is a branch (may not execute)
                    let branch = Branch::new(offset, 1, BranchKind::While);
                    self.table.push_branch(branch);
                    if let Some(stmts) = w.statements() {
                        self.process_node(&stmts.as_node());
                    }
                    self.table.pop_branch();
                }
            }
            Node::UntilNode { .. } => {
                let u = node.as_until_node().unwrap();
                let kw_loc = u.keyword_loc();
                let is_post_condition = if let Some(stmts) = u.statements() {
                    kw_loc.start_offset() > stmts.location().start_offset()
                } else {
                    false
                };

                if is_post_condition {
                    let branch = Branch::new(offset, 1, BranchKind::UntilPost);
                    self.table.push_branch(branch);
                    if let Some(stmts) = u.statements() {
                        self.process_node(&stmts.as_node());
                    }
                    self.table.pop_branch();
                    self.process_node(&u.predicate());
                } else {
                    self.process_node(&u.predicate());
                    let branch = Branch::new(offset, 1, BranchKind::Until);
                    self.table.push_branch(branch);
                    if let Some(stmts) = u.statements() {
                        self.process_node(&stmts.as_node());
                    }
                    self.table.pop_branch();
                }
            }
            _ => {
                self.process_children(node);
            }
        }

        // After processing the loop, mark assignments as referenced in loop
        self.mark_assignments_as_referenced_in_loop(node);
    }

    fn process_for_loop(&mut self, node: &Node) {
        let f = node.as_for_node().unwrap();
        let offset = node.location().start_offset();

        // Collection is evaluated first (always runs)
        self.process_node(&f.collection());

        // For variable (index) - treat as assignment
        self.process_for_index(&f.index());

        // Body is a branch (loop body)
        let branch = Branch::new(offset, 2, BranchKind::For);
        self.table.push_branch(branch);
        if let Some(stmts) = f.statements() {
            self.process_node(&stmts.as_node());
        }
        self.table.pop_branch();

        // Mark loop references
        self.mark_assignments_as_referenced_in_loop(node);
    }

    fn process_for_index(&mut self, node: &Node) {
        match node {
            Node::LocalVariableTargetNode { .. } => {
                let lv = node.as_local_variable_target_node().unwrap();
                let name = name_str(&lv.name());
                if !self.table.variable_exist(&name) {
                    self.table.declare_variable(&name, false, false);
                }
                self.table.assign_to_variable(
                    &name,
                    lv.location().start_offset(),
                    lv.location().end_offset(),
                    AssignmentKind::MultipleAssignment,
                    None,
                    lv.location().start_offset(),
                );
            }
            Node::MultiTargetNode { .. } => {
                let mt = node.as_multi_target_node().unwrap();
                for t in mt.lefts().iter() {
                    self.process_for_index(&t);
                }
                if let Some(rest) = mt.rest() {
                    self.process_for_index(&rest);
                }
                for t in mt.rights().iter() {
                    self.process_for_index(&t);
                }
            }
            Node::SplatNode { .. } => {
                let splat = node.as_splat_node().unwrap();
                if let Some(expr) = splat.expression() {
                    self.process_for_index(&expr);
                }
            }
            _ => {}
        }
    }

    /// Mark last assignments referenced in a loop body as referenced.
    /// This is RuboCop's mark_assignments_as_referenced_in_loop.
    fn mark_assignments_as_referenced_in_loop(&mut self, loop_node: &Node) {
        // Collect all variable references and assignment offsets in the loop
        let mut ref_names: HashSet<String> = HashSet::new();
        let mut assign_offsets: HashSet<usize> = HashSet::new();
        collect_loop_refs(loop_node, &mut ref_names, &mut assign_offsets);

        // For each referenced variable, find its assignments that are in this loop
        // and mark them as referenced
        for name in &ref_names {
            if let Some(scope) = self.table.current_scope_mut() {
                if let Some(var) = scope.variables.get_mut(name) {
                    let loop_assignments: Vec<usize> = var
                        .assignments
                        .iter()
                        .enumerate()
                        .filter(|(_, a)| assign_offsets.contains(&a.node_offset))
                        .map(|(i, _)| i)
                        .collect();

                    if loop_assignments.is_empty() {
                        continue;
                    }

                    // Reference assignments:
                    // - All assignments inside branch nodes are referenced
                    // - The last assignment is always referenced
                    let last_idx = *loop_assignments.last().unwrap();
                    var.assignments[last_idx].reference();

                    for &idx in &loop_assignments {
                        if var.assignments[idx].branch.is_some() {
                            var.assignments[idx].reference();
                        }
                    }
                }
            }
        }
    }

    // ── Begin/Rescue ──

    fn process_begin(&mut self, node: &Node) {
        let begin = node.as_begin_node().unwrap();
        let offset = node.location().start_offset();
        let has_rescue = begin.rescue_clause().is_some();
        let has_ensure = begin.ensure_clause().is_some();

        // Check for retry in rescue clauses
        let has_retry = has_retry_in_rescue(&begin);

        if has_rescue || has_ensure {
            // Main body is a rescue branch (may jump to rescue on exception)
            let branch = Branch::new(offset, 0, BranchKind::Rescue);
            self.table.push_branch(branch);
            if let Some(stmts) = begin.statements() {
                self.process_node(&stmts.as_node());
            }
            self.table.pop_branch();

            // Each rescue clause is a separate branch
            let mut rc = begin.rescue_clause();
            let mut rescue_idx = 1;
            while let Some(clause) = rc {
                let branch = Branch::new(offset, rescue_idx, BranchKind::Rescue);
                self.table.push_branch(branch);
                self.process_rescue_clause(&clause);
                self.table.pop_branch();
                rescue_idx += 1;
                rc = clause.subsequent();
            }

            // Else clause (runs if no exception)
            if let Some(else_clause) = begin.else_clause() {
                let branch = Branch::new(offset, 100, BranchKind::Rescue);
                self.table.push_branch(branch);
                if let Some(stmts) = else_clause.statements() {
                    self.process_node(&stmts.as_node());
                }
                self.table.pop_branch();
            }

            // Ensure clause always runs (no branch)
            if let Some(ensure) = begin.ensure_clause() {
                if let Some(stmts) = ensure.statements() {
                    self.process_node(&stmts.as_node());
                }
            }
        } else {
            // No rescue/ensure — just process body
            if let Some(stmts) = begin.statements() {
                self.process_node(&stmts.as_node());
            }
        }

        // With retry, treat as loop
        if has_retry {
            self.mark_assignments_as_referenced_in_loop(node);
        }
    }

    fn process_rescue_clause(&mut self, clause: &ruby_prism::RescueNode) {
        // Process exception types
        for exc in clause.exceptions().iter() {
            self.process_node(&exc);
        }
        // Process exception variable assignment
        if let Some(reference) = clause.reference() {
            if let Some(lv) = reference.as_local_variable_target_node() {
                let name = name_str(&lv.name());
                if !self.table.variable_exist(&name) {
                    self.table.declare_variable(&name, false, false);
                }
                self.table.assign_to_variable(
                    &name,
                    lv.location().start_offset(),
                    lv.location().end_offset(),
                    AssignmentKind::Simple,
                    None,
                    lv.location().start_offset(),
                );
            }
        }
        // Process body
        if let Some(stmts) = clause.statements() {
            self.process_node(&stmts.as_node());
        }
    }

    fn process_rescue_node(&mut self, node: &Node) {
        let rescue = node.as_rescue_node().unwrap();
        // Process exception types
        for exc in rescue.exceptions().iter() {
            self.process_node(&exc);
        }
        // Exception variable
        if let Some(reference) = rescue.reference() {
            if let Some(lv) = reference.as_local_variable_target_node() {
                let name = name_str(&lv.name());
                if !self.table.variable_exist(&name) {
                    self.table.declare_variable(&name, false, false);
                }
                self.table.assign_to_variable(
                    &name,
                    lv.location().start_offset(),
                    lv.location().end_offset(),
                    AssignmentKind::Simple,
                    None,
                    lv.location().start_offset(),
                );
            }
        }
        if let Some(stmts) = rescue.statements() {
            self.process_node(&stmts.as_node());
        }
        if let Some(subsequent) = rescue.subsequent() {
            self.process_rescue_clause(&subsequent);
        }
    }

    fn process_ensure(&mut self, node: &Node) {
        let ensure = node.as_ensure_node().unwrap();
        if let Some(stmts) = ensure.statements() {
            self.process_node(&stmts.as_node());
        }
    }

    // ── Zero-arity super ──

    fn process_zero_arity_super(&mut self) {
        // Mark all method arguments as referenced
        // Need to find variables across the scope stack, not just current scope
        for scope in self.table.scope_stack.iter_mut().rev() {
            for var in scope.variables.values_mut() {
                if var.is_method_argument {
                    var.reference_count += 1;
                    for assignment in &mut var.assignments {
                        assignment.reference();
                    }
                }
            }
            if !scope.is_block() {
                break;
            }
        }
    }

    // ── Call / binding ──

    fn process_call(&mut self, node: &Node) {
        let call = node.as_call_node().unwrap();

        // Check for `binding` call
        if call.receiver().is_none() {
            if let Some(msg_loc) = call.message_loc() {
                let method_name = String::from_utf8_lossy(msg_loc.as_slice()).to_string();
                if method_name == "binding" {
                    let has_args = call
                        .arguments()
                        .map_or(false, |a| a.arguments().len() > 0);
                    if !has_args {
                        // Mark all accessible variables as referenced
                        let vars = self.table.accessible_variables_mut();
                        for var in vars {
                            var.reference_count += 1;
                            for assignment in &mut var.assignments {
                                assignment.reference();
                            }
                        }
                    }
                }
            }
        }

        // Process receiver
        if let Some(recv) = call.receiver() {
            self.process_node(&recv);
        }
        // Process arguments
        if let Some(args) = call.arguments() {
            for arg in args.arguments().iter() {
                self.process_node(&arg);
            }
        }
        // Process block (scope handled by process_scope)
        if let Some(block) = call.block() {
            self.process_node(&block);
        }
    }

    // ── Branching constructs ──

    fn process_if(&mut self, node: &Node) {
        let if_node = node.as_if_node().unwrap();

        // Detect modifier form: body appears before the keyword in source
        let is_modifier = if let Some(stmts) = if_node.statements() {
            if let Some(kw_loc) = if_node.if_keyword_loc() {
                stmts.location().start_offset() < kw_loc.start_offset()
            } else {
                false
            }
        } else {
            false
        };

        // Process condition (always runs)
        if is_modifier {
            self.in_modifier_condition = true;
        }
        self.process_node(&if_node.predicate());
        if is_modifier {
            self.in_modifier_condition = false;
        }

        // Process then-branch
        let branch = Branch::new(
            node.location().start_offset(),
            1, // then-branch
            BranchKind::If,
        );
        self.table.push_branch(branch);
        if let Some(stmts) = if_node.statements() {
            self.process_node(&stmts.as_node());
        }
        self.table.pop_branch();

        // Process else/elsif-branch
        if let Some(subsequent) = if_node.subsequent() {
            let branch = Branch::new(
                node.location().start_offset(),
                2, // else-branch
                BranchKind::If,
            );
            self.table.push_branch(branch);
            self.process_node(&subsequent);
            self.table.pop_branch();
        }
    }

    fn process_unless(&mut self, node: &Node) {
        let unless = node.as_unless_node().unwrap();

        // Detect modifier form: body appears before keyword
        let kw_loc = unless.keyword_loc();
        let is_modifier = if let Some(stmts) = unless.statements() {
            stmts.location().start_offset() < kw_loc.start_offset()
        } else {
            false
        };

        if is_modifier {
            self.in_modifier_condition = true;
        }
        self.process_node(&unless.predicate());
        if is_modifier {
            self.in_modifier_condition = false;
        }

        let branch = Branch::new(
            node.location().start_offset(),
            1,
            BranchKind::If,
        );
        self.table.push_branch(branch);
        if let Some(stmts) = unless.statements() {
            self.process_node(&stmts.as_node());
        }
        self.table.pop_branch();

        if let Some(else_clause) = unless.else_clause() {
            let branch = Branch::new(
                node.location().start_offset(),
                2,
                BranchKind::If,
            );
            self.table.push_branch(branch);
            self.process_node(&else_clause.as_node());
            self.table.pop_branch();
        }
    }

    fn process_case(&mut self, node: &Node) {
        let case = node.as_case_node().unwrap();

        if let Some(pred) = case.predicate() {
            self.process_node(&pred);
        }

        for (i, when) in case.conditions().iter().enumerate() {
            if let Some(when_node) = when.as_when_node() {
                for cond in when_node.conditions().iter() {
                    self.process_node(&cond);
                }
                let branch = Branch::new(
                    node.location().start_offset(),
                    i + 1,
                    BranchKind::Case,
                );
                self.table.push_branch(branch);
                if let Some(stmts) = when_node.statements() {
                    self.process_node(&stmts.as_node());
                }
                self.table.pop_branch();
            }
        }

        if let Some(else_clause) = case.else_clause() {
            let branch = Branch::new(
                node.location().start_offset(),
                100, // else
                BranchKind::Case,
            );
            self.table.push_branch(branch);
            if let Some(stmts) = else_clause.statements() {
                self.process_node(&stmts.as_node());
            }
            self.table.pop_branch();
        }
    }

    fn process_case_match(&mut self, node: &Node) {
        let case_match = node.as_case_match_node().unwrap();

        if let Some(pred) = case_match.predicate() {
            self.process_node(&pred);
        }

        for (i, in_node) in case_match.conditions().iter().enumerate() {
            if let Some(in_n) = in_node.as_in_node() {
                // Process pattern variables
                self.process_pattern_variables(&in_n.pattern());

                let branch = Branch::new(
                    node.location().start_offset(),
                    i + 1,
                    BranchKind::CaseMatch,
                );
                self.table.push_branch(branch);
                if let Some(stmts) = in_n.statements() {
                    self.process_node(&stmts.as_node());
                }
                self.table.pop_branch();
            }
        }

        if let Some(else_clause) = case_match.else_clause() {
            let branch = Branch::new(
                node.location().start_offset(),
                100,
                BranchKind::CaseMatch,
            );
            self.table.push_branch(branch);
            if let Some(stmts) = else_clause.statements() {
                self.process_node(&stmts.as_node());
            }
            self.table.pop_branch();
        }
    }

    fn process_and(&mut self, node: &Node) {
        let and = node.as_and_node().unwrap();
        // Left always executes
        self.process_node(&and.left());
        // Right is conditional
        let branch = Branch::new(
            node.location().start_offset(),
            1,
            BranchKind::And,
        );
        self.table.push_branch(branch);
        self.process_node(&and.right());
        self.table.pop_branch();
    }

    fn process_or(&mut self, node: &Node) {
        let or = node.as_or_node().unwrap();
        self.process_node(&or.left());
        let branch = Branch::new(
            node.location().start_offset(),
            1,
            BranchKind::Or,
        );
        self.table.push_branch(branch);
        self.process_node(&or.right());
        self.table.pop_branch();
    }

    // ── Pattern match variables ──

    fn process_pattern_variables(&mut self, pattern: &Node) {
        match pattern {
            Node::LocalVariableTargetNode { .. } => {
                let lv = pattern.as_local_variable_target_node().unwrap();
                let name = name_str(&lv.name());
                if !self.table.variable_exist(&name) {
                    self.table.declare_variable(&name, false, false);
                }
                // Don't assign - pattern match variables are just declarations
                // in RuboCop's model (never flagged as useless)
            }
            Node::CapturePatternNode { .. } => {
                let cp = pattern.as_capture_pattern_node().unwrap();
                let target = cp.target();
                let name = name_str(&target.name());
                if !self.table.variable_exist(&name) {
                    self.table.declare_variable(&name, false, false);
                }
                self.process_pattern_variables(&cp.value());
            }
            Node::HashPatternNode { .. } => {
                let hp = pattern.as_hash_pattern_node().unwrap();
                for elem in hp.elements().iter() {
                    self.process_pattern_variables(&elem);
                }
                if let Some(rest) = hp.rest() {
                    self.process_pattern_variables(&rest);
                }
            }
            Node::ArrayPatternNode { .. } => {
                let ap = pattern.as_array_pattern_node().unwrap();
                for elem in ap.requireds().iter() {
                    self.process_pattern_variables(&elem);
                }
                if let Some(rest) = ap.rest() {
                    self.process_pattern_variables(&rest);
                }
                for elem in ap.posts().iter() {
                    self.process_pattern_variables(&elem);
                }
            }
            Node::AssocNode { .. } => {
                let assoc = pattern.as_assoc_node().unwrap();
                self.process_pattern_variables(&assoc.value());
            }
            Node::SplatNode { .. } => {
                let splat = pattern.as_splat_node().unwrap();
                if let Some(expr) = splat.expression() {
                    self.process_pattern_variables(&expr);
                }
            }
            Node::FindPatternNode { .. } => {
                let fp = pattern.as_find_pattern_node().unwrap();
                if let Some(expr) = fp.left().expression() {
                    self.process_pattern_variables(&expr);
                }
                for elem in fp.requireds().iter() {
                    self.process_pattern_variables(&elem);
                }
                if let Some(splat) = fp.right().as_splat_node() {
                    if let Some(expr) = splat.expression() {
                        self.process_pattern_variables(&expr);
                    }
                }
            }
            Node::AssocSplatNode { .. } => {
                let n = pattern.as_assoc_splat_node().unwrap();
                if let Some(value) = n.value() {
                    self.process_pattern_variables(&value);
                }
            }
            Node::NoKeywordsParameterNode { .. } => {}
            Node::PinnedVariableNode { .. } => {
                let pv = pattern.as_pinned_variable_node().unwrap();
                // Pinned variable is a reference, not a declaration
                self.process_node(&pv.variable());
            }
            _ => {}
        }
    }

    // ── Helpers ──

    fn is_in_method_scope(&self) -> bool {
        self.table
            .current_scope()
            .map(|s| s.is_def())
            .unwrap_or(false)
    }
}

/// Fallback visitor for nodes we don't explicitly handle.
/// Only looks for variable reads/writes in the subtree.
struct FallbackVisitor<'a, 'b, H: VariableForceHook> {
    dispatcher: &'a mut VariableForceDispatcher<'b, H>,
}

impl<'a, 'b, H: VariableForceHook> Visit<'_> for FallbackVisitor<'a, 'b, H> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.dispatcher.table.reference_variable(&name);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = name_str(&node.name());
        if !self.dispatcher.table.variable_exist(&name) {
            self.dispatcher.table.declare_variable(&name, false, false);
        }
        // Visit value first
        ruby_prism::visit_local_variable_write_node(self, node);
        // Then assign
        self.dispatcher.table.assign_to_variable(
            &name,
            node.name_loc().start_offset(),
            node.name_loc().end_offset(),
            AssignmentKind::Simple,
            None,
            node.location().start_offset(),
        );
    }

    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode,
    ) {
        let name = name_str(&node.name());
        let op = String::from_utf8_lossy(node.binary_operator().as_slice()).to_string();
        if !self.dispatcher.table.variable_exist(&name) {
            self.dispatcher.table.declare_variable(&name, false, false);
        }
        self.dispatcher.table.reference_variable(&name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
        self.dispatcher.table.assign_to_variable(
            &name,
            node.name_loc().start_offset(),
            node.name_loc().end_offset(),
            AssignmentKind::OperatorAssignment,
            Some(op),
            node.location().start_offset(),
        );
    }

    fn visit_local_variable_and_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableAndWriteNode,
    ) {
        let name = name_str(&node.name());
        if !self.dispatcher.table.variable_exist(&name) {
            self.dispatcher.table.declare_variable(&name, false, false);
        }
        self.dispatcher.table.reference_variable(&name);
        ruby_prism::visit_local_variable_and_write_node(self, node);
        self.dispatcher.table.assign_to_variable(
            &name,
            node.name_loc().start_offset(),
            node.name_loc().end_offset(),
            AssignmentKind::AndAssignment,
            Some("&&".to_string()),
            node.location().start_offset(),
        );
    }

    fn visit_local_variable_or_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOrWriteNode,
    ) {
        let name = name_str(&node.name());
        if !self.dispatcher.table.variable_exist(&name) {
            self.dispatcher.table.declare_variable(&name, false, false);
        }
        self.dispatcher.table.reference_variable(&name);
        ruby_prism::visit_local_variable_or_write_node(self, node);
        self.dispatcher.table.assign_to_variable(
            &name,
            node.name_loc().start_offset(),
            node.name_loc().end_offset(),
            AssignmentKind::OrAssignment,
            Some("||".to_string()),
            node.location().start_offset(),
        );
    }

    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        let name = name_str(&node.name());
        if !self.dispatcher.table.variable_exist(&name) {
            self.dispatcher.table.declare_variable(&name, false, false);
        }
        self.dispatcher.table.assign_to_variable(
            &name,
            node.location().start_offset(),
            node.location().end_offset(),
            AssignmentKind::MultipleAssignment,
            None,
            node.location().start_offset(),
        );
    }

    fn visit_forwarding_super_node(&mut self, _node: &ruby_prism::ForwardingSuperNode) {
        // Mark all method arguments as referenced
        let vars = self.dispatcher.table.accessible_variables_mut();
        for var in vars {
            if var.is_method_argument {
                var.reference_count += 1;
                for assignment in &mut var.assignments {
                    assignment.reference();
                }
            }
        }
    }

    // Don't descend into scope-creating nodes (they're handled by the dispatcher)
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.dispatcher.process_scope(&node.as_node());
    }
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.dispatcher.process_scope(&node.as_node());
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.dispatcher.process_scope(&node.as_node());
    }
    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        self.dispatcher.process_scope(&node.as_node());
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.dispatcher.process_scope(&node.as_node());
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        self.dispatcher.process_scope(&node.as_node());
    }
}

// ── Helper functions ──

fn name_str(id: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(id.as_slice()).to_string()
}

/// Collect variable refs and assignment offsets in a loop body.
fn collect_loop_refs(node: &Node, ref_names: &mut HashSet<String>, assign_offsets: &mut HashSet<usize>) {
    let mut collector = LoopRefCollector { ref_names, assign_offsets };
    collector.visit(node);
}

struct LoopRefCollector<'a> {
    ref_names: &'a mut HashSet<String>,
    assign_offsets: &'a mut HashSet<usize>,
}

impl Visit<'_> for LoopRefCollector<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.ref_names.insert(name);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        self.assign_offsets.insert(node.location().start_offset());
        // Also count as a ref if it's an op-assign target
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode,
    ) {
        let name = name_str(&node.name());
        self.ref_names.insert(name);
        self.assign_offsets.insert(node.location().start_offset());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableAndWriteNode,
    ) {
        let name = name_str(&node.name());
        self.ref_names.insert(name);
        self.assign_offsets.insert(node.location().start_offset());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOrWriteNode,
    ) {
        let name = name_str(&node.name());
        self.ref_names.insert(name);
        self.assign_offsets.insert(node.location().start_offset());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        self.assign_offsets.insert(node.location().start_offset());
    }

    // Don't descend into scopes
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

/// Check if any rescue clause in a begin node contains `retry`.
fn has_retry_in_rescue(begin: &ruby_prism::BeginNode) -> bool {
    let mut rc = begin.rescue_clause();
    while let Some(clause) = rc {
        if has_retry_in_node(&clause.as_node()) {
            return true;
        }
        rc = clause.subsequent();
    }
    false
}

fn has_retry_in_node(node: &Node) -> bool {
    let mut checker = RetryChecker { has_retry: false };
    checker.visit(node);
    checker.has_retry
}

struct RetryChecker {
    has_retry: bool,
}

impl Visit<'_> for RetryChecker {
    fn visit_retry_node(&mut self, _node: &ruby_prism::RetryNode) {
        self.has_retry = true;
    }
    fn visit_begin_node(&mut self, _node: &ruby_prism::BeginNode) {}
}
