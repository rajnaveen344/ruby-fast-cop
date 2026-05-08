//! Style/ConditionalAssignment - Checks for consistent assignment placement relative to conditionals.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
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
    end_alignment_keyword: bool,
}

impl ConditionalAssignment {
    pub fn new(style: EnforcedStyle) -> Self {
        Self {
            enforced_style: style,
            include_ternary_expressions: true,
            single_line_conditions_only: true,
            max_line_length: 80,
            end_alignment_keyword: false,
        }
    }

    pub fn with_config(style: EnforcedStyle, include_ternary: bool, single_line_only: bool) -> Self {
        Self {
            enforced_style: style,
            include_ternary_expressions: include_ternary,
            single_line_conditions_only: single_line_only,
            max_line_length: 80,
            end_alignment_keyword: false,
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
            end_alignment_keyword: self.end_alignment_keyword,
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
    end_alignment_keyword: bool,
    offenses: Vec<Offense>,
    filename: &'a str,
}

type AssignmentInfo = (String, String);

struct BranchInfo {
    stmt_count: usize,
    tail_assignment: Option<AssignmentInfo>,
}

/// Macro to extract the RHS node from assignment node types.
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

    fn add_offense_with_correction(&mut self, start_offset: usize, end_offset: usize, message: &str, correction: Correction) {
        let effective_end = self.source[start_offset..end_offset]
            .find('\n').map_or(end_offset, |p| start_offset + p);
        let location = Location::from_offsets(self.source, start_offset, effective_end);
        let offense = Offense::new(COP_NAME, message, Severity::Convention, location, self.filename)
            .with_correction(correction);
        self.offenses.push(offense);
    }

    fn check_assign_inside_condition(&mut self, node: &Node) {
        let (assign_start, assign_end) = (node.location().start_offset(), node.location().end_offset());
        let rhs = match self.extract_rhs_node(node) {
            Some(r) => r,
            None => return,
        };

        let rhs_inner = self.get_paren_inner(&rhs);
        let check_node = rhs_inner.as_ref().unwrap_or(&rhs);
        let has_parens = rhs_inner.is_some();

        match check_node {
            Node::IfNode { .. } => {
                let if_node = check_node.as_if_node().unwrap();
                if self.is_ternary(&if_node) {
                    if !self.include_ternary { return; }
                    if let Some(correction) = self.correct_assign_inside_ternary(node, &if_node, has_parens) {
                        self.add_offense_with_correction(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG, correction);
                    } else {
                        self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
                    }
                    return;
                }
                if !if_node.subsequent().is_some() { return; }
                if self.single_line_only && self.if_has_multiline_branch(&if_node) { return; }
                if let Some(correction) = self.correct_assign_inside_if(node, check_node) {
                    self.add_offense_with_correction(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG, correction);
                } else {
                    self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
                }
            }
            Node::UnlessNode { .. } => {
                let unless_node = check_node.as_unless_node().unwrap();
                if self.single_line_only && self.unless_has_multiline_branch(&unless_node) { return; }
                if let Some(correction) = self.correct_assign_inside_unless(node, check_node) {
                    self.add_offense_with_correction(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG, correction);
                } else {
                    self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
                }
            }
            Node::CaseNode { .. } => {
                let case_node = check_node.as_case_node().unwrap();
                if case_node.else_clause().is_none() { return; }
                if self.single_line_only && self.case_has_multiline_branch(&case_node) { return; }
                if let Some(correction) = self.correct_assign_inside_case(node, check_node) {
                    self.add_offense_with_correction(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG, correction);
                } else {
                    self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
                }
            }
            Node::CaseMatchNode { .. } => {
                let cm = check_node.as_case_match_node().unwrap();
                if cm.else_clause().is_none() { return; }
                if self.single_line_only && self.case_match_has_multiline_branch(&cm) { return; }
                if let Some(correction) = self.correct_assign_inside_case_match(node, check_node) {
                    self.add_offense_with_correction(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG, correction);
                } else {
                    self.add_offense(assign_start, assign_end, ASSIGN_TO_CONDITION_MSG);
                }
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------------------
    // assign_inside_condition corrections
    // -------------------------------------------------------------------------

    /// Get the LHS text from the outer assignment node (e.g. "bar = ", "bar += ", "bar &&= ").
    fn get_lhs_text(&self, node: &Node) -> Option<String> {
        match node {
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                Some(format!("{} = ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::InstanceVariableWriteNode { .. } => {
                let n = node.as_instance_variable_write_node().unwrap();
                Some(format!("{} = ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ClassVariableWriteNode { .. } => {
                let n = node.as_class_variable_write_node().unwrap();
                Some(format!("{} = ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::GlobalVariableWriteNode { .. } => {
                let n = node.as_global_variable_write_node().unwrap();
                Some(format!("{} = ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ConstantWriteNode { .. } => {
                let n = node.as_constant_write_node().unwrap();
                Some(format!("{} = ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ConstantPathWriteNode { .. } => {
                let n = node.as_constant_path_write_node().unwrap();
                let t = n.target();
                let ts = self.src(t.location().start_offset(), t.location().end_offset());
                Some(format!("{} = ", ts))
            }
            Node::LocalVariableOperatorWriteNode { .. } => {
                let n = node.as_local_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some(format!("{} {}= ", name, op))
            }
            Node::InstanceVariableOperatorWriteNode { .. } => {
                let n = node.as_instance_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some(format!("{} {}= ", name, op))
            }
            Node::ClassVariableOperatorWriteNode { .. } => {
                let n = node.as_class_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some(format!("{} {}= ", name, op))
            }
            Node::GlobalVariableOperatorWriteNode { .. } => {
                let n = node.as_global_variable_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some(format!("{} {}= ", name, op))
            }
            Node::ConstantOperatorWriteNode { .. } => {
                let n = node.as_constant_operator_write_node().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some(format!("{} {}= ", name, op))
            }
            Node::LocalVariableAndWriteNode { .. } => {
                let n = node.as_local_variable_and_write_node().unwrap();
                Some(format!("{} &&= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::InstanceVariableAndWriteNode { .. } => {
                let n = node.as_instance_variable_and_write_node().unwrap();
                Some(format!("{} &&= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ClassVariableAndWriteNode { .. } => {
                let n = node.as_class_variable_and_write_node().unwrap();
                Some(format!("{} &&= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::GlobalVariableAndWriteNode { .. } => {
                let n = node.as_global_variable_and_write_node().unwrap();
                Some(format!("{} &&= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ConstantAndWriteNode { .. } => {
                let n = node.as_constant_and_write_node().unwrap();
                Some(format!("{} &&= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ConstantPathAndWriteNode { .. } => {
                let n = node.as_constant_path_and_write_node().unwrap();
                let t = n.target();
                let ts = self.src(t.location().start_offset(), t.location().end_offset());
                Some(format!("{} &&= ", ts))
            }
            Node::LocalVariableOrWriteNode { .. } => {
                let n = node.as_local_variable_or_write_node().unwrap();
                Some(format!("{} ||= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::InstanceVariableOrWriteNode { .. } => {
                let n = node.as_instance_variable_or_write_node().unwrap();
                Some(format!("{} ||= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ClassVariableOrWriteNode { .. } => {
                let n = node.as_class_variable_or_write_node().unwrap();
                Some(format!("{} ||= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::GlobalVariableOrWriteNode { .. } => {
                let n = node.as_global_variable_or_write_node().unwrap();
                Some(format!("{} ||= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ConstantOrWriteNode { .. } => {
                let n = node.as_constant_or_write_node().unwrap();
                Some(format!("{} ||= ", std::str::from_utf8(n.name().as_slice()).unwrap_or("")))
            }
            Node::ConstantPathOrWriteNode { .. } => {
                let n = node.as_constant_path_or_write_node().unwrap();
                let t = n.target();
                let ts = self.src(t.location().start_offset(), t.location().end_offset());
                Some(format!("{} ||= ", ts))
            }
            Node::MultiWriteNode { .. } => {
                let n = node.as_multi_write_node().unwrap();
                // Multi-write: get the LHS range (from start to before value)
                let v = n.value();
                let lhs_end = v.location().start_offset();
                let lhs_start = node.location().start_offset();
                Some(self.src(lhs_start, lhs_end).to_string())
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let name_str = std::str::from_utf8(call.name().as_slice()).unwrap_or("");
                if !self.is_assignment_call(&call) { return None; }
                let recv_src = call.receiver().map(|r| {
                    self.src(r.location().start_offset(), r.location().end_offset()).to_string()
                }).unwrap_or_default();
                if name_str == "[]=" {
                    let args: Vec<(usize, usize)> = call.arguments()
                        .map(|a| a.arguments().iter().map(|arg| (arg.location().start_offset(), arg.location().end_offset())).collect())
                        .unwrap_or_default();
                    if args.len() >= 2 {
                        let indices: Vec<String> = args[..args.len()-1].iter().map(|&(s, e)| self.src(s, e).to_string()).collect();
                        return Some(format!("{}[{}] = ", recv_src, indices.join(", ")));
                    }
                    return None;
                }
                if name_str == "<<" { return Some(format!("{} << ", recv_src)); }
                if matches!(name_str, "==" | "!=" | "===" | "=~" | "!~" | "<=>" | "<" | ">" | ">=" | "<=") {
                    return Some(format!("{} {} ", recv_src, name_str));
                }
                if name_str.ends_with('=') {
                    let method = &name_str[..name_str.len() - 1];
                    return Some(format!("{}.{} = ", recv_src, method));
                }
                None
            }
            _ => None,
        }
    }

    /// Compute byte offset of start of line containing `offset`.
    fn line_start(&self, offset: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut i = offset;
        while i > 0 && bytes[i - 1] != b'\n' { i -= 1; }
        i
    }

    /// Compute column of `offset` (bytes from line start).
    fn col_of(&self, offset: usize) -> usize {
        offset - self.line_start(offset)
    }

    /// For assign_inside_condition: rewrite the whole assignment node.
    /// Strategy: replace entire `outer_node` with rewritten conditional.
    fn correct_assign_inside_if(&self, outer_node: &Node, if_node_raw: &Node) -> Option<Correction> {
        let lhs = self.get_lhs_text(outer_node)?;
        let if_node = if_node_raw.as_if_node().unwrap();
        let col = self.col_of(outer_node.location().start_offset());
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        let new_src = self.rewrite_if_assign_inside(if_node_raw, &lhs, &indent, &branch_indent, false)?;
        Some(Correction::replace(
            outer_node.location().start_offset(),
            outer_node.location().end_offset(),
            new_src,
        ))
    }

    fn correct_assign_inside_unless(&self, outer_node: &Node, unless_node_raw: &Node) -> Option<Correction> {
        let lhs = self.get_lhs_text(outer_node)?;
        let unless_node = unless_node_raw.as_unless_node().unwrap();
        let col = self.col_of(outer_node.location().start_offset());
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        let cond_src = {
            let c = unless_node.predicate();
            self.src(c.location().start_offset(), c.location().end_offset())
        };

        let if_stmts = unless_node.statements();
        let if_body = self.rewrite_branch_body_inline(&if_stmts, &lhs, &branch_indent)?;

        let result = if let Some(else_clause) = unless_node.else_clause() {
            let else_body = self.rewrite_else_body_inline(&Some(else_clause), &lhs, &branch_indent)?;
            format!("{}unless {}\n{}\n{}else\n{}\n{}end",
                indent, cond_src, if_body, indent, else_body, indent)
        } else {
            format!("{}unless {}\n{}\n{}end",
                indent, cond_src, if_body, indent)
        };
        Some(Correction::replace(
            outer_node.location().start_offset(),
            outer_node.location().end_offset(),
            result,
        ))
    }

    fn correct_assign_inside_case(&self, outer_node: &Node, case_node_raw: &Node) -> Option<Correction> {
        let lhs = self.get_lhs_text(outer_node)?;
        let case_node = case_node_raw.as_case_node().unwrap();
        let col = self.col_of(outer_node.location().start_offset());
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        let subject = case_node.predicate().map(|p| {
            format!(" {}", self.src(p.location().start_offset(), p.location().end_offset()))
        }).unwrap_or_default();

        let mut parts = vec![format!("{}case{}", indent, subject)];

        for cond in case_node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                let when_node = cond.as_when_node().unwrap();
                let conds_src: Vec<String> = when_node.conditions().iter()
                    .map(|c| self.src(c.location().start_offset(), c.location().end_offset()).to_string())
                    .collect();

                // Check for "then" keyword style
                let when_src = self.src(cond.location().start_offset(), cond.location().end_offset());
                let has_then = when_src.contains(" then ");

                if has_then {
                    // inline style: "when cond then val"
                    let body_src = self.get_branch_raw_src(&when_node.statements())?;
                    parts.push(format!("{}when {} then {}{}", indent, conds_src.join(", "), lhs, body_src));
                } else {
                    let body = self.rewrite_branch_body_inline(&when_node.statements(), &lhs, &branch_indent)?;
                    parts.push(format!("{}when {}\n{}", indent, conds_src.join(", "), body));
                }
            }
        }

        // Determine if any when used "then" style to decide else formatting
        let any_when_has_then = case_node.conditions().iter().any(|cond| {
            if let Node::WhenNode { .. } = &cond {
                let ws = self.src(cond.location().start_offset(), cond.location().end_offset());
                ws.contains(" then ")
            } else { false }
        });

        if let Some(else_clause) = case_node.else_clause() {
            let else_src = self.src(else_clause.location().start_offset(), else_clause.location().end_offset());
            let has_inline_else = !else_src.starts_with("else\n") && !else_src.starts_with("else \n");
            if any_when_has_then || (has_inline_else && !else_src.contains('\n')) {
                // inline else: "else lhs val"
                let body_src = self.get_branch_raw_src(&else_clause.statements())?;
                parts.push(format!("{}else {}{}", indent, lhs, body_src));
            } else {
                let body = self.rewrite_else_body_inline(&Some(else_clause), &lhs, &branch_indent)?;
                parts.push(format!("{}else\n{}", indent, body));
            }
        }

        parts.push(format!("{}end", indent));

        Some(Correction::replace(
            outer_node.location().start_offset(),
            outer_node.location().end_offset(),
            parts.join("\n"),
        ))
    }

    fn correct_assign_inside_case_match(&self, outer_node: &Node, case_node_raw: &Node) -> Option<Correction> {
        let lhs = self.get_lhs_text(outer_node)?;
        let cm = case_node_raw.as_case_match_node().unwrap();
        let col = self.col_of(outer_node.location().start_offset());
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        let subject = cm.predicate().map(|p| {
            format!(" {}", self.src(p.location().start_offset(), p.location().end_offset()))
        }).unwrap_or_default();

        let mut parts = vec![format!("{}case{}", indent, subject)];

        for cond in cm.conditions().iter() {
            if let Node::InNode { .. } = &cond {
                let in_node = cond.as_in_node().unwrap();
                let pat = in_node.pattern();
                let pat_src = self.src(pat.location().start_offset(), pat.location().end_offset());
                let body = self.rewrite_branch_body_inline(&in_node.statements(), &lhs, &branch_indent)?;
                parts.push(format!("{}in {}\n{}", indent, pat_src, body));
            }
        }

        if let Some(else_clause) = cm.else_clause() {
            let body = self.rewrite_else_body_inline(&Some(else_clause), &lhs, &branch_indent)?;
            parts.push(format!("{}else\n{}", indent, body));
        }

        parts.push(format!("{}end", indent));

        Some(Correction::replace(
            outer_node.location().start_offset(),
            outer_node.location().end_offset(),
            parts.join("\n"),
        ))
    }

    fn get_branch_raw_src(&self, stmts: &Option<ruby_prism::StatementsNode>) -> Option<String> {
        let stmts = stmts.as_ref()?;
        let last = stmts.body().iter().last()?;
        Some(self.src(last.location().start_offset(), last.location().end_offset()).to_string())
    }

    fn correct_assign_inside_ternary(&self, outer_node: &Node, if_node: &ruby_prism::IfNode, has_parens: bool) -> Option<Correction> {
        let lhs = self.get_lhs_text(outer_node)?;
        let cond_src = {
            let c = if_node.predicate();
            self.src(c.location().start_offset(), c.location().end_offset())
        };
        // Ternary branches are bare values (not assignments), use raw src
        let if_rhs = self.get_branch_raw_src(&if_node.statements())?;
        let else_rhs = match if_node.subsequent() {
            Some(sub) if matches!(&sub, Node::ElseNode { .. }) => {
                self.get_branch_raw_src(&sub.as_else_node().unwrap().statements())?
            }
            _ => return None,
        };
        // assign_inside_condition: push lhs into each branch of ternary. Never needs parens here.
        let expr = format!("{} ? {}{} : {}{}", cond_src, lhs, if_rhs, lhs, else_rhs);
        Some(Correction::replace(
            outer_node.location().start_offset(),
            outer_node.location().end_offset(),
            expr,
        ))
    }

    /// Rewrite if node for assign_inside_condition, returning new source string.
    fn rewrite_if_assign_inside(&self, if_node_raw: &Node, lhs: &str, indent: &str, branch_indent: &str, is_elsif: bool) -> Option<String> {
        let if_node = if_node_raw.as_if_node().unwrap();
        let cond_src = {
            let c = if_node.predicate();
            self.src(c.location().start_offset(), c.location().end_offset())
        };

        // Detect "then" keyword style
        let node_src = self.src(if_node_raw.location().start_offset(), if_node_raw.location().end_offset());
        let keyword = if is_elsif { "elsif" } else { "if" };
        let has_then = node_src.starts_with(&format!("{} {} then", keyword, cond_src)) ||
            node_src.starts_with(&format!("{} {}", keyword, cond_src)) && {
                // check for " then " in first line
                let first_line_end = node_src.find('\n').unwrap_or(node_src.len());
                node_src[..first_line_end].contains(" then ")
            };

        let if_body = self.rewrite_branch_body_inline(&if_node.statements(), lhs, branch_indent)?;

        // Check subsequent (else / elsif)
        let subsequent = if_node.subsequent();

        let result = match subsequent {
            None => {
                // No else - but we got here, so must have elsif only (like "if foo; elsif bar" without else)
                if has_then {
                    format!("{}{} {} then {}", indent, keyword, cond_src, if_body.trim())
                } else {
                    format!("{}{} {}\n{}", indent, keyword, cond_src, if_body)
                }
            }
            Some(sub) => {
                match &sub {
                    Node::IfNode { .. } => {
                        // elsif branch
                        let elsif_part = self.rewrite_if_assign_inside(&sub, lhs, indent, branch_indent, true)?;
                        // Check if the elsif chain terminates with an else or not
                        // If no else in whole chain, we need to append "end" at root level
                        let has_final_else = self.if_chain_has_else(&sub);
                        if has_then {
                            if is_elsif || has_final_else {
                                format!("{}{} {} then {}\n{}", indent, keyword, cond_src, if_body.trim(), elsif_part)
                            } else {
                                format!("{}{} {} then {}\n{}\n{}end", indent, keyword, cond_src, if_body.trim(), elsif_part, indent)
                            }
                        } else {
                            if is_elsif || has_final_else {
                                format!("{}{} {}\n{}\n{}", indent, keyword, cond_src, if_body, elsif_part)
                            } else {
                                format!("{}{} {}\n{}\n{}\n{}end", indent, keyword, cond_src, if_body, elsif_part, indent)
                            }
                        }
                    }
                    Node::ElseNode { .. } => {
                        let else_node = sub.as_else_node().unwrap();
                        let else_src_raw = self.src(sub.location().start_offset(), sub.location().end_offset());
                        let else_body = self.rewrite_branch_body_inline(&else_node.statements(), lhs, branch_indent)?;
                        // Handle "else X end" inline
                        let has_end_on_else_line = {
                            let else_src = self.src(sub.location().start_offset(), if_node_raw.location().end_offset());
                            let newlines = else_src.chars().filter(|&c| c == '\n').count();
                            newlines == 0
                        };

                        if has_then {
                            // "if cond then ... else ... end"
                            let else_trimmed = else_body.trim().to_string();
                            // If entire original node is single-line, keep single-line output
                            let is_single_line = !node_src.contains('\n');
                            if is_single_line {
                                format!("{}{} {} then {} else {} end", indent, keyword, cond_src, if_body.trim(), else_trimmed)
                            } else {
                                // Multi-line: keep else inline (then-style), end on own line
                                format!("{}{} {} then {}\n{}else {}\n{}end", indent, keyword, cond_src, if_body.trim(), indent, else_trimmed, indent)
                            }
                        } else {
                            // Check if else was inline in original: "else val end" on one line after if body
                            let else_has_newline = else_src_raw.contains('\n');
                            if !else_has_newline {
                                // inline else: "else bar = val end"
                                let else_trimmed = else_body.trim().to_string();
                                format!("{}{} {}\n{}\n{}else {} end", indent, keyword, cond_src, if_body, indent, else_trimmed)
                            } else {
                                format!("{}{} {}\n{}\n{}else\n{}\n{}end", indent, keyword, cond_src, if_body, indent, else_body, indent)
                            }
                        }
                    }
                    _ => return None,
                }
            }
        };
        Some(result)
    }

    fn if_chain_has_else(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                match if_node.subsequent() {
                    None => false,
                    Some(sub) => match &sub {
                        Node::ElseNode { .. } => true,
                        Node::IfNode { .. } => self.if_chain_has_else(&sub),
                        _ => false,
                    }
                }
            }
            Node::ElseNode { .. } => true,
            _ => false,
        }
    }

    /// Get the last stmt's RHS source from a statements node (strips assignment LHS if present).
    /// For assign_to_condition branches (where stmts contain assignments like `bar = 1`).
    fn get_tail_rhs_src(&self, stmts: &Option<ruby_prism::StatementsNode>) -> Option<String> {
        let stmts = stmts.as_ref()?;
        let last = stmts.body().iter().last()?;
        let rhs = self.extract_rhs_node_for_src(&last)?;
        let rhs_src = self.src(rhs.location().start_offset(), rhs.location().end_offset());
        Some(rhs_src.to_string())
    }

    /// Rewrite a branch's statements for assign_inside_condition:
    /// The branch body contains bare values (not assignments). Prepend `lhs` to the last stmt.
    fn rewrite_branch_body_inline(&self, stmts: &Option<ruby_prism::StatementsNode>, lhs: &str, indent: &str) -> Option<String> {
        let stmts = stmts.as_ref()?;
        let body: Vec<Node> = stmts.body().iter().collect();
        if body.is_empty() { return None; }

        let mut lines = Vec::new();
        // All but last: just re-indent
        for node in &body[..body.len()-1] {
            let src = self.src(node.location().start_offset(), node.location().end_offset());
            lines.push(format!("{}{}", indent, src.trim()));
        }
        // Last: the bare value (not an assignment) — prepend lhs.
        // Use true_end_offset to include heredoc body if the last node has heredoc args.
        let last = &body[body.len()-1];
        let last_end = self.true_end_offset(last);
        let last_src = self.src(last.location().start_offset(), last_end);
        lines.push(format!("{}{}{}", indent, lhs, last_src.trim()));
        Some(lines.join("\n"))
    }

    fn rewrite_else_body_inline(&self, else_clause: &Option<ruby_prism::ElseNode>, lhs: &str, indent: &str) -> Option<String> {
        let else_node = else_clause.as_ref()?;
        self.rewrite_branch_body_inline(&else_node.statements(), lhs, indent)
    }

    fn extract_rhs_node_for_src<'b>(&self, node: &'b Node) -> Option<Node<'b>> {
        match_write_nodes!(self, node, extract_rhs_node_inner)
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

    /// Get the true end offset of a node, accounting for heredoc arguments.
    /// In Prism, CallNode.location().end_offset() only covers the call syntax (up to `)`)
    /// but not heredoc body/closing-marker which appear on subsequent lines.
    fn true_end_offset(&self, node: &Node) -> usize {
        let base_end = node.location().end_offset();
        // Walk arguments of call nodes to find any heredoc (InterpolatedStringNode whose
        // source starts with `<<`)
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if let Some(args) = call.arguments() {
                let mut max_end = base_end;
                for arg in args.arguments().iter() {
                    if let Node::InterpolatedStringNode { .. } = &arg {
                        let heredoc_token = self.src(arg.location().start_offset(), arg.location().end_offset());
                        if heredoc_token.starts_with("<<") {
                            // Heredoc body starts after the call's closing line.
                            // Scan forward from base_end for the closing marker.
                            // The closing marker is on its own line; find the next \n after all body.
                            // Body elements: their end is the last body char before closing marker.
                            let istr = arg.as_interpolated_string_node().unwrap();
                            let body_parts: Vec<Node> = istr.parts().iter().collect();
                            if let Some(last_part) = body_parts.last() {
                                let body_end = last_part.location().end_offset();
                                // Find the closing marker: scan from body_end for end of line
                                let src_bytes = self.source.as_bytes();
                                let mut pos = body_end;
                                // Skip through the closing marker line (ends with \n)
                                while pos < src_bytes.len() && src_bytes[pos] != b'\n' { pos += 1; }
                                if pos < src_bytes.len() { pos += 1; } // skip the \n
                                max_end = max_end.max(pos);
                            }
                        }
                    }
                }
                return max_end;
            }
        }
        base_end
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
        let (cond_start, cond_end) = (node.location().start_offset(), node.location().end_offset());
        self.check_branch_infos_same_assignment_if(cond_start, cond_end, &branch_infos, node);
    }

    fn check_assign_to_condition_unless(&mut self, node: &ruby_prism::UnlessNode) {
        if node.else_clause().is_none() { return; }
        let mut branch_infos = vec![self.branch_info_from_stmts(&node.statements())];
        if let Some(ec) = node.else_clause() {
            branch_infos.push(self.branch_info_from_stmts(&ec.statements()));
        }
        let (cond_start, cond_end) = (node.location().start_offset(), node.location().end_offset());
        self.check_branch_infos_same_assignment_unless(cond_start, cond_end, &branch_infos, node);
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
        let (cond_start, cond_end) = (node.location().start_offset(), node.location().end_offset());
        self.check_branch_infos_same_assignment_case(cond_start, cond_end, &branch_infos, node);
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
        let (cond_start, cond_end) = (node.location().start_offset(), node.location().end_offset());
        self.check_branch_infos_same_assignment_case_match(cond_start, cond_end, &branch_infos, node);
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
        let (cond_start, cond_end) = (node.location().start_offset(), node.location().end_offset());
        let branch_infos = [if_info, else_info];
        // Check same assignment
        if !self.branches_have_same_assignment(&branch_infos) { return; }
        if let Some(lhs) = branch_infos[0].tail_assignment.as_ref().map(|(l, _)| l.clone()) {
            if self.correction_exceeds_line_limit(cond_start, cond_end, &lhs) { return; }
            if let Some(correction) = self.correct_assign_to_condition_ternary(node) {
                self.add_offense_with_correction(cond_start, cond_end, MSG, correction);
            } else {
                self.add_offense(cond_start, cond_end, MSG);
            }
        }
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

    fn branches_have_same_assignment(&self, branches: &[BranchInfo]) -> bool {
        if branches.is_empty() { return false; }
        let mut first_lhs: Option<&str> = None;
        let mut first_kind: Option<&str> = None;
        for branch in branches {
            let (lhs, kind) = match &branch.tail_assignment { Some(a) => a, None => return false };
            if first_lhs.map_or(false, |fl| fl != lhs) { return false; }
            if first_kind.map_or(false, |fk| fk != kind) { return false; }
            first_lhs = Some(lhs);
            first_kind = Some(kind);
        }
        true
    }

    fn check_branch_infos_same_assignment(&mut self, cond_start: usize, cond_end: usize, branches: &[BranchInfo]) {
        if branches.is_empty() { return; }
        if !self.branches_have_same_assignment(branches) { return; }
        if self.single_line_only && branches.iter().any(|b| b.stmt_count > 1) { return; }
        if let Some(assignment_lhs) = branches[0].tail_assignment.as_ref().map(|(l, _)| l.as_str()) {
            if self.correction_exceeds_line_limit(cond_start, cond_end, assignment_lhs) { return; }
        }
        self.add_offense(cond_start, cond_end, MSG);
    }

    fn check_branch_infos_same_assignment_if(&mut self, cond_start: usize, cond_end: usize, branches: &[BranchInfo], node: &ruby_prism::IfNode) {
        if branches.is_empty() { return; }
        if !self.branches_have_same_assignment(branches) { return; }
        if self.single_line_only && branches.iter().any(|b| b.stmt_count > 1) { return; }
        let lhs = match branches[0].tail_assignment.as_ref().map(|(l, _)| l.clone()) {
            Some(l) => l,
            None => return,
        };
        if self.correction_exceeds_line_limit(cond_start, cond_end, &lhs) { return; }
        if let Some(correction) = self.correct_assign_to_condition_if(node, &lhs) {
            self.add_offense_with_correction(cond_start, cond_end, MSG, correction);
        } else {
            self.add_offense(cond_start, cond_end, MSG);
        }
    }

    fn check_branch_infos_same_assignment_unless(&mut self, cond_start: usize, cond_end: usize, branches: &[BranchInfo], node: &ruby_prism::UnlessNode) {
        if branches.is_empty() { return; }
        if !self.branches_have_same_assignment(branches) { return; }
        if self.single_line_only && branches.iter().any(|b| b.stmt_count > 1) { return; }
        let lhs = match branches[0].tail_assignment.as_ref().map(|(l, _)| l.clone()) {
            Some(l) => l,
            None => return,
        };
        if self.correction_exceeds_line_limit(cond_start, cond_end, &lhs) { return; }
        if let Some(correction) = self.correct_assign_to_condition_unless(node, &lhs) {
            self.add_offense_with_correction(cond_start, cond_end, MSG, correction);
        } else {
            self.add_offense(cond_start, cond_end, MSG);
        }
    }

    fn check_branch_infos_same_assignment_case(&mut self, cond_start: usize, cond_end: usize, branches: &[BranchInfo], node: &ruby_prism::CaseNode) {
        if branches.is_empty() { return; }
        if !self.branches_have_same_assignment(branches) { return; }
        if self.single_line_only && branches.iter().any(|b| b.stmt_count > 1) { return; }
        let lhs = match branches[0].tail_assignment.as_ref().map(|(l, _)| l.clone()) {
            Some(l) => l,
            None => return,
        };
        if self.correction_exceeds_line_limit(cond_start, cond_end, &lhs) { return; }
        if let Some(correction) = self.correct_assign_to_condition_case_node(node, &lhs) {
            self.add_offense_with_correction(cond_start, cond_end, MSG, correction);
        } else {
            self.add_offense(cond_start, cond_end, MSG);
        }
    }

    fn check_branch_infos_same_assignment_case_match(&mut self, cond_start: usize, cond_end: usize, branches: &[BranchInfo], node: &ruby_prism::CaseMatchNode) {
        if branches.is_empty() { return; }
        if !self.branches_have_same_assignment(branches) { return; }
        if self.single_line_only && branches.iter().any(|b| b.stmt_count > 1) { return; }
        let lhs = match branches[0].tail_assignment.as_ref().map(|(l, _)| l.clone()) {
            Some(l) => l,
            None => return,
        };
        if self.correction_exceeds_line_limit(cond_start, cond_end, &lhs) { return; }
        if let Some(correction) = self.correct_assign_to_condition_case_match_node(node, &lhs) {
            self.add_offense_with_correction(cond_start, cond_end, MSG, correction);
        } else {
            self.add_offense(cond_start, cond_end, MSG);
        }
    }

    // -------------------------------------------------------------------------
    // assign_to_condition corrections (lift assignment out)
    // -------------------------------------------------------------------------

    /// Wrap array RHS source in brackets if it's an implicit array (no brackets in source).
    fn wrap_array_if_needed<'b>(&self, rhs: &Node<'b>, rhs_src: &str) -> String {
        if matches!(rhs, Node::ArrayNode { .. }) && !rhs_src.starts_with('[') {
            format!("[{}]", rhs_src)
        } else {
            rhs_src.to_string()
        }
    }

    /// Strip assignment LHS from a single-statement branch, return just the RHS source.
    /// Preserves source text between stmts node start and first statement (comments).
    fn branch_rhs_src(&self, stmts: &Option<ruby_prism::StatementsNode>) -> Option<String> {
        self.branch_rhs_src_with_prefix(stmts, None)
    }

    /// branch_rhs_src with optional prefix_start for comment capture.
    fn branch_rhs_src_with_prefix(&self, stmts: &Option<ruby_prism::StatementsNode>, prefix_start: Option<usize>) -> Option<String> {
        let stmts = stmts.as_ref()?;
        let body: Vec<Node> = stmts.body().iter().collect();
        let last = body.last()?;
        let rhs = self.extract_rhs_node_for_src(last)?;
        let rhs_src = self.src(rhs.location().start_offset(), rhs.location().end_offset());
        let rhs_str = self.wrap_array_if_needed(&rhs, rhs_src);

        if body.len() == 1 {
            // Single stmt: check for prefix (comments) before this stmt
            if let Some(ps) = prefix_start {
                let first_stmt_start = last.location().start_offset();
                if ps < first_stmt_start {
                    let prefix_src = &self.source[ps..first_stmt_start];
                    // Only include if there's meaningful content (not just whitespace)
                    let prefix_trimmed = prefix_src.trim_start_matches(&[' ', '\t'][..]);
                    if !prefix_trimmed.is_empty() {
                        return Some(format!("{}{}", prefix_src, rhs_str));
                    }
                }
            }
            Some(rhs_str)
        } else {
            // Multi-stmt: use source from prefix_start (or first stmt) to last stmt end,
            // replacing the tail assignment with its rhs.
            let region_start = prefix_start.unwrap_or_else(|| body[0].location().start_offset());
            let tail_start = last.location().start_offset();
            let rhs_start = rhs.location().start_offset();

            // Everything from region_start to the tail stmt's lhs end, then rhs
            let prefix = &self.source[region_start..tail_start];
            // Find where the lhs ends — from tail_start to rhs_start
            // (assignment operator + whitespace)
            // We just skip from tail_start to rhs_start
            let suffix = &self.source[rhs_start..rhs.location().end_offset()];
            Some(format!("{}{}", prefix, self.wrap_array_if_needed(&rhs, suffix)))
        }
    }

    /// Compute end indent: either col-of-cond (start_of_line) or col+lhs_len (keyword).
    fn end_indent(&self, lhs: &str, cond_start: usize) -> String {
        let col = self.col_of(cond_start);
        if self.end_alignment_keyword {
            // Keyword: align end with the start of the full expression (col + lhs)
            " ".repeat(col + lhs.len())
        } else {
            // start_of_line: align end with start of conditional
            " ".repeat(col)
        }
    }

    fn correct_assign_to_condition_ternary(&self, node: &ruby_prism::IfNode) -> Option<Correction> {
        // `foo? ? bar = "a" : bar = "b"` → `bar = foo? ? "a" : "b"`
        let cond_src = {
            let c = node.predicate();
            self.src(c.location().start_offset(), c.location().end_offset())
        };
        let if_stmts = node.statements();
        let else_stmts = match node.subsequent() {
            Some(sub) if matches!(&sub, Node::ElseNode { .. }) => sub.as_else_node().unwrap().statements(),
            _ => return None,
        };
        let if_rhs = self.branch_rhs_src(&if_stmts)?;
        let else_rhs = self.branch_rhs_src(&else_stmts)?;

        // Get the tail assignment node to extract lhs
        let if_stmts_ref = if_stmts.as_ref()?;
        let last = if_stmts_ref.body().iter().last()?;
        let lhs = self.get_lhs_text(&last)?;

        // element_assignment / comparison-method check (need paren wrap around ternary rhs).
        // Compound operator-assignments like <<=, >>= end in <= or >= but are NOT comparisons.
        let lhs_trimmed = lhs.trim_end();
        let is_compound_assign = lhs_trimmed.ends_with("<<=") || lhs_trimmed.ends_with(">>=");
        // Setter method calls (foo.bar =) need parens, but element assignments (foo[k] =) do not.
        // Detect setter by presence of `.` in the lhs (not `[]=` which has `[` but no `.`).
        let is_call_assign = lhs_trimmed.contains('.');
        let needs_parens = is_call_assign || (!is_compound_assign && (
            lhs_trimmed.ends_with("==") || lhs_trimmed.ends_with("!=") ||
            lhs_trimmed.ends_with("===") || lhs_trimmed.ends_with("=~") ||
            lhs_trimmed.ends_with("!~") || lhs_trimmed.ends_with("<=>") ||
            lhs_trimmed.ends_with('<') || lhs_trimmed.ends_with('>') ||
            lhs_trimmed.ends_with(">=") || lhs_trimmed.ends_with("<=") ||
            lhs_trimmed.ends_with("<<")));

        let new_src = if needs_parens {
            format!("{}({} ? {} : {})", lhs, cond_src, if_rhs, else_rhs)
        } else {
            format!("{}{} ? {} : {}", lhs, cond_src, if_rhs, else_rhs)
        };

        Some(Correction::replace(
            node.location().start_offset(),
            node.location().end_offset(),
            new_src,
        ))
    }

    fn correct_assign_to_condition_if(&self, node: &ruby_prism::IfNode, lhs: &str) -> Option<Correction> {
        // `if c; bar = 1; else; bar = 2; end` → `bar = if c; 1; else; 2; end`
        let cond_start = node.location().start_offset();
        let cond_end = node.location().end_offset();
        let col = self.col_of(cond_start);
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        // Build new source: prepend lhs, rewrite each branch removing assignment
        let new_body = self.rewrite_if_remove_lhs(node, lhs, &indent, &branch_indent, false)?;
        let new_src = format!("{}{}", lhs, new_body);

        Some(Correction::replace(cond_start, cond_end, new_src))
    }

    fn rewrite_if_remove_lhs(&self, node: &ruby_prism::IfNode, lhs: &str, indent: &str, branch_indent: &str, is_elsif: bool) -> Option<String> {
        let cond_src = {
            let c = node.predicate();
            self.src(c.location().start_offset(), c.location().end_offset())
        };

        let keyword = if is_elsif { "elsif" } else { "if" };
        // Detect "then" keyword style
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let node_src_full = self.src(node_start, node_end);
        let first_line_end = node_src_full.find('\n').unwrap_or(node_src_full.len());
        let first_line = &node_src_full[..first_line_end];
        let has_then = first_line.contains(" then ");

        // Find branch body start (after condition line) to capture comments before first stmt
        let branch_body_start = {
            let pred_end = node.predicate().location().end_offset();
            let src_bytes = self.source.as_bytes();
            let mut pos = pred_end;
            while pos < src_bytes.len() && src_bytes[pos] != b'\n' { pos += 1; }
            if pos < src_bytes.len() { pos + 1 } else { pred_end }
        };
        let if_body = self.branch_rhs_src_with_prefix(&node.statements(), Some(branch_body_start))?;
        let if_stmt_count = Self::stmts_count(&node.statements());

        match node.subsequent() {
            None => {
                // No else (only possible in elsif chain without final else)
                if has_then {
                    Some(format!("{} {} then {}", keyword, cond_src, if_body))
                } else {
                    let if_lines = if if_stmt_count > 1 {
                        // multi-stmt: each line indented
                        if_body.lines().enumerate().map(|(i, l)| {
                            if i == 0 { format!("{}{}", branch_indent, l.trim()) }
                            else { format!("{}{}", branch_indent, l.trim()) }
                        }).collect::<Vec<_>>().join("\n")
                    } else {
                        format!("{}{}", branch_indent, if_body.trim())
                    };
                    Some(format!("{} {}\n{}", keyword, cond_src, if_lines))
                }
            }
            Some(sub) => {
                match &sub {
                    Node::IfNode { .. } => {
                        let elsif_node = sub.as_if_node().unwrap();
                        let elsif_part = self.rewrite_if_remove_lhs(&elsif_node, lhs, indent, branch_indent, true)?;
                        if has_then {
                            Some(format!("{} {} then {}\n{}{}", keyword, cond_src, if_body, indent, elsif_part))
                        } else {
                            let if_line = format!("{}{}", branch_indent, if_body.trim());
                            Some(format!("{} {}\n{}\n{}{}", keyword, cond_src, if_line, indent, elsif_part))
                        }
                    }
                    Node::ElseNode { .. } => {
                        let else_node = sub.as_else_node().unwrap();
                        // Find else body start (after `else\n`) for comment capture
                        let else_body_start = {
                            let else_kw_end = sub.location().start_offset() + "else".len();
                            let src_bytes = self.source.as_bytes();
                            let mut pos = else_kw_end;
                            while pos < src_bytes.len() && src_bytes[pos] != b'\n' { pos += 1; }
                            if pos < src_bytes.len() { pos + 1 } else { else_kw_end }
                        };
                        let else_body = self.branch_rhs_src_with_prefix(&else_node.statements(), Some(else_body_start))?;
                        let else_stmt_count = Self::stmts_count(&else_node.statements());

                        let end_indent_str = self.end_indent(lhs, node_start);

                        if has_then {
                            // Check if else is inline too
                            let else_src_full = self.src(sub.location().start_offset(), node_end);
                            let else_first_line_end = else_src_full.find('\n').unwrap_or(else_src_full.len());
                            let else_first_line = &else_src_full[..else_first_line_end];
                            // "else bar = 2" → inline
                            let else_inline = !else_src_full.starts_with("else\n") && !else_src_full.starts_with("else \n");
                            if else_inline {
                                Some(format!("{} {} then {}\n{}else {}\n{}end", keyword, cond_src, if_body, indent, else_body, end_indent_str))
                            } else {
                                let else_line = format!("{}{}", branch_indent, else_body.trim());
                                Some(format!("{} {} then {}\n{}else\n{}\n{}end", keyword, cond_src, if_body, indent, else_line, end_indent_str))
                            }
                        } else {
                            let if_line = if if_stmt_count > 1 {
                                if_body.lines().map(|l| format!("{}{}", branch_indent, l.trim())).collect::<Vec<_>>().join("\n")
                            } else {
                                format!("{}{}", branch_indent, if_body.trim())
                            };
                            let else_line = if else_stmt_count > 1 {
                                else_body.lines().map(|l| format!("{}{}", branch_indent, l.trim())).collect::<Vec<_>>().join("\n")
                            } else {
                                format!("{}{}", branch_indent, else_body.trim())
                            };
                            Some(format!("{} {}\n{}\n{}else\n{}\n{}end", keyword, cond_src, if_line, indent, else_line, end_indent_str))
                        }
                    }
                    _ => None,
                }
            }
        }
    }

    fn correct_assign_to_condition_unless(&self, node: &ruby_prism::UnlessNode, lhs: &str) -> Option<Correction> {
        let cond_start = node.location().start_offset();
        let cond_end = node.location().end_offset();
        let col = self.col_of(cond_start);
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        let cond_src = {
            let c = node.predicate();
            self.src(c.location().start_offset(), c.location().end_offset())
        };
        let if_body = self.branch_rhs_src(&node.statements())?;
        let else_body = node.else_clause().and_then(|ec| self.branch_rhs_src(&ec.statements()))?;

        let end_indent_str = self.end_indent(lhs, cond_start);

        let new_body = format!("unless {}\n{}{}\n{}else\n{}{}\n{}end",
            cond_src,
            branch_indent, if_body.trim(),
            indent,
            branch_indent, else_body.trim(),
            end_indent_str);
        let new_src = format!("{}{}", lhs, new_body);
        Some(Correction::replace(cond_start, cond_end, new_src))
    }

    fn correct_assign_to_condition_case_node(&self, node: &ruby_prism::CaseNode, lhs: &str) -> Option<Correction> {
        let cond_start = node.location().start_offset();
        let cond_end = node.location().end_offset();
        let col = self.col_of(cond_start);
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        let subject = node.predicate().map(|p| {
            format!(" {}", self.src(p.location().start_offset(), p.location().end_offset()))
        }).unwrap_or_default();

        let mut parts = vec![format!("{}case{}", indent, subject)];

        for cond in node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                let when_node = cond.as_when_node().unwrap();
                let conds_src: Vec<String> = when_node.conditions().iter()
                    .map(|c| self.src(c.location().start_offset(), c.location().end_offset()).to_string())
                    .collect();

                let when_src = self.src(cond.location().start_offset(), cond.location().end_offset());
                let has_then = when_src.contains(" then ");

                // Find when body start (after condition line) for comment capture
                let when_body_start = {
                    let last_cond_end = when_node.conditions().iter().last()
                        .map(|c| c.location().end_offset())
                        .unwrap_or(cond.location().start_offset());
                    let src_bytes = self.source.as_bytes();
                    let mut pos = last_cond_end;
                    while pos < src_bytes.len() && src_bytes[pos] != b'\n' { pos += 1; }
                    if pos < src_bytes.len() { pos + 1 } else { last_cond_end }
                };
                let body = self.branch_rhs_src_with_prefix(&when_node.statements(), Some(when_body_start))?;
                if has_then {
                    parts.push(format!("{}when {} then {}", indent, conds_src.join(", "), body.trim()));
                } else {
                    parts.push(format!("{}when {}\n{}{}", indent, conds_src.join(", "), branch_indent, body.trim()));
                }
            }
        }

        if let Some(else_clause) = node.else_clause() {
            let else_src = self.src(else_clause.location().start_offset(), else_clause.location().end_offset());
            // Find else body start for comment capture
            let else_body_start = {
                let else_kw_end = else_clause.location().start_offset() + "else".len();
                let src_bytes = self.source.as_bytes();
                let mut pos = else_kw_end;
                while pos < src_bytes.len() && src_bytes[pos] != b'\n' { pos += 1; }
                if pos < src_bytes.len() { pos + 1 } else { else_kw_end }
            };
            let else_body = self.branch_rhs_src_with_prefix(&else_clause.statements(), Some(else_body_start))?;
            let else_inline = !else_src.starts_with("else\n");
            if else_inline {
                parts.push(format!("{}else {}", indent, else_body.trim()));
            } else {
                parts.push(format!("{}else\n{}{}", indent, branch_indent, else_body.trim()));
            }
        }

        let end_indent_str = self.end_indent(lhs, cond_start);
        parts.push(format!("{}end", end_indent_str));

        let body = parts.join("\n");
        let new_src = format!("{}{}", lhs, body);
        Some(Correction::replace(cond_start, cond_end, new_src))
    }

    fn correct_assign_to_condition_case_match_node(&self, node: &ruby_prism::CaseMatchNode, lhs: &str) -> Option<Correction> {
        let cond_start = node.location().start_offset();
        let cond_end = node.location().end_offset();
        let col = self.col_of(cond_start);
        let indent = " ".repeat(col);
        let branch_indent = " ".repeat(col + 2);

        let subject = node.predicate().map(|p| {
            format!(" {}", self.src(p.location().start_offset(), p.location().end_offset()))
        }).unwrap_or_default();

        let mut parts = vec![format!("{}case{}", indent, subject)];

        for cond in node.conditions().iter() {
            if let Node::InNode { .. } = &cond {
                let in_node = cond.as_in_node().unwrap();
                let pat = in_node.pattern();
                let pat_src = self.src(pat.location().start_offset(), pat.location().end_offset());
                let body = self.branch_rhs_src(&in_node.statements())?;
                parts.push(format!("{}in {}\n{}{}", indent, pat_src, branch_indent, body.trim()));
            }
        }

        if let Some(else_clause) = node.else_clause() {
            let else_body = self.branch_rhs_src(&else_clause.statements())?;
            parts.push(format!("{}else\n{}{}", indent, branch_indent, else_body.trim()));
        }

        let end_indent_str = self.end_indent(lhs, cond_start);
        parts.push(format!("{}end", end_indent_str));

        let body = parts.join("\n");
        let new_src = format!("{}{}", lhs, body);
        Some(Correction::replace(cond_start, cond_end, new_src))
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
        macro_rules! simple_write {
            ($node_type:ident, $kind:expr) => {{
                let n = node.$node_type().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} = ", name), $kind.to_string()))
            }};
        }
        macro_rules! op_write {
            ($node_type:ident, $kind:expr) => {{
                let n = node.$node_type().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                let op = std::str::from_utf8(n.binary_operator().as_slice()).unwrap_or("");
                Some((format!("{} {}= ", name, op), $kind.to_string()))
            }};
        }
        macro_rules! bool_write {
            ($node_type:ident, $op:expr, $kind:expr) => {{
                let n = node.$node_type().unwrap();
                let name = std::str::from_utf8(n.name().as_slice()).unwrap_or("");
                Some((format!("{} {} ", name, $op), $kind.to_string()))
            }};
        }
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

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct EndAlignCfg { enforced_style_align_with: String }
impl Default for EndAlignCfg {
    fn default() -> Self { Self { enforced_style_align_with: "start_of_line".to_string() } }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String, include_ternary_expressions: bool, single_line_conditions_only: bool }
impl Default for Cfg {
    fn default() -> Self {
        Self { enforced_style: String::new(), include_ternary_expressions: true, single_line_conditions_only: true }
    }
}

crate::register_cop!("Style/ConditionalAssignment", |cfg| {
    let c: Cfg = cfg.typed("Style/ConditionalAssignment");
    let ea: EndAlignCfg = cfg.typed("Layout/EndAlignment");
    let style = match c.enforced_style.as_str() {
        "assign_to_condition" => EnforcedStyle::AssignToCondition,
        _ => EnforcedStyle::AssignInsideCondition,
    };
    let end_alignment_keyword = ea.enforced_style_align_with == "keyword";
    let mut cop = ConditionalAssignment::with_config(
        style, c.include_ternary_expressions, c.single_line_conditions_only,
    );
    cop.end_alignment_keyword = end_alignment_keyword;
    Some(Box::new(cop))
});
