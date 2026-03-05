//! Style/ConditionalAssignment - Checks for consistent assignment placement relative to conditionals.

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

    pub fn with_config(style: EnforcedStyle, include_ternary: bool, single_line_only: bool) -> Self {
        Self {
            enforced_style: style,
            include_ternary_expressions: include_ternary,
            single_line_conditions_only: single_line_only,
            max_line_length: 80,
        }
    }
}

impl Cop for ConditionalAssignment {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ConditionalAssignmentVisitor {
            source: ctx.source, enforced_style: self.enforced_style,
            include_ternary: self.include_ternary_expressions,
            single_line_only: self.single_line_conditions_only,
            max_line_length: self.max_line_length,
            offenses: Vec::new(), filename: ctx.filename,
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

type AssignmentInfo = (String, String);

struct BranchInfo {
    stmt_count: usize,
    tail_assignment: Option<AssignmentInfo>,
}

/// Macro to extract the RHS node from assignment node types.
/// Reduces the massive match blocks in extract_rhs_node and get_assignment_rhs_offsets.
macro_rules! match_write_nodes {
    ($self:expr, $node:expr, $handler:ident) => {
        match $node {
            Node::LocalVariableWriteNode { .. } => $self.$handler($node.as_local_variable_write_node().unwrap().value()),
            Node::InstanceVariableWriteNode { .. } => $self.$handler($node.as_instance_variable_write_node().unwrap().value()),
            Node::ClassVariableWriteNode { .. } => $self.$handler($node.as_class_variable_write_node().unwrap().value()),
            Node::GlobalVariableWriteNode { .. } => $self.$handler($node.as_global_variable_write_node().unwrap().value()),
            Node::ConstantWriteNode { .. } => $self.$handler($node.as_constant_write_node().unwrap().value()),
            Node::ConstantPathWriteNode { .. } => $self.$handler($node.as_constant_path_write_node().unwrap().value()),
            Node::LocalVariableOperatorWriteNode { .. } => $self.$handler($node.as_local_variable_operator_write_node().unwrap().value()),
            Node::InstanceVariableOperatorWriteNode { .. } => $self.$handler($node.as_instance_variable_operator_write_node().unwrap().value()),
            Node::ClassVariableOperatorWriteNode { .. } => $self.$handler($node.as_class_variable_operator_write_node().unwrap().value()),
            Node::GlobalVariableOperatorWriteNode { .. } => $self.$handler($node.as_global_variable_operator_write_node().unwrap().value()),
            Node::ConstantOperatorWriteNode { .. } => $self.$handler($node.as_constant_operator_write_node().unwrap().value()),
            Node::ConstantPathOperatorWriteNode { .. } => $self.$handler($node.as_constant_path_operator_write_node().unwrap().value()),
            Node::LocalVariableAndWriteNode { .. } => $self.$handler($node.as_local_variable_and_write_node().unwrap().value()),
            Node::InstanceVariableAndWriteNode { .. } => $self.$handler($node.as_instance_variable_and_write_node().unwrap().value()),
            Node::ClassVariableAndWriteNode { .. } => $self.$handler($node.as_class_variable_and_write_node().unwrap().value()),
            Node::GlobalVariableAndWriteNode { .. } => $self.$handler($node.as_global_variable_and_write_node().unwrap().value()),
            Node::ConstantAndWriteNode { .. } => $self.$handler($node.as_constant_and_write_node().unwrap().value()),
            Node::ConstantPathAndWriteNode { .. } => $self.$handler($node.as_constant_path_and_write_node().unwrap().value()),
            Node::LocalVariableOrWriteNode { .. } => $self.$handler($node.as_local_variable_or_write_node().unwrap().value()),
            Node::InstanceVariableOrWriteNode { .. } => $self.$handler($node.as_instance_variable_or_write_node().unwrap().value()),
            Node::ClassVariableOrWriteNode { .. } => $self.$handler($node.as_class_variable_or_write_node().unwrap().value()),
            Node::GlobalVariableOrWriteNode { .. } => $self.$handler($node.as_global_variable_or_write_node().unwrap().value()),
            Node::ConstantOrWriteNode { .. } => $self.$handler($node.as_constant_or_write_node().unwrap().value()),
            Node::ConstantPathOrWriteNode { .. } => $self.$handler($node.as_constant_path_or_write_node().unwrap().value()),
            Node::MultiWriteNode { .. } => $self.$handler($node.as_multi_write_node().unwrap().value()),
            Node::CallNode { .. } => {
                let call = $node.as_call_node().unwrap();
                if $self.is_assignment_call(&call) {
                    call.arguments().and_then(|a| {
                        let args: Vec<Node> = a.arguments().iter().collect();
                        args.into_iter().last()
                    }).map(|v| $self.$handler(v)).unwrap_or(None)
                } else {
                    None
                }
            }
            _ => None,
        }
    };
}

impl<'a> ConditionalAssignmentVisitor<'a> {
    fn src(&self, start: usize, end: usize) -> &'a str { &self.source[start..end] }

    fn add_offense(&mut self, start_offset: usize, end_offset: usize, message: &str) {
        let effective_end = self.source[start_offset..end_offset]
            .find('\n').map_or(end_offset, |p| start_offset + p);
        let location = Location::from_offsets(self.source, start_offset, effective_end);
        self.offenses.push(Offense::new(COP_NAME, message, Severity::Convention, location, self.filename));
    }

    fn check_assign_inside_condition(&mut self, node: &Node) {
        let (assign_start, assign_end) = (node.location().start_offset(), node.location().end_offset());
        let rhs = match self.extract_rhs_node(node) {
            Some(r) => r,
            None => return,
        };

        let rhs_inner = self.get_paren_inner(&rhs);
        let check_node = rhs_inner.as_ref().unwrap_or(&rhs);

        match check_node {
            Node::IfNode { .. } => {
                let if_node = check_node.as_if_node().unwrap();
                if self.is_ternary(&if_node) {
                    if !self.include_ternary { return; }
                    self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
                    return;
                }
                if !if_node.subsequent().is_some() { return; }
                if self.single_line_only && self.if_has_multiline_branch(&if_node) { return; }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            Node::UnlessNode { .. } => {
                let unless_node = check_node.as_unless_node().unwrap();
                if self.single_line_only && self.unless_has_multiline_branch(&unless_node) { return; }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            Node::CaseNode { .. } => {
                let case_node = check_node.as_case_node().unwrap();
                if case_node.else_clause().is_none() { return; }
                if self.single_line_only && self.case_has_multiline_branch(&case_node) { return; }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            Node::CaseMatchNode { .. } => {
                let cm = check_node.as_case_match_node().unwrap();
                if cm.else_clause().is_none() { return; }
                if self.single_line_only && self.case_match_has_multiline_branch(&cm) { return; }
                self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
            }
            _ => {}
        }
    }

    fn extract_rhs_node_inner<'b>(&self, value: Node<'b>) -> Option<Node<'b>> { Some(value) }

    fn extract_rhs_node<'b>(&self, node: &'b Node) -> Option<Node<'b>> {
        match_write_nodes!(self, node, extract_rhs_node_inner)
    }

    fn get_paren_inner<'b>(&self, node: &'b Node) -> Option<Node<'b>> {
        if let Node::ParenthesesNode { .. } = node {
            let paren = node.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Node::StatementsNode { .. } = &body {
                    let stmts = body.as_statements_node().unwrap();
                    let mut iter = stmts.body().iter();
                    let first = iter.next();
                    if iter.next().is_none() { return first; }
                } else {
                    return Some(body);
                }
            }
        }
        None
    }

    fn rhs_offsets_inner(&self, value: Node) -> Option<(usize, usize)> {
        Some((value.location().start_offset(), value.location().end_offset()))
    }

    fn get_assignment_rhs_offsets(&self, node: &Node) -> Option<(usize, usize, usize, usize)> {
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        let rhs_range = match_write_nodes!(self, node, rhs_offsets_inner);
        rhs_range.map(|(rs, re)| (start, end, rs, re))
    }

    fn is_assignment_call(&self, call: &ruby_prism::CallNode) -> bool {
        let name = std::str::from_utf8(call.name().as_slice()).unwrap_or("");
        matches!(name, "[]=" | "<<" | "=~" | "!~" | "<=>" | "<" | ">" | "==" | "!=" | "===" | ">=" | "<=")
            || (name.ends_with('=') && name.len() > 1 && !matches!(name, "!=" | "==" | "===" | ">=" | "<="))
    }

    fn is_ternary(&self, node: &ruby_prism::IfNode) -> bool {
        !self.source[node.location().start_offset()..].starts_with("if")
    }

    fn stmts_count(stmts: &Option<ruby_prism::StatementsNode>) -> usize {
        stmts.as_ref().map_or(0, |s| s.body().iter().count())
    }

    fn if_has_multiline_branch(&self, node: &ruby_prism::IfNode) -> bool {
        if Self::stmts_count(&node.statements()) > 1 { return true; }
        let mut sub = node.subsequent();
        while let Some(s) = sub {
            match &s {
                Node::IfNode { .. } => {
                    let elsif = s.as_if_node().unwrap();
                    if Self::stmts_count(&elsif.statements()) > 1 { return true; }
                    sub = elsif.subsequent();
                }
                Node::ElseNode { .. } => return Self::stmts_count(&s.as_else_node().unwrap().statements()) > 1,
                _ => break,
            }
        }
        false
    }

    fn unless_has_multiline_branch(&self, node: &ruby_prism::UnlessNode) -> bool {
        Self::stmts_count(&node.statements()) > 1
            || node.else_clause().map_or(false, |ec| Self::stmts_count(&ec.statements()) > 1)
    }

    fn case_has_multiline_branch(&self, node: &ruby_prism::CaseNode) -> bool {
        node.conditions().iter().any(|cond| {
            matches!(&cond, Node::WhenNode { .. }) && Self::stmts_count(&cond.as_when_node().unwrap().statements()) > 1
        }) || node.else_clause().map_or(false, |ec| Self::stmts_count(&ec.statements()) > 1)
    }

    fn case_match_has_multiline_branch(&self, node: &ruby_prism::CaseMatchNode) -> bool {
        node.conditions().iter().any(|cond| {
            matches!(&cond, Node::InNode { .. }) && Self::stmts_count(&cond.as_in_node().unwrap().statements()) > 1
        }) || node.else_clause().map_or(false, |ec| Self::stmts_count(&ec.statements()) > 1)
    }

    fn branch_info_from_stmts(&self, stmts: &Option<ruby_prism::StatementsNode>) -> BranchInfo {
        match stmts {
            Some(s) => {
                let mut count = 0usize;
                let mut last_node_info: Option<AssignmentInfo> = None;
                for node in s.body().iter() {
                    count += 1;
                    last_node_info = self.extract_assignment_lhs(&node);
                }
                BranchInfo { stmt_count: count, tail_assignment: last_node_info }
            }
            None => BranchInfo { stmt_count: 0, tail_assignment: None },
        }
    }

    fn check_assign_to_condition_if(&mut self, node: &ruby_prism::IfNode) {
        if self.is_elsif(node) { return; }
        if self.is_ternary(node) {
            if !self.include_ternary { return; }
            self.check_ternary_assign_to_condition(node);
            return;
        }
        let branch_infos = self.collect_if_branch_infos(node);
        if branch_infos.is_empty() { return; }
        self.check_branch_infos_same_assignment(node.location().start_offset(), node.location().end_offset(), &branch_infos);
    }

    fn check_assign_to_condition_unless(&mut self, node: &ruby_prism::UnlessNode) {
        if node.else_clause().is_none() { return; }
        let mut branch_infos = vec![self.branch_info_from_stmts(&node.statements())];
        if let Some(ec) = node.else_clause() {
            branch_infos.push(self.branch_info_from_stmts(&ec.statements()));
        }
        self.check_branch_infos_same_assignment(node.location().start_offset(), node.location().end_offset(), &branch_infos);
    }

    fn check_assign_to_condition_case(&mut self, node: &ruby_prism::CaseNode) {
        if node.else_clause().is_none() { return; }
        let mut branch_infos = Vec::new();
        for cond in node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                let info = self.branch_info_from_stmts(&cond.as_when_node().unwrap().statements());
                if info.tail_assignment.is_none() { return; }
                branch_infos.push(info);
            }
        }
        if let Some(ec) = node.else_clause() {
            let info = self.branch_info_from_stmts(&ec.statements());
            if info.tail_assignment.is_none() { return; }
            branch_infos.push(info);
        }
        self.check_branch_infos_same_assignment(node.location().start_offset(), node.location().end_offset(), &branch_infos);
    }

    fn check_assign_to_condition_case_match(&mut self, node: &ruby_prism::CaseMatchNode) {
        if node.else_clause().is_none() { return; }
        let mut branch_infos = Vec::new();
        for cond in node.conditions().iter() {
            if let Node::InNode { .. } = &cond {
                let info = self.branch_info_from_stmts(&cond.as_in_node().unwrap().statements());
                if info.tail_assignment.is_none() { return; }
                branch_infos.push(info);
            }
        }
        if let Some(ec) = node.else_clause() {
            let info = self.branch_info_from_stmts(&ec.statements());
            if info.tail_assignment.is_none() { return; }
            branch_infos.push(info);
        }
        self.check_branch_infos_same_assignment(node.location().start_offset(), node.location().end_offset(), &branch_infos);
    }

    fn check_ternary_assign_to_condition(&mut self, node: &ruby_prism::IfNode) {
        let if_info = self.branch_info_from_stmts(&node.statements());
        if if_info.stmt_count != 1 { return; }
        let else_info = match node.subsequent() {
            Some(sub) if matches!(&sub, Node::ElseNode { .. }) => {
                let info = self.branch_info_from_stmts(&sub.as_else_node().unwrap().statements());
                if info.stmt_count != 1 { return; }
                info
            }
            _ => return,
        };
        self.check_branch_infos_same_assignment(
            node.location().start_offset(), node.location().end_offset(), &[if_info, else_info],
        );
    }

    fn is_elsif(&self, node: &ruby_prism::IfNode) -> bool {
        node.if_keyword_loc().map_or(false, |loc| self.source[loc.start_offset()..].starts_with("elsif"))
    }

    fn collect_if_branch_infos(&self, node: &ruby_prism::IfNode) -> Vec<BranchInfo> {
        let mut infos = vec![self.branch_info_from_stmts(&node.statements())];
        let mut sub = node.subsequent();
        loop {
            match sub {
                None => return vec![],
                Some(s) => match &s {
                    Node::IfNode { .. } => {
                        let elsif = s.as_if_node().unwrap();
                        infos.push(self.branch_info_from_stmts(&elsif.statements()));
                        sub = elsif.subsequent();
                    }
                    Node::ElseNode { .. } => {
                        infos.push(self.branch_info_from_stmts(&s.as_else_node().unwrap().statements()));
                        break;
                    }
                    _ => return vec![],
                },
            }
        }
        infos
    }

    fn check_branch_infos_same_assignment(&mut self, cond_start: usize, cond_end: usize, branches: &[BranchInfo]) {
        if branches.is_empty() { return; }

        let mut first_lhs: Option<&str> = None;
        let mut first_kind: Option<&str> = None;

        for branch in branches {
            let (lhs, kind) = match &branch.tail_assignment { Some(a) => a, None => return };
            if first_lhs.map_or(false, |fl| fl != lhs) { return; }
            if first_kind.map_or(false, |fk| fk != kind) { return; }
            first_lhs = Some(lhs);
            first_kind = Some(kind);
        }

        if self.single_line_only && branches.iter().any(|b| b.stmt_count > 1) { return; }

        if let Some(assignment_lhs) = first_lhs {
            if self.correction_exceeds_line_limit(cond_start, cond_end, assignment_lhs) { return; }
        }

        self.add_offense(cond_start, cond_end, MSG);
    }

    fn correction_exceeds_line_limit(&self, cond_start: usize, cond_end: usize, assignment_lhs: &str) -> bool {
        let cond_source = &self.source[cond_start..cond_end];
        let assignment_trimmed = assignment_lhs.trim();
        let longest_stripped_len = cond_source.lines()
            .map(|line| self.strip_assignment_from_line(line.trim_end_matches('\r'), assignment_trimmed).len())
            .max().unwrap_or(0);
        assignment_lhs.len() + longest_stripped_len > self.max_line_length
    }

    fn strip_assignment_from_line<'b>(&self, line: &'b str, assignment_trimmed: &str) -> String {
        if let Some(pos) = line.find(assignment_trimmed) {
            let mut start = pos;
            while start > 0 && line.as_bytes()[start - 1] == b' ' { start -= 1; }
            let mut end_adj = pos + assignment_trimmed.len();
            while end_adj < line.len() && line.as_bytes()[end_adj] == b' ' { end_adj += 1; }
            format!("{}{}", &line[..start], &line[end_adj..])
        } else {
            line.to_string()
        }
    }

    fn extract_assignment_lhs(&self, node: &Node) -> Option<AssignmentInfo> {
        // Simple write nodes: name = value
        macro_rules! simple_write {
            ($node_type:ident, $kind:expr) => {{
                let n = node.$node_type().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} = ", name), $kind.to_string()))
            }};
        }
        // Operator write nodes: name op= value
        macro_rules! op_write {
            ($node_type:ident, $kind:expr) => {{
                let n = node.$node_type().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some((format!("{} {}= ", name, op), $kind.to_string()))
            }};
        }
        // Boolean compound write: name &&= or name ||= value
        macro_rules! bool_write {
            ($node_type:ident, $op:expr, $kind:expr) => {{
                let n = node.$node_type().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} {} ", name, $op), $kind.to_string()))
            }};
        }
        // Path-based boolean compound write: target &&= or target ||= value
        macro_rules! path_bool_write {
            ($node_type:ident, $op:expr, $kind:expr) => {{
                let n = node.$node_type().unwrap();
                let target = n.target();
                let target_src = self.src(target.location().start_offset(), target.location().end_offset());
                Some((format!("{} {} ", target_src, $op), $kind.to_string()))
            }};
        }

        match node {
            Node::LocalVariableWriteNode { .. } => simple_write!(as_local_variable_write_node, "lvasgn"),
            Node::InstanceVariableWriteNode { .. } => simple_write!(as_instance_variable_write_node, "ivasgn"),
            Node::ClassVariableWriteNode { .. } => simple_write!(as_class_variable_write_node, "cvasgn"),
            Node::GlobalVariableWriteNode { .. } => simple_write!(as_global_variable_write_node, "gvasgn"),
            Node::ConstantWriteNode { .. } => simple_write!(as_constant_write_node, "casgn"),
            Node::ConstantPathWriteNode { .. } => {
                let n = node.as_constant_path_write_node().unwrap();
                let target = n.target();
                let target_src = self.src(target.location().start_offset(), target.location().end_offset());
                Some((format!("{} = ", target_src), "casgn".to_string()))
            }
            Node::LocalVariableOperatorWriteNode { .. } => op_write!(as_local_variable_operator_write_node, "op_asgn"),
            Node::InstanceVariableOperatorWriteNode { .. } => op_write!(as_instance_variable_operator_write_node, "op_asgn"),
            Node::ClassVariableOperatorWriteNode { .. } => op_write!(as_class_variable_operator_write_node, "op_asgn"),
            Node::GlobalVariableOperatorWriteNode { .. } => op_write!(as_global_variable_operator_write_node, "op_asgn"),
            Node::ConstantOperatorWriteNode { .. } => op_write!(as_constant_operator_write_node, "op_asgn"),
            Node::LocalVariableAndWriteNode { .. } => bool_write!(as_local_variable_and_write_node, "&&=", "and_asgn"),
            Node::InstanceVariableAndWriteNode { .. } => bool_write!(as_instance_variable_and_write_node, "&&=", "and_asgn"),
            Node::ClassVariableAndWriteNode { .. } => bool_write!(as_class_variable_and_write_node, "&&=", "and_asgn"),
            Node::GlobalVariableAndWriteNode { .. } => bool_write!(as_global_variable_and_write_node, "&&=", "and_asgn"),
            Node::ConstantAndWriteNode { .. } => bool_write!(as_constant_and_write_node, "&&=", "and_asgn"),
            Node::ConstantPathAndWriteNode { .. } => path_bool_write!(as_constant_path_and_write_node, "&&=", "and_asgn"),
            Node::LocalVariableOrWriteNode { .. } => bool_write!(as_local_variable_or_write_node, "||=", "or_asgn"),
            Node::InstanceVariableOrWriteNode { .. } => bool_write!(as_instance_variable_or_write_node, "||=", "or_asgn"),
            Node::ClassVariableOrWriteNode { .. } => bool_write!(as_class_variable_or_write_node, "||=", "or_asgn"),
            Node::GlobalVariableOrWriteNode { .. } => bool_write!(as_global_variable_or_write_node, "||=", "or_asgn"),
            Node::ConstantOrWriteNode { .. } => bool_write!(as_constant_or_write_node, "||=", "or_asgn"),
            Node::ConstantPathOrWriteNode { .. } => path_bool_write!(as_constant_path_or_write_node, "||=", "or_asgn"),
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let name_str = std::str::from_utf8(call.name().as_slice()).unwrap_or("");
                if !self.is_assignment_call(&call) { return None; }

                let receiver_src = call.receiver().map(|r| {
                    self.src(r.location().start_offset(), r.location().end_offset()).to_string()
                }).unwrap_or_default();

                if name_str == "[]=" {
                    let args: Vec<(usize, usize)> = call.arguments()
                        .map(|a| a.arguments().iter().map(|arg| (arg.location().start_offset(), arg.location().end_offset())).collect())
                        .unwrap_or_default();
                    if args.len() >= 2 {
                        let indices: Vec<String> = args[..args.len()-1].iter().map(|&(s, e)| self.src(s, e).to_string()).collect();
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

/// Macro to generate visit methods for write nodes that check assign_inside_condition.
macro_rules! visit_write_node {
    ($method:ident, $node_type:ty, $visit_fn:path) => {
        fn $method(&mut self, node: &$node_type) {
            if self.enforced_style == EnforcedStyle::AssignInsideCondition {
                self.check_assign_inside_condition(&node.as_node());
            }
            $visit_fn(self, node);
        }
    };
}

impl Visit<'_> for ConditionalAssignmentVisitor<'_> {
    visit_write_node!(visit_local_variable_write_node, ruby_prism::LocalVariableWriteNode, ruby_prism::visit_local_variable_write_node);
    visit_write_node!(visit_instance_variable_write_node, ruby_prism::InstanceVariableWriteNode, ruby_prism::visit_instance_variable_write_node);
    visit_write_node!(visit_class_variable_write_node, ruby_prism::ClassVariableWriteNode, ruby_prism::visit_class_variable_write_node);
    visit_write_node!(visit_global_variable_write_node, ruby_prism::GlobalVariableWriteNode, ruby_prism::visit_global_variable_write_node);
    visit_write_node!(visit_constant_write_node, ruby_prism::ConstantWriteNode, ruby_prism::visit_constant_write_node);
    visit_write_node!(visit_constant_path_write_node, ruby_prism::ConstantPathWriteNode, ruby_prism::visit_constant_path_write_node);
    visit_write_node!(visit_multi_write_node, ruby_prism::MultiWriteNode, ruby_prism::visit_multi_write_node);
    visit_write_node!(visit_local_variable_operator_write_node, ruby_prism::LocalVariableOperatorWriteNode, ruby_prism::visit_local_variable_operator_write_node);
    visit_write_node!(visit_instance_variable_operator_write_node, ruby_prism::InstanceVariableOperatorWriteNode, ruby_prism::visit_instance_variable_operator_write_node);
    visit_write_node!(visit_class_variable_operator_write_node, ruby_prism::ClassVariableOperatorWriteNode, ruby_prism::visit_class_variable_operator_write_node);
    visit_write_node!(visit_global_variable_operator_write_node, ruby_prism::GlobalVariableOperatorWriteNode, ruby_prism::visit_global_variable_operator_write_node);
    visit_write_node!(visit_constant_operator_write_node, ruby_prism::ConstantOperatorWriteNode, ruby_prism::visit_constant_operator_write_node);
    visit_write_node!(visit_constant_path_operator_write_node, ruby_prism::ConstantPathOperatorWriteNode, ruby_prism::visit_constant_path_operator_write_node);
    visit_write_node!(visit_local_variable_and_write_node, ruby_prism::LocalVariableAndWriteNode, ruby_prism::visit_local_variable_and_write_node);
    visit_write_node!(visit_instance_variable_and_write_node, ruby_prism::InstanceVariableAndWriteNode, ruby_prism::visit_instance_variable_and_write_node);
    visit_write_node!(visit_class_variable_and_write_node, ruby_prism::ClassVariableAndWriteNode, ruby_prism::visit_class_variable_and_write_node);
    visit_write_node!(visit_global_variable_and_write_node, ruby_prism::GlobalVariableAndWriteNode, ruby_prism::visit_global_variable_and_write_node);
    visit_write_node!(visit_constant_and_write_node, ruby_prism::ConstantAndWriteNode, ruby_prism::visit_constant_and_write_node);
    visit_write_node!(visit_constant_path_and_write_node, ruby_prism::ConstantPathAndWriteNode, ruby_prism::visit_constant_path_and_write_node);
    visit_write_node!(visit_local_variable_or_write_node, ruby_prism::LocalVariableOrWriteNode, ruby_prism::visit_local_variable_or_write_node);
    visit_write_node!(visit_instance_variable_or_write_node, ruby_prism::InstanceVariableOrWriteNode, ruby_prism::visit_instance_variable_or_write_node);
    visit_write_node!(visit_class_variable_or_write_node, ruby_prism::ClassVariableOrWriteNode, ruby_prism::visit_class_variable_or_write_node);
    visit_write_node!(visit_global_variable_or_write_node, ruby_prism::GlobalVariableOrWriteNode, ruby_prism::visit_global_variable_or_write_node);
    visit_write_node!(visit_constant_or_write_node, ruby_prism::ConstantOrWriteNode, ruby_prism::visit_constant_or_write_node);
    visit_write_node!(visit_constant_path_or_write_node, ruby_prism::ConstantPathOrWriteNode, ruby_prism::visit_constant_path_or_write_node);

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.enforced_style == EnforcedStyle::AssignInsideCondition && self.is_assignment_call(node) {
            self.check_assign_inside_condition(&node.as_node());
        }
        ruby_prism::visit_call_node(self, node);
    }

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
    fn test_assign_to_condition_if_elsif_else() {
        let source = "if foo\n  bar = 1\nelsif baz\n  bar = 2\nelse\n  bar = 3\nend\n";
        let cop = ConditionalAssignment::with_config(EnforcedStyle::AssignToCondition, true, true);
        let result = ruby_prism::parse(source.as_bytes());
        let node = result.node();
        let program = node.as_program_node().unwrap();
        let ctx = crate::cops::CheckContext::new(source, "test.rb");
        let offenses = cop.check_program(&program, &ctx);
        assert_eq!(offenses.len(), 1, "Expected 1 offense, got {}", offenses.len());
    }
}
