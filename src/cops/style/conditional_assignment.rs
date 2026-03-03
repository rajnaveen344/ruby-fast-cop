//! Style/ConditionalAssignment - Checks for consistent assignment placement relative to conditionals.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/conditional_assignment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/ConditionalAssignment";
const ASSIGN_TO_CONDITION_MSG: &str = "Assign variables inside of conditionals.";
const MSG: &str =
    "Use the return of the conditional for variable assignment and comparison.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    AssignInsideCondition,
    AssignToCondition,
}

pub struct ConditionalAssignment {
    enforced_style: EnforcedStyle,
    include_ternary_expressions: bool,
    single_line_conditions_only: bool,
    /// Max line length for correction-exceeds-line-limit check.
    /// Defaults to 80 (matching common Ruby convention and RuboCop test setups).
    /// In production, this should be read from Layout/LineLength Max config.
    max_line_length: usize,
}

impl ConditionalAssignment {
    pub fn new(style: EnforcedStyle) -> Self {
        Self {
            enforced_style: style,
            include_ternary_expressions: true,
            single_line_conditions_only: true,
            max_line_length: 80,
        }
    }

    pub fn with_config(
        style: EnforcedStyle,
        include_ternary: bool,
        single_line_only: bool,
    ) -> Self {
        Self {
            enforced_style: style,
            include_ternary_expressions: include_ternary,
            single_line_conditions_only: single_line_only,
            max_line_length: 80,
        }
    }
}

impl Cop for ConditionalAssignment {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = ConditionalAssignmentVisitor {
            source: ctx.source,
            enforced_style: self.enforced_style,
            include_ternary: self.include_ternary_expressions,
            single_line_only: self.single_line_conditions_only,
            max_line_length: self.max_line_length,
            offenses: Vec::new(),
            filename: ctx.filename,
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct ConditionalAssignmentVisitor<'a> {
    source: &'a str,
    enforced_style: EnforcedStyle,
    include_ternary: bool,
    single_line_only: bool,
    max_line_length: usize,
    offenses: Vec<Offense>,
    filename: &'a str,
}

/// Info about an assignment: (lhs_string, kind_string)
/// E.g. ("bar = ", "lvasgn") for `bar = ...`
type AssignmentInfo = (String, String);

/// Info about a branch for assign_to_condition checking
struct BranchInfo {
    /// Number of statements in the branch
    stmt_count: usize,
    /// Assignment info of the last statement (if it is an assignment)
    tail_assignment: Option<AssignmentInfo>,
}

impl<'a> ConditionalAssignmentVisitor<'a> {
    fn src(&self, start: usize, end: usize) -> &'a str {
        &self.source[start..end]
    }

    fn add_offense(&mut self, start_offset: usize, end_offset: usize, message: &str) {
        // RuboCop highlights the first line of the offense range.
        // If the range spans multiple lines, clamp end_offset to end of first line.
        let effective_end = self.first_line_end(start_offset, end_offset);
        let location = Location::from_offsets(self.source, start_offset, effective_end);
        self.offenses.push(Offense::new(
            COP_NAME,
            message,
            Severity::Convention,
            location,
            self.filename,
        ));
    }

    /// Given a range [start..end], find the end of the first line starting from start.
    /// If a newline exists between start and end, returns the offset of the newline.
    /// Otherwise returns end.
    fn first_line_end(&self, start: usize, end: usize) -> usize {
        if let Some(nl_pos) = self.source[start..end].find('\n') {
            start + nl_pos
        } else {
            end
        }
    }

    // =====================
    // assign_inside_condition: detects `LHS = if/unless/case/ternary` and flags it
    // =====================

    fn check_assign_inside_condition(&mut self, node: &Node) {
        // Get the RHS of the assignment as offsets + type info
        let (assign_start, assign_end, rhs_start, rhs_end) = match self.get_assignment_rhs_offsets(node) {
            Some(v) => v,
            None => return,
        };

        // Get the RHS node from the source to check its type
        // We need to re-parse or check the actual RHS node.
        // Better approach: check the RHS node type directly.
        self.check_rhs_for_conditional(node, assign_start, assign_end, rhs_start, rhs_end);
    }

    /// Check the RHS of an assignment node for conditionals.
    /// This avoids lifetime issues by working with the node inline.
    fn check_rhs_for_conditional(
        &mut self,
        node: &Node,
        assign_start: usize,
        assign_end: usize,
        _rhs_start: usize,
        _rhs_end: usize,
    ) {
        // Extract the RHS node from the assignment
        let rhs = match self.extract_rhs_node(node) {
            Some(r) => r,
            None => return,
        };

        // Unwrap single paren wrapping
        let rhs_inner = self.get_paren_inner(&rhs);
        let check_node = rhs_inner.as_ref().unwrap_or(&rhs);

        match check_node {
            Node::IfNode { .. } => {
                let if_node = check_node.as_if_node().unwrap();
                if self.is_ternary(&if_node) {
                    if !self.include_ternary {
                        return;
                    }
                    self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
                    return;
                }
                if !self.if_has_else_or_elsif(&if_node) {
                    return;
                }
                if self.single_line_only && self.if_has_multiline_branch(&if_node) {
                    return;
                }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            Node::UnlessNode { .. } => {
                let unless_node = check_node.as_unless_node().unwrap();
                // For assign_inside_condition, unless without else IS still flagged.
                // In RuboCop's parser gem AST, `unless cond; body; end` is represented as
                // `if(cond, nil, body)` - the body is in the else position, so `else_branch`
                // is truthy and the check proceeds. We replicate that here by not requiring
                // else_clause for unless.
                if self.single_line_only && self.unless_has_multiline_branch(&unless_node) {
                    return;
                }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            Node::CaseNode { .. } => {
                let case_node = check_node.as_case_node().unwrap();
                if case_node.else_clause().is_none() {
                    return;
                }
                if self.single_line_only && self.case_has_multiline_branch(&case_node) {
                    return;
                }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            Node::CaseMatchNode { .. } => {
                let cm = check_node.as_case_match_node().unwrap();
                if cm.else_clause().is_none() {
                    return;
                }
                if self.single_line_only && self.case_match_has_multiline_branch(&cm) {
                    return;
                }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            _ => {}
        }
    }

    /// Extract the raw RHS node from an assignment node.
    /// Returns None for non-assignment nodes.
    fn extract_rhs_node<'b>(&self, node: &'b Node) -> Option<Node<'b>> {
        match node {
            Node::LocalVariableWriteNode { .. } => Some(node.as_local_variable_write_node().unwrap().value()),
            Node::InstanceVariableWriteNode { .. } => Some(node.as_instance_variable_write_node().unwrap().value()),
            Node::ClassVariableWriteNode { .. } => Some(node.as_class_variable_write_node().unwrap().value()),
            Node::GlobalVariableWriteNode { .. } => Some(node.as_global_variable_write_node().unwrap().value()),
            Node::ConstantWriteNode { .. } => Some(node.as_constant_write_node().unwrap().value()),
            Node::ConstantPathWriteNode { .. } => Some(node.as_constant_path_write_node().unwrap().value()),
            Node::LocalVariableOperatorWriteNode { .. } => Some(node.as_local_variable_operator_write_node().unwrap().value()),
            Node::InstanceVariableOperatorWriteNode { .. } => Some(node.as_instance_variable_operator_write_node().unwrap().value()),
            Node::ClassVariableOperatorWriteNode { .. } => Some(node.as_class_variable_operator_write_node().unwrap().value()),
            Node::GlobalVariableOperatorWriteNode { .. } => Some(node.as_global_variable_operator_write_node().unwrap().value()),
            Node::ConstantOperatorWriteNode { .. } => Some(node.as_constant_operator_write_node().unwrap().value()),
            Node::ConstantPathOperatorWriteNode { .. } => Some(node.as_constant_path_operator_write_node().unwrap().value()),
            Node::LocalVariableAndWriteNode { .. } => Some(node.as_local_variable_and_write_node().unwrap().value()),
            Node::InstanceVariableAndWriteNode { .. } => Some(node.as_instance_variable_and_write_node().unwrap().value()),
            Node::ClassVariableAndWriteNode { .. } => Some(node.as_class_variable_and_write_node().unwrap().value()),
            Node::GlobalVariableAndWriteNode { .. } => Some(node.as_global_variable_and_write_node().unwrap().value()),
            Node::ConstantAndWriteNode { .. } => Some(node.as_constant_and_write_node().unwrap().value()),
            Node::ConstantPathAndWriteNode { .. } => Some(node.as_constant_path_and_write_node().unwrap().value()),
            Node::LocalVariableOrWriteNode { .. } => Some(node.as_local_variable_or_write_node().unwrap().value()),
            Node::InstanceVariableOrWriteNode { .. } => Some(node.as_instance_variable_or_write_node().unwrap().value()),
            Node::ClassVariableOrWriteNode { .. } => Some(node.as_class_variable_or_write_node().unwrap().value()),
            Node::GlobalVariableOrWriteNode { .. } => Some(node.as_global_variable_or_write_node().unwrap().value()),
            Node::ConstantOrWriteNode { .. } => Some(node.as_constant_or_write_node().unwrap().value()),
            Node::ConstantPathOrWriteNode { .. } => Some(node.as_constant_path_or_write_node().unwrap().value()),
            Node::MultiWriteNode { .. } => Some(node.as_multi_write_node().unwrap().value()),
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if self.is_assignment_call(&call) {
                    call.arguments().and_then(|a| {
                        let args: Vec<Node> = a.arguments().iter().collect();
                        args.into_iter().last()
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// If the node is a ParenthesesNode wrapping a single statement, return that inner node.
    fn get_paren_inner<'b>(&self, node: &'b Node) -> Option<Node<'b>> {
        if let Node::ParenthesesNode { .. } = node {
            let paren = node.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Node::StatementsNode { .. } = &body {
                    let stmts = body.as_statements_node().unwrap();
                    let mut iter = stmts.body().iter();
                    let first = iter.next();
                    let second = iter.next();
                    if second.is_none() {
                        return first;
                    }
                } else {
                    return Some(body);
                }
            }
        }
        None
    }

    /// Get assignment extent offsets: (assign_start, assign_end, rhs_start, rhs_end)
    fn get_assignment_rhs_offsets(&self, node: &Node) -> Option<(usize, usize, usize, usize)> {
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        match node {
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::InstanceVariableWriteNode { .. } => {
                let n = node.as_instance_variable_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ClassVariableWriteNode { .. } => {
                let n = node.as_class_variable_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::GlobalVariableWriteNode { .. } => {
                let n = node.as_global_variable_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantWriteNode { .. } => {
                let n = node.as_constant_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantPathWriteNode { .. } => {
                let n = node.as_constant_path_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::LocalVariableOperatorWriteNode { .. } => {
                let n = node.as_local_variable_operator_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::InstanceVariableOperatorWriteNode { .. } => {
                let n = node.as_instance_variable_operator_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ClassVariableOperatorWriteNode { .. } => {
                let n = node.as_class_variable_operator_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::GlobalVariableOperatorWriteNode { .. } => {
                let n = node.as_global_variable_operator_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantOperatorWriteNode { .. } => {
                let n = node.as_constant_operator_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantPathOperatorWriteNode { .. } => {
                let n = node.as_constant_path_operator_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::LocalVariableAndWriteNode { .. } => {
                let n = node.as_local_variable_and_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::InstanceVariableAndWriteNode { .. } => {
                let n = node.as_instance_variable_and_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ClassVariableAndWriteNode { .. } => {
                let n = node.as_class_variable_and_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::GlobalVariableAndWriteNode { .. } => {
                let n = node.as_global_variable_and_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantAndWriteNode { .. } => {
                let n = node.as_constant_and_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantPathAndWriteNode { .. } => {
                let n = node.as_constant_path_and_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::LocalVariableOrWriteNode { .. } => {
                let n = node.as_local_variable_or_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::InstanceVariableOrWriteNode { .. } => {
                let n = node.as_instance_variable_or_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ClassVariableOrWriteNode { .. } => {
                let n = node.as_class_variable_or_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::GlobalVariableOrWriteNode { .. } => {
                let n = node.as_global_variable_or_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantOrWriteNode { .. } => {
                let n = node.as_constant_or_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::ConstantPathOrWriteNode { .. } => {
                let n = node.as_constant_path_or_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::MultiWriteNode { .. } => {
                let n = node.as_multi_write_node().unwrap();
                let v = n.value();
                Some((start, end, v.location().start_offset(), v.location().end_offset()))
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if self.is_assignment_call(&call) {
                    if let Some(args) = call.arguments() {
                        let mut last_end = 0usize;
                        let mut last_start = 0usize;
                        for arg in args.arguments().iter() {
                            last_start = arg.location().start_offset();
                            last_end = arg.location().end_offset();
                        }
                        if last_end > 0 {
                            return Some((start, end, last_start, last_end));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn is_assignment_call(&self, call: &ruby_prism::CallNode) -> bool {
        let name = std::str::from_utf8(call.name().as_slice()).unwrap_or("");
        matches!(name, "[]=" | "<<" | "=~" | "!~" | "<=>" | "<" | ">" | "==" | "!=" | "===" | ">=" | "<=")
            || (name.ends_with('=') && name.len() > 1 && !matches!(name, "!=" | "==" | "===" | ">=" | "<="))
    }

    /// If node has an else branch or an elsif (with or without a final else)
    fn if_has_else_or_elsif(&self, node: &ruby_prism::IfNode) -> bool {
        node.subsequent().is_some()
    }

    fn is_ternary(&self, node: &ruby_prism::IfNode) -> bool {
        let start = node.location().start_offset();
        !self.source[start..].starts_with("if")
    }

    // ---- Multiline branch detection ----

    fn stmts_count(stmts: &Option<ruby_prism::StatementsNode>) -> usize {
        match stmts {
            Some(s) => s.body().iter().count(),
            None => 0,
        }
    }

    fn if_has_multiline_branch(&self, node: &ruby_prism::IfNode) -> bool {
        if Self::stmts_count(&node.statements()) > 1 {
            return true;
        }
        let mut sub = node.subsequent();
        while let Some(s) = sub {
            match &s {
                Node::IfNode { .. } => {
                    let elsif = s.as_if_node().unwrap();
                    if Self::stmts_count(&elsif.statements()) > 1 {
                        return true;
                    }
                    sub = elsif.subsequent();
                }
                Node::ElseNode { .. } => {
                    let else_node = s.as_else_node().unwrap();
                    if Self::stmts_count(&else_node.statements()) > 1 {
                        return true;
                    }
                    break;
                }
                _ => break,
            }
        }
        false
    }

    fn unless_has_multiline_branch(&self, node: &ruby_prism::UnlessNode) -> bool {
        if Self::stmts_count(&node.statements()) > 1 {
            return true;
        }
        if let Some(ec) = node.else_clause() {
            if Self::stmts_count(&ec.statements()) > 1 {
                return true;
            }
        }
        false
    }

    fn case_has_multiline_branch(&self, node: &ruby_prism::CaseNode) -> bool {
        for cond in node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                let when = cond.as_when_node().unwrap();
                if Self::stmts_count(&when.statements()) > 1 {
                    return true;
                }
            }
        }
        if let Some(ec) = node.else_clause() {
            if Self::stmts_count(&ec.statements()) > 1 {
                return true;
            }
        }
        false
    }

    fn case_match_has_multiline_branch(&self, node: &ruby_prism::CaseMatchNode) -> bool {
        for cond in node.conditions().iter() {
            if let Node::InNode { .. } = &cond {
                let in_node = cond.as_in_node().unwrap();
                if Self::stmts_count(&in_node.statements()) > 1 {
                    return true;
                }
            }
        }
        if let Some(ec) = node.else_clause() {
            if Self::stmts_count(&ec.statements()) > 1 {
                return true;
            }
        }
        false
    }

    // =====================
    // assign_to_condition: detects conditionals where all branches end with the same assignment
    // =====================

    /// Extract BranchInfo from a StatementsNode option
    fn branch_info_from_stmts(&self, stmts: &Option<ruby_prism::StatementsNode>) -> BranchInfo {
        match stmts {
            Some(s) => {
                let mut count = 0usize;
                let mut last_node_info: Option<AssignmentInfo> = None;
                for node in s.body().iter() {
                    count += 1;
                    last_node_info = self.extract_assignment_lhs(&node);
                }
                BranchInfo {
                    stmt_count: count,
                    tail_assignment: last_node_info,
                }
            }
            None => BranchInfo {
                stmt_count: 0,
                tail_assignment: None,
            },
        }
    }

    fn check_assign_to_condition_if(&mut self, node: &ruby_prism::IfNode) {
        // Skip elsif nodes (handled as part of parent) - check before ternary
        if self.is_elsif(node) {
            return;
        }

        if self.is_ternary(node) {
            if !self.include_ternary {
                return;
            }
            self.check_ternary_assign_to_condition(node);
            return;
        }

        // Collect branch infos without storing Node references
        let branch_infos = self.collect_if_branch_infos(node);
        if branch_infos.is_empty() {
            return;
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_branch_infos_same_assignment(start, end, &branch_infos);
    }

    fn check_assign_to_condition_unless(&mut self, node: &ruby_prism::UnlessNode) {
        if node.else_clause().is_none() {
            return;
        }

        let mut branch_infos = Vec::new();
        let info = self.branch_info_from_stmts(&node.statements());
        if info.stmt_count == 0 && info.tail_assignment.is_none() {
            // Empty body with no assignment - RuboCop still processes this
        }
        branch_infos.push(info);

        if let Some(ec) = node.else_clause() {
            branch_infos.push(self.branch_info_from_stmts(&ec.statements()));
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_branch_infos_same_assignment(start, end, &branch_infos);
    }

    fn check_assign_to_condition_case(&mut self, node: &ruby_prism::CaseNode) {
        if node.else_clause().is_none() {
            return;
        }

        let mut branch_infos = Vec::new();
        for cond in node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                let when = cond.as_when_node().unwrap();
                let info = self.branch_info_from_stmts(&when.statements());
                if info.tail_assignment.is_none() {
                    return; // Branch without assignment at tail - can't consolidate
                }
                branch_infos.push(info);
            }
        }
        if let Some(ec) = node.else_clause() {
            let info = self.branch_info_from_stmts(&ec.statements());
            if info.tail_assignment.is_none() {
                return;
            }
            branch_infos.push(info);
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_branch_infos_same_assignment(start, end, &branch_infos);
    }

    fn check_assign_to_condition_case_match(&mut self, node: &ruby_prism::CaseMatchNode) {
        if node.else_clause().is_none() {
            return;
        }

        let mut branch_infos = Vec::new();
        for cond in node.conditions().iter() {
            if let Node::InNode { .. } = &cond {
                let in_node = cond.as_in_node().unwrap();
                let info = self.branch_info_from_stmts(&in_node.statements());
                if info.tail_assignment.is_none() {
                    return;
                }
                branch_infos.push(info);
            }
        }
        if let Some(ec) = node.else_clause() {
            let info = self.branch_info_from_stmts(&ec.statements());
            if info.tail_assignment.is_none() {
                return;
            }
            branch_infos.push(info);
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_branch_infos_same_assignment(start, end, &branch_infos);
    }

    fn check_ternary_assign_to_condition(&mut self, node: &ruby_prism::IfNode) {
        let if_info = self.branch_info_from_stmts(&node.statements());
        if if_info.stmt_count != 1 { return; }

        let else_info = match node.subsequent() {
            Some(sub) => {
                match &sub {
                    Node::ElseNode { .. } => {
                        let el = sub.as_else_node().unwrap();
                        let info = self.branch_info_from_stmts(&el.statements());
                        if info.stmt_count != 1 { return; }
                        info
                    }
                    _ => return,
                }
            }
            None => return,
        };

        let branch_infos = vec![if_info, else_info];
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_branch_infos_same_assignment(start, end, &branch_infos);
    }

    fn is_elsif(&self, node: &ruby_prism::IfNode) -> bool {
        // Check the if_keyword_loc to determine if this is "if" or "elsif"
        let kw_start = node.if_keyword_loc().map(|loc| loc.start_offset());
        if let Some(kw_start) = kw_start {
            return self.source[kw_start..].starts_with("elsif");
        }
        // No keyword loc means modifier if or ternary, not elsif
        false
    }

    /// Collect branch infos for an if/elsif/else chain.
    /// Returns empty vec if the chain doesn't have a final else (incomplete).
    fn collect_if_branch_infos(&self, node: &ruby_prism::IfNode) -> Vec<BranchInfo> {
        let mut infos = Vec::new();

        // If body
        infos.push(self.branch_info_from_stmts(&node.statements()));

        // Walk subsequent chain
        let mut sub = node.subsequent();
        loop {
            match sub {
                None => return vec![], // No else at all - not complete
                Some(s) => {
                    match &s {
                        Node::IfNode { .. } => {
                            let elsif = s.as_if_node().unwrap();
                            infos.push(self.branch_info_from_stmts(&elsif.statements()));
                            sub = elsif.subsequent();
                        }
                        Node::ElseNode { .. } => {
                            let else_node = s.as_else_node().unwrap();
                            infos.push(self.branch_info_from_stmts(&else_node.statements()));
                            break;
                        }
                        _ => return vec![],
                    }
                }
            }
        }

        infos
    }

    fn check_branch_infos_same_assignment(
        &mut self,
        cond_start: usize,
        cond_end: usize,
        branches: &[BranchInfo],
    ) {
        if branches.is_empty() {
            return;
        }

        // All branches must have a tail assignment
        let mut first_lhs: Option<&str> = None;
        let mut first_kind: Option<&str> = None;

        for branch in branches {
            let (lhs, kind) = match &branch.tail_assignment {
                Some(a) => a,
                None => return,
            };

            if let Some(fl) = first_lhs {
                if fl != lhs {
                    return;
                }
            } else {
                first_lhs = Some(lhs);
            }

            if let Some(fk) = first_kind {
                if fk != kind {
                    return;
                }
            } else {
                first_kind = Some(kind);
            }
        }

        // Check single_line_only
        if self.single_line_only {
            for branch in branches {
                if branch.stmt_count > 1 {
                    return;
                }
            }
        }

        // Check if correction would exceed max line length.
        // RuboCop skips the offense if pulling the assignment out would create a line
        // longer than Layout/LineLength Max.
        if let Some(assignment_lhs) = first_lhs {
            if self.correction_exceeds_line_limit(cond_start, cond_end, assignment_lhs) {
                return;
            }
        }

        self.add_offense(cond_start, cond_end, MSG);
    }

    /// Check if the corrected form (assignment pulled out) would exceed the max line length.
    /// Implements RuboCop's `correction_exceeds_line_limit?` logic:
    /// 1. Take each line of the conditional source
    /// 2. Strip the assignment LHS from each line (since after correction it won't be there)
    /// 3. Find the longest resulting line
    /// 4. Prepend the assignment LHS to get the corrected longest line
    /// 5. Check if it exceeds max_line_length
    fn correction_exceeds_line_limit(
        &self,
        cond_start: usize,
        cond_end: usize,
        assignment_lhs: &str,
    ) -> bool {
        let cond_source = &self.source[cond_start..cond_end];

        // Build a pattern to strip the assignment from lines.
        // RuboCop uses: /\s*#{Regexp.escape(assignment).gsub('\ ', '\s*')}/
        // We simplify: strip leading whitespace + the assignment text (allowing flexible spaces)
        let assignment_trimmed = assignment_lhs.trim();

        let mut longest_stripped_len = 0usize;
        for line in cond_source.lines() {
            let chomped = line.trim_end_matches('\r');
            // Try to strip the assignment from this line
            let stripped = self.strip_assignment_from_line(chomped, assignment_trimmed);
            if stripped.len() > longest_stripped_len {
                longest_stripped_len = stripped.len();
            }
        }

        // The corrected longest line would be: assignment_lhs + longest_stripped_line
        let corrected_len = assignment_lhs.len() + longest_stripped_len;
        corrected_len > self.max_line_length
    }

    /// Strip the assignment LHS pattern from a line (first occurrence only).
    /// Handles flexible whitespace around the assignment operator.
    fn strip_assignment_from_line<'b>(&self, line: &'b str, assignment_trimmed: &str) -> String {
        // Try to find and remove `\s*assignment_trimmed\s*` pattern from the line
        // Split the assignment into parts around spaces for flexible matching
        if let Some(pos) = line.find(assignment_trimmed) {
            // Find the start position including leading whitespace
            let mut start = pos;
            while start > 0 && line.as_bytes()[start - 1] == b' ' {
                start -= 1;
            }
            let end = pos + assignment_trimmed.len();
            // Skip trailing spaces too
            let mut end_adj = end;
            while end_adj < line.len() && line.as_bytes()[end_adj] == b' ' {
                end_adj += 1;
            }
            format!("{}{}", &line[..start], &line[end_adj..])
        } else {
            line.to_string()
        }
    }

    fn extract_assignment_lhs(&self, node: &Node) -> Option<AssignmentInfo> {
        match node {
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} = ", name), "lvasgn".to_string()))
            }
            Node::InstanceVariableWriteNode { .. } => {
                let n = node.as_instance_variable_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} = ", name), "ivasgn".to_string()))
            }
            Node::ClassVariableWriteNode { .. } => {
                let n = node.as_class_variable_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} = ", name), "cvasgn".to_string()))
            }
            Node::GlobalVariableWriteNode { .. } => {
                let n = node.as_global_variable_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} = ", name), "gvasgn".to_string()))
            }
            Node::ConstantWriteNode { .. } => {
                let n = node.as_constant_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} = ", name), "casgn".to_string()))
            }
            Node::ConstantPathWriteNode { .. } => {
                let n = node.as_constant_path_write_node().unwrap();
                let target = n.target();
                let target_src = self.src(target.location().start_offset(), target.location().end_offset());
                Some((format!("{} = ", target_src), "casgn".to_string()))
            }
            // Op assigns
            Node::LocalVariableOperatorWriteNode { .. } => {
                let n = node.as_local_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some((format!("{} {}= ", name, op), "op_asgn".to_string()))
            }
            Node::InstanceVariableOperatorWriteNode { .. } => {
                let n = node.as_instance_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some((format!("{} {}= ", name, op), "op_asgn".to_string()))
            }
            Node::ClassVariableOperatorWriteNode { .. } => {
                let n = node.as_class_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some((format!("{} {}= ", name, op), "op_asgn".to_string()))
            }
            Node::GlobalVariableOperatorWriteNode { .. } => {
                let n = node.as_global_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some((format!("{} {}= ", name, op), "op_asgn".to_string()))
            }
            Node::ConstantOperatorWriteNode { .. } => {
                let n = node.as_constant_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some((format!("{} {}= ", name, op), "op_asgn".to_string()))
            }
            // And assigns
            Node::LocalVariableAndWriteNode { .. } => {
                let n = node.as_local_variable_and_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} &&= ", name), "and_asgn".to_string()))
            }
            Node::InstanceVariableAndWriteNode { .. } => {
                let n = node.as_instance_variable_and_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} &&= ", name), "and_asgn".to_string()))
            }
            Node::ClassVariableAndWriteNode { .. } => {
                let n = node.as_class_variable_and_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} &&= ", name), "and_asgn".to_string()))
            }
            Node::GlobalVariableAndWriteNode { .. } => {
                let n = node.as_global_variable_and_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} &&= ", name), "and_asgn".to_string()))
            }
            Node::ConstantAndWriteNode { .. } => {
                let n = node.as_constant_and_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} &&= ", name), "and_asgn".to_string()))
            }
            Node::ConstantPathAndWriteNode { .. } => {
                let n = node.as_constant_path_and_write_node().unwrap();
                let target = n.target();
                let target_src = self.src(target.location().start_offset(), target.location().end_offset());
                Some((format!("{} &&= ", target_src), "and_asgn".to_string()))
            }
            // Or assigns
            Node::LocalVariableOrWriteNode { .. } => {
                let n = node.as_local_variable_or_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} ||= ", name), "or_asgn".to_string()))
            }
            Node::InstanceVariableOrWriteNode { .. } => {
                let n = node.as_instance_variable_or_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} ||= ", name), "or_asgn".to_string()))
            }
            Node::ClassVariableOrWriteNode { .. } => {
                let n = node.as_class_variable_or_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} ||= ", name), "or_asgn".to_string()))
            }
            Node::GlobalVariableOrWriteNode { .. } => {
                let n = node.as_global_variable_or_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} ||= ", name), "or_asgn".to_string()))
            }
            Node::ConstantOrWriteNode { .. } => {
                let n = node.as_constant_or_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} ||= ", name), "or_asgn".to_string()))
            }
            Node::ConstantPathOrWriteNode { .. } => {
                let n = node.as_constant_path_or_write_node().unwrap();
                let target = n.target();
                let target_src = self.src(target.location().start_offset(), target.location().end_offset());
                Some((format!("{} ||= ", target_src), "or_asgn".to_string()))
            }
            // Send-based
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let name_str = std::str::from_utf8(call.name().as_slice()).unwrap_or("");

                if !self.is_assignment_call(&call) {
                    return None;
                }

                let receiver_src = call.receiver().map(|r| {
                    self.src(r.location().start_offset(), r.location().end_offset()).to_string()
                }).unwrap_or_default();

                if name_str == "[]=" {
                    let args: Vec<(usize, usize)> = call.arguments()
                        .map(|a| a.arguments().iter().map(|arg| (arg.location().start_offset(), arg.location().end_offset())).collect())
                        .unwrap_or_default();
                    if args.len() >= 2 {
                        let indices: Vec<String> = args[..args.len()-1].iter().map(|&(s, e)| {
                            self.src(s, e).to_string()
                        }).collect();
                        return Some((format!("{}[{}] = ", receiver_src, indices.join(", ")), "send_[]=".to_string()));
                    }
                    return None;
                }

                if name_str == "<<" {
                    return Some((format!("{} << ", receiver_src), "send_<<".to_string()));
                }

                if matches!(name_str, "==" | "!=" | "===" | "=~" | "!~" | "<=>" | "<" | ">" | ">=" | "<=") {
                    return Some((format!("{} {} ", receiver_src, name_str), format!("send_{}", name_str)));
                }

                // Setter
                if name_str.ends_with('=') {
                    let method = &name_str[..name_str.len() - 1];
                    return Some((format!("{}.{} = ", receiver_src, method), "send_setter".to_string()));
                }

                None
            }
            _ => None,
        }
    }
}

// =====================
// Visitor implementation
// =====================

impl Visit<'_> for ConditionalAssignmentVisitor<'_> {
    // --- assign_inside_condition: visit all assignment nodes ---

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_path_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_instance_variable_operator_write_node(&mut self, node: &ruby_prism::InstanceVariableOperatorWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_instance_variable_operator_write_node(self, node);
    }

    fn visit_class_variable_operator_write_node(&mut self, node: &ruby_prism::ClassVariableOperatorWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_class_variable_operator_write_node(self, node);
    }

    fn visit_global_variable_operator_write_node(&mut self, node: &ruby_prism::GlobalVariableOperatorWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_global_variable_operator_write_node(self, node);
    }

    fn visit_constant_operator_write_node(&mut self, node: &ruby_prism::ConstantOperatorWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_operator_write_node(self, node);
    }

    fn visit_constant_path_operator_write_node(&mut self, node: &ruby_prism::ConstantPathOperatorWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_path_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_instance_variable_and_write_node(&mut self, node: &ruby_prism::InstanceVariableAndWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_instance_variable_and_write_node(self, node);
    }

    fn visit_class_variable_and_write_node(&mut self, node: &ruby_prism::ClassVariableAndWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_class_variable_and_write_node(self, node);
    }

    fn visit_global_variable_and_write_node(&mut self, node: &ruby_prism::GlobalVariableAndWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_global_variable_and_write_node(self, node);
    }

    fn visit_constant_and_write_node(&mut self, node: &ruby_prism::ConstantAndWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_and_write_node(self, node);
    }

    fn visit_constant_path_and_write_node(&mut self, node: &ruby_prism::ConstantPathAndWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_path_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }

    fn visit_class_variable_or_write_node(&mut self, node: &ruby_prism::ClassVariableOrWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_class_variable_or_write_node(self, node);
    }

    fn visit_global_variable_or_write_node(&mut self, node: &ruby_prism::GlobalVariableOrWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_global_variable_or_write_node(self, node);
    }

    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_or_write_node(self, node);
    }

    fn visit_constant_path_or_write_node(&mut self, node: &ruby_prism::ConstantPathOrWriteNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_constant_path_or_write_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition && self.is_assignment_call(node) {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_call_node(self, node);
    }

    // --- assign_to_condition: visit conditional nodes ---

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if self.enforced_style == EnforcedStyle::AssignToCondition {
            self.check_assign_to_condition_if(node);
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if self.enforced_style == EnforcedStyle::AssignToCondition {
            self.check_assign_to_condition_unless(node);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if self.enforced_style == EnforcedStyle::AssignToCondition {
            self.check_assign_to_condition_case(node);
        }
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        if self.enforced_style == EnforcedStyle::AssignToCondition {
            self.check_assign_to_condition_case_match(node);
        }
        ruby_prism::visit_case_match_node(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elsif_keyword_loc() {
        // Debug: check what if_keyword_loc returns for elsif
        let source = "if foo\n  bar = 1\nelsif baz\n  bar = 2\nelse\n  bar = 3\nend\n";
        let result = ruby_prism::parse(source.as_bytes());
        let node = result.node();

        struct IfDebug<'a> { source: &'a str }
        impl Visit<'_> for IfDebug<'_> {
            fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
                let start = node.location().start_offset();
                let src_at = &self.source[start..self.source.len().min(start + 20)];
                let kw_loc = node.if_keyword_loc();
                if let Some(kw) = &kw_loc {
                    let kw_text = &self.source[kw.start_offset()..kw.end_offset()];
                    eprintln!("IfNode start={} src={:?} kw_loc=Some({:?})", start, src_at, kw_text);
                } else {
                    eprintln!("IfNode start={} src={:?} kw_loc=None", start, src_at);
                }
                ruby_prism::visit_if_node(self, node);
            }
        }

        let mut debug = IfDebug { source };
        debug.visit(&node);
    }

    #[test]
    fn test_assign_to_condition_if_elsif_else() {
        // Test that if/elsif/else with same assignment only produces 1 offense
        let source = "if foo\n  bar = 1\nelsif baz\n  bar = 2\nelse\n  bar = 3\nend\n";
        let cop = ConditionalAssignment::with_config(
            EnforcedStyle::AssignToCondition, true, true,
        );
        let result = ruby_prism::parse(source.as_bytes());
        let node = result.node();
        let program = node.as_program_node().unwrap();
        let ctx = crate::cops::CheckContext::new(source, "test.rb");
        let offenses = cop.check_program(&program, &ctx);
        for o in &offenses {
            eprintln!("Offense: line={} col={}-{} msg={}", o.location.line, o.location.column, o.location.last_column, o.message);
        }
        assert_eq!(offenses.len(), 1, "Expected 1 offense, got {}", offenses.len());
    }
}
