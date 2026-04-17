//! Style/SoleNestedConditional - Detect if/unless nested inside another if/unless
//! that could be combined with `&&`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/sole_nested_conditional.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/SoleNestedConditional";

pub struct SoleNestedConditional {
    allow_modifier: bool,
}

impl SoleNestedConditional {
    pub fn new() -> Self {
        Self {
            allow_modifier: false,
        }
    }

    pub fn with_config(allow_modifier: bool) -> Self {
        Self { allow_modifier }
    }
}

impl Default for SoleNestedConditional {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for SoleNestedConditional {
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
        let mut visitor = SoleNestedConditionalVisitor {
            ctx,
            allow_modifier: self.allow_modifier,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SoleNestedConditionalVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    allow_modifier: bool,
    offenses: Vec<Offense>,
}

impl<'a> SoleNestedConditionalVisitor<'a> {
    /// Check an if/unless node for sole nested conditional pattern
    fn check_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Skip ternaries, elsif, or nodes with else branches
        if is_ternary_if(node, self.ctx.source) {
            return;
        }
        if is_elsif(node, self.ctx.source) {
            return;
        }
        if node.subsequent().is_some() {
            return;
        }

        let if_branch = match get_if_branch_if(node) {
            Some(b) => b,
            None => return,
        };

        // Check for variable assignment in condition that's used in inner condition
        if use_variable_assignment_in_condition_if(node, &if_branch, self.ctx.source) {
            return;
        }

        if !self.offending_branch_from_if(node, &if_branch) {
            return;
        }

        // Determine the keyword for the message based on outer node
        let keyword = if_keyword_text_if(node, self.ctx.source);
        let message = format!(
            "Consider merging nested conditions into outer `{}` conditions.",
            keyword
        );

        // Offense location is the keyword of the inner if
        let inner_keyword_loc = inner_keyword_loc(&if_branch);
        if let Some((start, end)) = inner_keyword_loc {
            self.offenses.push(self.ctx.offense_with_range(
                COP_NAME,
                &message,
                Severity::Convention,
                start,
                end,
            ));
        }
    }

    fn check_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        // unless nodes don't have ternary/elsif
        if node.else_clause().is_some() {
            return;
        }

        let if_branch = match get_if_branch_unless(node) {
            Some(b) => b,
            None => return,
        };

        if use_variable_assignment_in_condition_unless(node, &if_branch, self.ctx.source) {
            return;
        }

        if !self.offending_branch_from_unless(node, &if_branch) {
            return;
        }

        let message = "Consider merging nested conditions into outer `unless` conditions.";

        let inner_keyword_loc = inner_keyword_loc(&if_branch);
        if let Some((start, end)) = inner_keyword_loc {
            self.offenses.push(self.ctx.offense_with_range(
                COP_NAME,
                message,
                Severity::Convention,
                start,
                end,
            ));
        }
    }

    fn offending_branch_from_if(
        &self,
        outer: &ruby_prism::IfNode,
        branch: &InnerConditional,
    ) -> bool {
        if branch.has_else {
            return false;
        }
        if branch.is_ternary {
            return false;
        }
        let outer_is_modifier = is_modifier_form_if(outer, self.ctx.source);
        if (outer_is_modifier || branch.is_modifier) && self.allow_modifier {
            return false;
        }
        true
    }

    fn offending_branch_from_unless(
        &self,
        outer: &ruby_prism::UnlessNode,
        branch: &InnerConditional,
    ) -> bool {
        if branch.has_else {
            return false;
        }
        if branch.is_ternary {
            return false;
        }
        let outer_is_modifier = is_modifier_form_unless(outer, self.ctx.source);
        if (outer_is_modifier || branch.is_modifier) && self.allow_modifier {
            return false;
        }
        true
    }
}

/// Represents the inner conditional branch info
struct InnerConditional {
    has_else: bool,
    is_ternary: bool,
    is_modifier: bool,
    keyword_start: usize,
    keyword_end: usize,
}

fn inner_keyword_loc(inner: &InnerConditional) -> Option<(usize, usize)> {
    Some((inner.keyword_start, inner.keyword_end))
}

/// Get the sole if_branch from an IfNode's body, if it's a single if/unless node
fn get_if_branch_if(node: &ruby_prism::IfNode) -> Option<InnerConditional> {
    let stmts = node.statements()?;
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 {
        return None;
    }
    extract_inner_conditional(&body[0])
}

/// Get the sole if_branch from an UnlessNode's body
fn get_if_branch_unless(node: &ruby_prism::UnlessNode) -> Option<InnerConditional> {
    let stmts = node.statements()?;
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 {
        return None;
    }
    extract_inner_conditional(&body[0])
}

fn extract_inner_conditional(node: &Node) -> Option<InnerConditional> {
    match node {
        Node::IfNode { .. } => {
            let if_node = node.as_if_node().unwrap();
            let has_else = if_node.subsequent().is_some();
            let is_ternary = if_node.if_keyword_loc().is_none();
            let is_modifier = if_node.end_keyword_loc().is_none() && !is_ternary;

            let (keyword_start, keyword_end) = if let Some(kw_loc) = if_node.if_keyword_loc() {
                (kw_loc.start_offset(), kw_loc.end_offset())
            } else {
                // ternary - use start of node
                let loc = node.location();
                (loc.start_offset(), loc.start_offset() + 2)
            };

            Some(InnerConditional {
                has_else,
                is_ternary,
                is_modifier,
                keyword_start,
                keyword_end,
            })
        }
        Node::UnlessNode { .. } => {
            let unless_node = node.as_unless_node().unwrap();
            let has_else = unless_node.else_clause().is_some();
            let is_modifier = unless_node.end_keyword_loc().is_none();

            let kw_loc = unless_node.keyword_loc();
            Some(InnerConditional {
                has_else,
                is_ternary: false,
                is_modifier,
                keyword_start: kw_loc.start_offset(),
                keyword_end: kw_loc.end_offset(),
            })
        }
        _ => None,
    }
}

fn is_ternary_if(node: &ruby_prism::IfNode, _source: &str) -> bool {
    node.if_keyword_loc().is_none()
}

fn is_elsif(node: &ruby_prism::IfNode, source: &str) -> bool {
    node.if_keyword_loc().map_or(false, |loc| {
        source[loc.start_offset()..].starts_with("elsif")
    })
}

fn is_modifier_form_if(node: &ruby_prism::IfNode, _source: &str) -> bool {
    // Modifier form if: no end_keyword_loc and has if_keyword_loc (not ternary)
    node.if_keyword_loc().is_some() && node.end_keyword_loc().is_none()
}

fn is_modifier_form_unless(node: &ruby_prism::UnlessNode, _source: &str) -> bool {
    node.end_keyword_loc().is_none()
}

fn if_keyword_text_if<'a>(node: &ruby_prism::IfNode, source: &'a str) -> &'a str {
    if let Some(kw_loc) = node.if_keyword_loc() {
        &source[kw_loc.start_offset()..kw_loc.end_offset()]
    } else {
        "if"
    }
}

/// Check if condition has an assignment whose variable is used in the inner branch's condition
fn use_variable_assignment_in_condition_if(
    node: &ruby_prism::IfNode,
    inner: &InnerConditional,
    source: &str,
) -> bool {
    let condition = node.predicate();
    let assigned = collect_assigned_variables(&condition, source);
    if assigned.is_empty() {
        return false;
    }

    // The inner conditional must be an if_type (not unless) for this check
    // And its condition source must match one of the assigned variables
    if let Some(stmts) = node.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() == 1 {
            if let Node::IfNode { .. } = &body[0] {
                let inner_if = body[0].as_if_node().unwrap();
                let inner_cond = inner_if.predicate();
                let inner_cond_src =
                    &source[inner_cond.location().start_offset()..inner_cond.location().end_offset()];
                if assigned.contains(&inner_cond_src.to_string()) {
                    return true;
                }
            }
        }
    }
    false
}

fn use_variable_assignment_in_condition_unless(
    node: &ruby_prism::UnlessNode,
    inner: &InnerConditional,
    source: &str,
) -> bool {
    let condition = node.predicate();
    let assigned = collect_assigned_variables(&condition, source);
    if assigned.is_empty() {
        return false;
    }

    if let Some(stmts) = node.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() == 1 {
            if let Node::IfNode { .. } = &body[0] {
                let inner_if = body[0].as_if_node().unwrap();
                let inner_cond = inner_if.predicate();
                let inner_cond_src =
                    &source[inner_cond.location().start_offset()..inner_cond.location().end_offset()];
                if assigned.contains(&inner_cond_src.to_string()) {
                    return true;
                }
            }
        }
    }
    let _ = inner;
    false
}

/// Collect variable names assigned in a condition (e.g., `foo = bar` assigns "foo")
fn collect_assigned_variables(node: &Node, source: &str) -> Vec<String> {
    let mut result = Vec::new();
    collect_assigned_variables_inner(node, source, &mut result);
    result
}

fn collect_assigned_variables_inner(node: &Node, source: &str, result: &mut Vec<String>) {
    match node {
        Node::LocalVariableWriteNode { .. } => {
            let write = node.as_local_variable_write_node().unwrap();
            let name = String::from_utf8_lossy(write.name().as_slice()).to_string();
            result.push(name);
        }
        Node::LocalVariableOperatorWriteNode { .. }
        | Node::LocalVariableAndWriteNode { .. }
        | Node::LocalVariableOrWriteNode { .. } => {
            // Extract variable name from the node source - first token
            let loc = node.location();
            let src = &source[loc.start_offset()..loc.end_offset()];
            if let Some(name) = src.split(|c: char| !c.is_alphanumeric() && c != '_').next() {
                if !name.is_empty() {
                    result.push(name.to_string());
                }
            }
        }
        Node::AndNode { .. } => {
            let and_node = node.as_and_node().unwrap();
            collect_assigned_variables_inner(&and_node.left(), source, result);
            collect_assigned_variables_inner(&and_node.right(), source, result);
        }
        Node::OrNode { .. } => {
            let or_node = node.as_or_node().unwrap();
            collect_assigned_variables_inner(&or_node.left(), source, result);
            collect_assigned_variables_inner(&or_node.right(), source, result);
        }
        Node::ParenthesesNode { .. } => {
            let paren = node.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                collect_assigned_variables_inner(&body, source, result);
            }
        }
        _ => {}
    }
}

impl Visit<'_> for SoleNestedConditionalVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if_node(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless_node(node);
        ruby_prism::visit_unless_node(self, node);
    }
}

crate::register_cop!("Style/SoleNestedConditional", |cfg| {
    let cop_config = cfg.get_cop_config("Style/SoleNestedConditional");
    let allow_modifier = cop_config
        .and_then(|c| c.raw.get("AllowModifier"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(Box::new(SoleNestedConditional::with_config(allow_modifier)))
});
