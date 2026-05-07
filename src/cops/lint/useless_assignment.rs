use crate::cops::{CheckContext, Cop};
use crate::helpers::variable_force::{
    AssignmentKind, Scope, VariableForceDispatcher, VariableForceHook,
};
use crate::helpers::variable_force::suggestion::{find_suggestion, find_suggestion_from_methods};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

pub struct UselessAssignment;

impl UselessAssignment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for UselessAssignment {
    fn name(&self) -> &'static str {
        "Lint/UselessAssignment"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut hook = UselessAssignmentHook {
            ctx,
            offenses: Vec::new(),
        };
        let mut dispatcher = VariableForceDispatcher::new(&mut hook, ctx.source);
        dispatcher.investigate(node);
        hook.offenses
    }
}

struct UselessAssignmentHook<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> UselessAssignmentHook<'a> {
    /// Check if an assignment at the given offset is nested inside another
    /// assignment's value expression (chained assignment).
    /// Uses AST-based detection for accuracy.
    fn is_chained_inner_assignment(&self, name_start: usize) -> bool {
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut checker = ChainedAssignmentChecker {
            target_offset: name_start,
            in_direct_chain: false,
            found: false,
        };
        for stmt in program.statements().body().iter() {
            checker.visit(&stmt);
        }
        checker.found
    }

    /// Check if the assignment is at a position that's inside a loop condition.
    /// RuboCop skips assignments where the variable is used in a loop condition.
    fn is_variable_in_loop_condition(&self, name: &str, name_start: usize) -> bool {
        // Parse the source and find if this variable appears in any loop condition
        // that contains the assignment.
        let result = ruby_prism::parse(self.ctx.source.as_bytes());
        let root = result.node();
        let program = root.as_program_node().unwrap();
        let mut checker = LoopConditionChecker {
            target_name: name,
            assignment_offset: name_start,
            found: false,
        };
        for stmt in program.statements().body().iter() {
            checker.visit(&stmt);
        }
        checker.found
    }
}

impl<'a> VariableForceHook for UselessAssignmentHook<'a> {
    fn after_leaving_scope(&mut self, scope: &Scope, source: &str) {
        // Collect which assignment offsets are "inner" in a chained assignment
        // by checking all variables' assignments
        let mut ignored_offsets: std::collections::HashSet<usize> = std::collections::HashSet::new();

        // First pass: identify chained assignments
        for variable in scope.variables.values() {
            for assignment in &variable.assignments {
                if self.is_chained_inner_assignment(assignment.name_start) {
                    ignored_offsets.insert(assignment.name_start);
                }
            }
        }

        for variable in scope.variables.values() {
            if variable.should_be_unused() {
                continue;
            }

            // Process assignments in reverse (like RuboCop)
            for assignment in variable.assignments.iter().rev() {
                if assignment.used(variable.captured_by_block) {
                    continue;
                }

                // Skip if this is an inner chained assignment
                if ignored_offsets.contains(&assignment.name_start) {
                    continue;
                }

                // Skip if variable is used in a loop condition
                if self.is_variable_in_loop_condition(&assignment.name, assignment.name_start) {
                    continue;
                }

                let mut message = format!(
                    "Useless assignment to variable - `{}`.",
                    assignment.name
                );

                if assignment.kind == AssignmentKind::MultipleAssignment {
                    if let Some(suggestion) = find_suggestion_from_methods(&assignment.name, scope, source) {
                        message = format!(
                            "Useless assignment to variable - `{}`. Did you mean `{}`?",
                            assignment.name, suggestion
                        );
                    } else {
                        message = format!(
                            "Useless assignment to variable - `{}`. Use `_` or `_{}` as a variable name to indicate that it won't be used.",
                            assignment.name, assignment.name
                        );
                    }
                } else if let Some(suggestion) = find_suggestion(&assignment.name, scope, source) {
                    message = format!(
                        "Useless assignment to variable - `{}`. Did you mean `{}`?",
                        assignment.name, suggestion
                    );
                } else if let Some(ref op) = assignment.op {
                    message = format!(
                        "Useless assignment to variable - `{}`. Use `{}` instead of `{}=`.",
                        assignment.name, op, op
                    );
                }

                let (start, end) = if assignment.kind == AssignmentKind::RegexpNamedCapture {
                    (assignment.regexp_start, assignment.regexp_end)
                } else {
                    (assignment.name_start, assignment.name_end)
                };

                let mut offense = self.ctx.offense_with_range(
                    "Lint/UselessAssignment",
                    &message,
                    Severity::Warning,
                    start,
                    end,
                );

                let src_bytes = source.as_bytes();
                // Is this the sole assignment to this variable in scope?
                // (used to distinguish numblock `foo += _1` from loop `foo += 1`)
                let is_sole_assignment = variable.assignments.len() == 1;
                match assignment.kind {
                    AssignmentKind::Simple => {
                        // Check if this is a rescue `=> varname` pattern
                        if let Some(arrow_start) = find_rescue_arrow(src_bytes, assignment.name_start) {
                            // Delete ` => varname` (from arrow_start to name_end)
                            offense = offense.with_correction(
                                Correction::delete(arrow_start, assignment.name_end)
                            );
                        } else if let Some(value_start) =
                            find_simple_assignment_value_start(source, assignment.name_end)
                        {
                            offense = offense
                                .with_correction(Correction::delete(assignment.name_start, value_start));
                        }
                    }
                    AssignmentKind::MultipleAssignment => {
                        // Replace variable name with `_`
                        offense = offense.with_correction(
                            Correction::replace(assignment.name_start, assignment.name_end, String::from("_"))
                        );
                    }
                    AssignmentKind::OperatorAssignment => {
                        if assignment.op.is_some() {
                            // Use stored op_loc (binary_operator_loc from Prism)
                            let op_start = assignment.op_loc_start;
                            let op_end = assignment.op_loc_end;
                            if op_start < op_end {
                                if is_sole_assignment {
                                    // Numblock-like: sole assignment `foo += _1` → `foo + _1`
                                    // Delete `=` (the char right after the op)
                                    // op_end points right after `+`, so source[op_end] = `=`
                                    if op_end < src_bytes.len() && src_bytes[op_end] == b'=' {
                                        offense = offense.with_correction(Correction::delete(op_end, op_end + 1));
                                    }
                                } else {
                                    // Multi-assignment: `foo += expr` → `foo = expr`
                                    // Delete op char (op_start..op_end = `+`)
                                    // Also delete preceding space to avoid double-space
                                    let del_start = if op_start > 0 && src_bytes[op_start - 1] == b' ' {
                                        op_start - 1
                                    } else {
                                        op_start
                                    };
                                    offense = offense.with_correction(
                                        Correction::delete(del_start, op_end)
                                    );
                                }
                            }
                        }
                    }
                    AssignmentKind::RegexpNamedCapture => {
                        // Replace `(?<varname>` with `(?:` in the regexp source
                        let pattern = format!("(?<{}>", assignment.name);
                        let rs = assignment.regexp_start;
                        let re = assignment.regexp_end;
                        if re <= source.len() {
                            let regexp_src = &source[rs..re];
                            if let Some(pos) = regexp_src.find(&pattern) {
                                let abs_start = rs + pos;
                                let abs_end = abs_start + pattern.len();
                                offense = offense.with_correction(
                                    Correction::replace(abs_start, abs_end, String::from("(?:"))
                                );
                            }
                        }
                    }
                    AssignmentKind::OrAssignment | AssignmentKind::AndAssignment => {
                        // No safe simple correction for ||= / &&=
                    }
                }

                self.offenses.push(offense);

                // If this is a chained outer assignment, ignore inner ones
                // (RuboCop's ignore_node + chained_assignment? logic)
                // Check if the value of this assignment is another assignment
                if self.is_outer_chained_assignment(assignment.name_start, assignment.name_end) {
                    // Mark that we should skip the next variable's assignment
                    // at the chained position
                }
            }
        }
    }
}

impl<'a> UselessAssignmentHook<'a> {
    fn is_outer_chained_assignment(&self, _name_start: usize, name_end: usize) -> bool {
        // Check if after `name =` there's another assignment
        let source = self.ctx.source.as_bytes();
        let mut i = name_end;
        // Skip whitespace and `=`
        while i < source.len() && (source[i] == b' ' || source[i] == b'\t') {
            i += 1;
        }
        if i < source.len() && source[i] == b'=' {
            i += 1;
            while i < source.len() && (source[i] == b' ' || source[i] == b'\t') {
                i += 1;
            }
            // Check if what follows looks like a variable assignment
            if i < source.len() && (source[i].is_ascii_lowercase() || source[i] == b'_') {
                return true;
            }
        }
        false
    }
}

/// Checks if a variable with the given name is used in a loop condition
/// that contains the assignment at the given offset.
struct LoopConditionChecker<'a> {
    target_name: &'a str,
    assignment_offset: usize,
    found: bool,
}

impl<'a> LoopConditionChecker<'a> {
    fn check_condition_for_var(&self, condition: &ruby_prism::Node) -> bool {
        let mut finder = VarNameFinder {
            name: self.target_name,
            found: false,
        };
        finder.visit(condition);
        finder.found
    }

    fn contains_offset(&self, node: &ruby_prism::Node) -> bool {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.assignment_offset >= start && self.assignment_offset < end
    }

    /// Check if the assignment is directly in the loop (no scope boundary between).
    fn directly_contains_offset(&self, node: &ruby_prism::Node) -> bool {
        if !self.contains_offset(node) {
            return false;
        }
        // Check there's no def/class/module between the loop and the assignment
        let mut checker = ScopeBoundaryChecker {
            target_offset: self.assignment_offset,
            found_boundary: false,
        };
        checker.visit(node);
        !checker.found_boundary
    }
}

struct ScopeBoundaryChecker {
    target_offset: usize,
    found_boundary: bool,
}

impl Visit<'_> for ScopeBoundaryChecker {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        if self.target_offset >= start && self.target_offset < end {
            self.found_boundary = true;
        }
    }
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        if self.target_offset >= start && self.target_offset < end {
            self.found_boundary = true;
        }
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        if self.target_offset >= start && self.target_offset < end {
            self.found_boundary = true;
        }
    }
    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        if self.target_offset >= start && self.target_offset < end {
            self.found_boundary = true;
        }
    }
}

impl<'a> Visit<'_> for LoopConditionChecker<'a> {
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if self.directly_contains_offset(&node.as_node()) {
            if self.check_condition_for_var(&node.predicate()) {
                self.found = true;
                return;
            }
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if self.directly_contains_offset(&node.as_node()) {
            if self.check_condition_for_var(&node.predicate()) {
                self.found = true;
                return;
            }
        }
        ruby_prism::visit_until_node(self, node);
    }

    // Don't descend into scope-creating nodes (the assignment's variable
    // is in a different scope than the loop condition's variable)
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

/// Detects if a LocalVariableWriteNode at target_offset is directly nested
/// inside another LocalVariableWriteNode's value (chained assignment like
/// `foo = bar = expr` or `foo = -bar = expr`). Does NOT flag assignments
/// inside arrays, hashes, arguments, etc.
struct ChainedAssignmentChecker {
    target_offset: usize,
    /// We're inside another LocalVariableWriteNode's value subtree,
    /// and haven't crossed any "container" boundary (array, hash, args, etc.)
    in_direct_chain: bool,
    found: bool,
}

impl Visit<'_> for ChainedAssignmentChecker {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        if node.name_loc().start_offset() == self.target_offset && self.in_direct_chain {
            self.found = true;
            return;
        }
        let was = self.in_direct_chain;
        self.in_direct_chain = true;
        ruby_prism::visit_local_variable_write_node(self, node);
        self.in_direct_chain = was;
    }

    // Container nodes break the direct chain
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let was = self.in_direct_chain;
        self.in_direct_chain = false;
        ruby_prism::visit_array_node(self, node);
        self.in_direct_chain = was;
    }
    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        let was = self.in_direct_chain;
        self.in_direct_chain = false;
        ruby_prism::visit_hash_node(self, node);
        self.in_direct_chain = was;
    }
    fn visit_arguments_node(&mut self, node: &ruby_prism::ArgumentsNode) {
        let was = self.in_direct_chain;
        self.in_direct_chain = false;
        ruby_prism::visit_arguments_node(self, node);
        self.in_direct_chain = was;
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let was = self.in_direct_chain;
        self.in_direct_chain = false;
        ruby_prism::visit_block_node(self, node);
        self.in_direct_chain = was;
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        let was = self.in_direct_chain;
        self.in_direct_chain = false;
        ruby_prism::visit_lambda_node(self, node);
        self.in_direct_chain = was;
    }
}

struct VarNameFinder<'a> {
    name: &'a str,
    found: bool,
}

impl<'a> Visit<'_> for VarNameFinder<'a> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = node_name!(node).to_string();
        if name == self.name {
            self.found = true;
        }
    }
}

/// For a simple `name = expr` assignment, given the end offset of `name`,
/// scan forward past whitespace, then `=`, then whitespace, and return the
/// byte offset where the value expression begins. Returns None if the bytes
/// after `name_end` don't match the simple-assignment shape (e.g. op-assign
/// `name +=`, or no `=` found).
/// Find the start of ` => ` pattern before a rescue variable name.
/// Returns the position of the space before `=>` (inclusive) if found.
fn find_rescue_arrow(source: &[u8], name_start: usize) -> Option<usize> {
    // Scan backwards from name_start looking for `=>`
    // Expected pattern: `rescue ExcType => varname` or `rescue => varname`
    // We want to find ` => ` ending at name_start
    if name_start < 4 { return None; }
    // name_start points to first char of varname. Before it: `> ` or `>=`?
    // Layout: `... => varname` where `varname` starts at name_start
    // The `>` is at name_start-2, ` ` at name_start-1 (space before varname), `>` ...
    // Actually: `=> varname`: `=` at K, `>` at K+1, ` ` at K+2, varname at K+3
    // So name_start = K+3, K+2 = ` `, K+1 = `>`, K = `=`
    let mut j = name_start;
    // Skip space before varname
    while j > 0 && source[j-1] == b' ' { j -= 1; }
    // Now should be at `>`
    if j < 2 { return None; }
    if source[j-1] != b'>' || source[j-2] != b'=' { return None; }
    // j-2 = start of `=>`, skip space before `=>`
    let arrow = j - 2;
    let mut k = arrow;
    while k > 0 && source[k-1] == b' ' { k -= 1; }
    Some(k)
}

/// Find the position of `=` in an operator assignment like `foo += expr`.
/// Returns the byte offset of `=` (the assignment operator).
fn find_op_assign_eq(source: &[u8], name_end: usize) -> Option<usize> {
    let mut i = name_end;
    // Skip whitespace
    while i < source.len() && (source[i] == b' ' || source[i] == b'\t') { i += 1; }
    // Skip operator chars (not `=`)
    while i < source.len() && source[i] != b'=' && source[i] != b'\n' { i += 1; }
    if i < source.len() && source[i] == b'=' { Some(i) } else { None }
}

fn find_simple_assignment_value_start(source: &str, name_end: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = name_end;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'=' {
        return None;
    }
    // Reject op-assigns (`==`, `+=` already filtered by AssignmentKind, but be safe).
    if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
        return None;
    }
    i += 1;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    Some(i)
}

crate::register_cop!("Lint/UselessAssignment", |_cfg| {
    Some(Box::new(UselessAssignment::new()))
});
