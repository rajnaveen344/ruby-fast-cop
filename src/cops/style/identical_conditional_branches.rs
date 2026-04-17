//! Style/IdenticalConditionalBranches - Detect identical code in all branches of conditional.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/identical_conditional_branches.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/IdenticalConditionalBranches";

#[derive(Default)]
pub struct IdenticalConditionalBranches;

impl IdenticalConditionalBranches {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for IdenticalConditionalBranches {
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
        let mut visitor = IdenticalBranchesVisitor {
            ctx,
            offenses: vec![],
            last_child_offsets: vec![],
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct IdenticalBranchesVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Whether current if/case is the last child of its parent scope
    /// Tracked by checking statements in def/class/module/program bodies
    last_child_offsets: Vec<usize>,
}

impl Visit<'_> for IdenticalBranchesVisitor<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        self.push_last_child_from_statements_node(&node.statements());
        ruby_prism::visit_program_node(self, node);
        self.last_child_offsets.pop();
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if let Some(body) = node.body() {
            self.push_last_child_from_node(&body);
        }
        ruby_prism::visit_def_node(self, node);
        if node.body().is_some() {
            self.last_child_offsets.pop();
        }
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(body) = node.body() {
            self.push_last_child_from_node(&body);
        }
        ruby_prism::visit_class_node(self, node);
        if node.body().is_some() {
            self.last_child_offsets.pop();
        }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if let Some(body) = node.body() {
            self.push_last_child_from_node(&body);
        }
        ruby_prism::visit_module_node(self, node);
        if node.body().is_some() {
            self.last_child_offsets.pop();
        }
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(stmts) = node.statements() {
            self.push_last_child_from_statements_node(&stmts);
        }
        ruby_prism::visit_begin_node(self, node);
        if node.statements().is_some() {
            self.last_child_offsets.pop();
        }
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        // The value of an assignment is always its "last child"
        self.last_child_offsets.push(node.value().location().start_offset());
        ruby_prism::visit_local_variable_write_node(self, node);
        self.last_child_offsets.pop();
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        self.last_child_offsets.push(node.value().location().start_offset());
        ruby_prism::visit_instance_variable_write_node(self, node);
        self.last_child_offsets.pop();
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if !is_elsif(node, self.ctx) {
            self.check_if(node);
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        self.check_case(node);
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        self.check_case_match(node);
        ruby_prism::visit_case_match_node(self, node);
    }
}

impl<'a> IdenticalBranchesVisitor<'a> {
    fn push_last_child_from_statements_node(&mut self, stmts: &ruby_prism::StatementsNode) {
        let items: Vec<_> = stmts.body().iter().collect();
        if let Some(last) = items.last() {
            self.last_child_offsets.push(last.location().start_offset());
        }
    }

    fn push_last_child_from_node(&mut self, node: &Node) {
        if let Some(stmts) = node.as_statements_node() {
            self.push_last_child_from_statements_node(&stmts);
        } else {
            // Single expression body
            self.last_child_offsets.push(node.location().start_offset());
        }
    }

    fn is_node_last_child(&self, node_start: usize, _node_end: usize) -> bool {
        // Check the nearest parent scope - is this node (or its containing statement)
        // the last child of that scope?
        if let Some(&last_start) = self.last_child_offsets.last() {
            // The conditional is the last child if its start >= last_child_start
            // (it's at or after the last statement's start)
            // OR if the last statement starts at/before the conditional
            // (handles `y = if ...` where lvasgn is the last child and contains the if)
            last_start <= node_start
        } else {
            false
        }
    }
}

impl<'a> IdenticalBranchesVisitor<'a> {
    fn check_if(&mut self, node: &ruby_prism::IfNode) {
        // Collect all branches by expanding elsif/else chains
        let mut branches: Vec<BranchBody> = Vec::new();

        // First branch (if branch)
        branches.push(self.extract_if_body(node));

        // Expand else/elsif chain
        let mut current = node.subsequent();
        loop {
            match current {
                None => {
                    // No else branch - don't check (need all branches covered)
                    return;
                }
                Some(ref subsequent) => {
                    if let Some(elsif_node) = subsequent.as_if_node() {
                        branches.push(self.extract_if_body(&elsif_node));
                        current = elsif_node.subsequent();
                    } else if let Some(else_node) = subsequent.as_else_node() {
                        branches.push(self.extract_else_body(&else_node));
                        break;
                    } else {
                        return;
                    }
                }
            }
        }

        self.check_branches(node.location().start_offset(), node.location().end_offset(),
                           &branches, Some(node));
    }

    fn check_case(&mut self, node: &ruby_prism::CaseNode) {
        // Need else clause
        let else_clause = match node.else_clause() {
            Some(e) => e,
            None => return,
        };

        let else_body = self.extract_else_body(&else_clause);
        if else_body.is_empty() {
            return;
        }

        let mut branches: Vec<BranchBody> = Vec::new();
        for when_node_raw in node.conditions().iter() {
            if let Some(when_node) = when_node_raw.as_when_node() {
                branches.push(self.extract_when_body(&when_node));
            }
        }
        branches.push(else_body);

        self.check_branches_case(node.location().start_offset(), node.location().end_offset(),
                                &branches, node);
    }

    fn check_case_match(&mut self, node: &ruby_prism::CaseMatchNode) {
        let else_clause = match node.else_clause() {
            Some(e) => e,
            None => return,
        };

        let else_body = self.extract_else_body(&else_clause);
        if else_body.is_empty() {
            return;
        }

        let mut branches: Vec<BranchBody> = Vec::new();
        for in_node_raw in node.conditions().iter() {
            if let Some(in_node) = in_node_raw.as_in_node() {
                branches.push(self.extract_in_body(&in_node));
            }
        }
        branches.push(else_body);

        self.check_branches_case_match(node.location().start_offset(), node.location().end_offset(),
                                      &branches, node);
    }

    fn check_branches(
        &mut self,
        _node_start: usize,
        _node_end: usize,
        branches: &[BranchBody],
        if_node: Option<&ruby_prism::IfNode>,
    ) {
        // Return if any branch is empty
        if branches.iter().any(|b| b.is_empty()) {
            return;
        }

        // Check tails (identical trailing expressions)
        let tails: Vec<&str> = branches.iter().map(|b| b.tail_source(self.ctx)).collect();
        if all_same(&tails) && !tails[0].is_empty() {
            // For tails, no condition-variable suppression (only applies to heads)
            for branch in branches {
                let (start, end) = branch.tail_range();
                let source = &self.ctx.source[start..end];
                let msg = format!("Move `{}` out of the conditional.", source);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    &msg,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }

        // Check heads (identical leading expressions)
        // Skip if last child of parent AND any branch is single-child
        if let Some(if_n) = if_node {
            if self.is_node_last_child(if_n.location().start_offset(), if_n.location().end_offset())
                && branches.iter().any(|b| b.is_single_child())
            {
                return;
            }
        }

        let heads: Vec<&str> = branches.iter().map(|b| b.head_source(self.ctx)).collect();
        if all_same(&heads) && !heads[0].is_empty() {
            // Check if assignment to condition variable
            if let Some(if_n) = if_node {
                if head_is_assignment_to_condition(branches[0].head_source(self.ctx), if_n, self.ctx) {
                    return;
                }
            }

            for branch in branches {
                let (start, end) = branch.head_range();
                let source = &self.ctx.source[start..end];
                let msg = format!("Move `{}` out of the conditional.", source);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    &msg,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }
    }

    fn check_branches_case(
        &mut self,
        _node_start: usize,
        _node_end: usize,
        branches: &[BranchBody],
        case_node: &ruby_prism::CaseNode,
    ) {
        if branches.iter().any(|b| b.is_empty()) {
            return;
        }

        // Check tails - no condition-variable suppression for tails
        let tails: Vec<&str> = branches.iter().map(|b| b.tail_source(self.ctx)).collect();
        if all_same(&tails) && !tails[0].is_empty() {
            for branch in branches {
                let (start, end) = branch.tail_range();
                let source = &self.ctx.source[start..end];
                let msg = format!("Move `{}` out of the conditional.", source);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME, &msg, Severity::Convention, start, end,
                ));
            }
        }

        // Check heads
        if self.is_node_last_child(case_node.location().start_offset(), case_node.location().end_offset())
            && branches.iter().any(|b| b.is_single_child())
        {
            return;
        }

        let heads: Vec<&str> = branches.iter().map(|b| b.head_source(self.ctx)).collect();
        if all_same(&heads) && !heads[0].is_empty() {
            if is_assignment_to_case_condition_var(branches[0].head_source(self.ctx), case_node, self.ctx) {
                return;
            }
            for branch in branches {
                let (start, end) = branch.head_range();
                let source = &self.ctx.source[start..end];
                let msg = format!("Move `{}` out of the conditional.", source);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME, &msg, Severity::Convention, start, end,
                ));
            }
        }
    }

    fn check_branches_case_match(
        &mut self,
        _node_start: usize,
        _node_end: usize,
        branches: &[BranchBody],
        case_node: &ruby_prism::CaseMatchNode,
    ) {
        if branches.iter().any(|b| b.is_empty()) {
            return;
        }

        // Check tails - no condition-variable suppression for tails
        let tails: Vec<&str> = branches.iter().map(|b| b.tail_source(self.ctx)).collect();
        if all_same(&tails) && !tails[0].is_empty() {
            for branch in branches {
                let (start, end) = branch.tail_range();
                let source = &self.ctx.source[start..end];
                let msg = format!("Move `{}` out of the conditional.", source);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME, &msg, Severity::Convention, start, end,
                ));
            }
        }

        // Check heads
        if self.is_node_last_child(case_node.location().start_offset(), case_node.location().end_offset())
            && branches.iter().any(|b| b.is_single_child())
        {
            return;
        }

        let heads: Vec<&str> = branches.iter().map(|b| b.head_source(self.ctx)).collect();
        if all_same(&heads) && !heads[0].is_empty() {
            if is_assignment_to_case_match_condition_var(branches[0].head_source(self.ctx), case_node, self.ctx) {
                return;
            }
            for branch in branches {
                let (start, end) = branch.head_range();
                let source = &self.ctx.source[start..end];
                let msg = format!("Move `{}` out of the conditional.", source);
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME, &msg, Severity::Convention, start, end,
                ));
            }
        }
    }

    fn extract_if_body(&self, node: &ruby_prism::IfNode) -> BranchBody {
        if let Some(stmts) = node.statements() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.is_empty() {
                return BranchBody::Empty;
            }
            let ranges: Vec<(usize, usize)> = items
                .iter()
                .map(|n| (n.location().start_offset(), n.location().end_offset()))
                .collect();
            let sources: Vec<String> = items
                .iter()
                .map(|n| self.ctx.source[n.location().start_offset()..n.location().end_offset()].to_string())
                .collect();
            BranchBody::Statements { ranges, sources }
        } else {
            BranchBody::Empty
        }
    }

    fn extract_else_body(&self, node: &ruby_prism::ElseNode) -> BranchBody {
        if let Some(stmts) = node.statements() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.is_empty() {
                return BranchBody::Empty;
            }
            let ranges: Vec<(usize, usize)> = items
                .iter()
                .map(|n| (n.location().start_offset(), n.location().end_offset()))
                .collect();
            let sources: Vec<String> = items
                .iter()
                .map(|n| self.ctx.source[n.location().start_offset()..n.location().end_offset()].to_string())
                .collect();
            BranchBody::Statements { ranges, sources }
        } else {
            BranchBody::Empty
        }
    }

    fn extract_when_body(&self, node: &ruby_prism::WhenNode) -> BranchBody {
        if let Some(stmts) = node.statements() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.is_empty() {
                return BranchBody::Empty;
            }
            let ranges: Vec<(usize, usize)> = items
                .iter()
                .map(|n| (n.location().start_offset(), n.location().end_offset()))
                .collect();
            let sources: Vec<String> = items
                .iter()
                .map(|n| self.ctx.source[n.location().start_offset()..n.location().end_offset()].to_string())
                .collect();
            BranchBody::Statements { ranges, sources }
        } else {
            BranchBody::Empty
        }
    }

    fn extract_in_body(&self, node: &ruby_prism::InNode) -> BranchBody {
        if let Some(stmts) = node.statements() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.is_empty() {
                return BranchBody::Empty;
            }
            let ranges: Vec<(usize, usize)> = items
                .iter()
                .map(|n| (n.location().start_offset(), n.location().end_offset()))
                .collect();
            let sources: Vec<String> = items
                .iter()
                .map(|n| self.ctx.source[n.location().start_offset()..n.location().end_offset()].to_string())
                .collect();
            BranchBody::Statements { ranges, sources }
        } else {
            BranchBody::Empty
        }
    }
}

/// Represents the body of a conditional branch
enum BranchBody {
    Empty,
    Statements {
        ranges: Vec<(usize, usize)>,
        sources: Vec<String>,
    },
}

impl BranchBody {
    fn is_empty(&self) -> bool {
        match self {
            BranchBody::Empty => true,
            BranchBody::Statements { sources, .. } => {
                // Treat branches with only empty parentheses "()" as empty
                sources.iter().all(|s| s.trim() == "()")
            }
        }
    }

    fn is_single_child(&self) -> bool {
        match self {
            BranchBody::Empty => true,
            BranchBody::Statements { ranges, .. } => ranges.len() == 1,
        }
    }

    fn tail_source<'a>(&self, _ctx: &'a CheckContext) -> &str {
        match self {
            BranchBody::Empty => "",
            BranchBody::Statements { sources, .. } => {
                sources.last().map(|s| s.as_str()).unwrap_or("")
            }
        }
    }

    fn tail_range(&self) -> (usize, usize) {
        match self {
            BranchBody::Empty => (0, 0),
            BranchBody::Statements { ranges, .. } => *ranges.last().unwrap(),
        }
    }

    fn head_source<'a>(&self, _ctx: &'a CheckContext) -> &str {
        match self {
            BranchBody::Empty => "",
            BranchBody::Statements { sources, .. } => {
                sources.first().map(|s| s.as_str()).unwrap_or("")
            }
        }
    }

    fn head_range(&self) -> (usize, usize) {
        match self {
            BranchBody::Empty => (0, 0),
            BranchBody::Statements { ranges, .. } => *ranges.first().unwrap(),
        }
    }
}

fn all_same(items: &[&str]) -> bool {
    if items.is_empty() {
        return false;
    }
    items.iter().all(|s| *s == items[0])
}

/// Check if an if node is actually an elsif
fn is_elsif(node: &ruby_prism::IfNode, ctx: &CheckContext) -> bool {
    if let Some(kw_loc) = node.if_keyword_loc() {
        let kw = &ctx.source[kw_loc.start_offset()..kw_loc.end_offset()];
        kw == "elsif"
    } else {
        false
    }
}

/// Check if the head expression is an assignment to the condition variable
fn head_is_assignment_to_condition(head_src: &str, if_node: &ruby_prism::IfNode, ctx: &CheckContext) -> bool {
    let assigned_var = extract_assignment_target(head_src);
    if let Some(var_name) = assigned_var {
        let cond_vars = extract_condition_variables(&if_node.predicate(), ctx);
        return cond_vars.iter().any(|cv| cv == &var_name);
    }
    false
}

fn is_assignment_to_case_condition_var(src: &str, case_node: &ruby_prism::CaseNode, ctx: &CheckContext) -> bool {
    let assigned_var = extract_assignment_target(src);
    if let Some(var_name) = assigned_var {
        if let Some(pred) = case_node.predicate() {
            let cond_src = source_of_node(&pred, ctx);
            if condition_uses_var(&cond_src, &var_name) {
                return true;
            }
        }
    }
    false
}

fn is_assignment_to_case_match_condition_var(src: &str, case_node: &ruby_prism::CaseMatchNode, ctx: &CheckContext) -> bool {
    let assigned_var = extract_assignment_target(src);
    if let Some(var_name) = assigned_var {
        if let Some(pred) = case_node.predicate() {
            let cond_src = source_of_node(&pred, ctx);
            if condition_uses_var(&cond_src, &var_name) {
                return true;
            }
        }
    }
    false
}

/// Extract the target variable name from an assignment expression
fn extract_assignment_target(src: &str) -> Option<String> {
    // Match patterns like "x = ...", "x += ...", "@x = ...", "h[:key] = ..."
    // Also match "self.foo ||= ..."
    let trimmed = src.trim();

    // Handle op-assignments: +=, -=, ||=, &&=, etc.
    for op in &[" ||= ", " &&= ", " += ", " -= ", " *= ", " /= ", " = "] {
        if let Some(idx) = trimmed.find(op) {
            let target = trimmed[..idx].trim();
            return Some(target.to_string());
        }
    }

    None
}

/// Check if the condition uses the given variable
fn condition_uses_var(cond_src: &str, var_name: &str) -> bool {
    // Simple check: does the condition source contain the variable name?
    // For "x.condition", receiver is "x", for "x", it's "x"
    // For "@x", check "@x"
    // Strip method calls from condition: "x.condition" -> "x", "x&.condition" -> "x"
    let base = if let Some(dot_idx) = cond_src.find('.') {
        &cond_src[..dot_idx]
    } else if let Some(amp_idx) = cond_src.find("&.") {
        &cond_src[..amp_idx]
    } else {
        cond_src
    };

    // Check if the assigned variable matches the condition base
    // For "h[:key] = foo" and condition "h[:key]", check the whole var_name
    if var_name == base || var_name == cond_src {
        return true;
    }

    // For index-style assignments like h[:key]
    if var_name.contains('[') {
        if cond_src.contains(var_name) || var_name.starts_with(base) {
            return true;
        }
    }

    // Check if var_name is a sub-part of the condition (e.g., "x" in "x == 0")
    // This handles the case like `if x == 0; x += 1`
    cond_src == var_name || cond_src.starts_with(&format!("{} ", var_name))
        || cond_src.starts_with(&format!("{}.", var_name))
        || cond_src.starts_with(&format!("{}&.", var_name))
}

fn source_of_node(node: &Node, ctx: &CheckContext) -> String {
    let loc = node.location();
    ctx.source[loc.start_offset()..loc.end_offset()].to_string()
}

fn extract_condition_variables(node: &Node, ctx: &CheckContext) -> Vec<String> {
    let src = source_of_node(node, ctx);
    let mut vars = vec![src.clone()]; // Always include the full condition source

    // For call nodes like "x.condition", also include the receiver
    if let Some(call) = node.as_call_node() {
        if let Some(recv) = call.receiver() {
            vars.push(source_of_node(&recv, ctx));
        }
    }

    vars
}


crate::register_cop!("Style/IdenticalConditionalBranches", |_cfg| {
    Some(Box::new(IdenticalConditionalBranches::new()))
});
