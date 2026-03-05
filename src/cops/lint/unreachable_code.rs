//! Lint/UnreachableCode - Checks for unreachable code.
//!
//! Detects code that appears after flow-control statements (return, break, next, retry, redo)
//! or flow-control method calls (raise, fail, throw, exit, exit!, abort) in statement sequences.
//! Also handles cases where all branches of if/unless/case/case_match terminate.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/unreachable_code.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const MSG: &str = "Unreachable code detected.";

/// Redefinable flow-control method names.
const FLOW_METHODS: &[&str] = &["raise", "fail", "throw", "exit", "exit!", "abort"];

pub struct UnreachableCode;

impl UnreachableCode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UnreachableCode {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for UnreachableCode {
    fn name(&self) -> &'static str {
        "Lint/UnreachableCode"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = UnreachableCodeVisitor {
            ctx,
            offenses: Vec::new(),
            redefined: HashSet::new(),
            instance_eval_depth: 0,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct UnreachableCodeVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    redefined: HashSet<String>,
    instance_eval_depth: usize,
}

impl<'a> UnreachableCodeVisitor<'a> {
    /// Check if a node is a flow-control expression that always terminates.
    /// Also registers method redefinitions as a side-effect (matching RuboCop's behavior
    /// where register_redefinition is called from within flow_expression?).
    fn is_flow_expression(&mut self, node: &Node) -> bool {
        match node {
            // Direct flow-control keywords
            Node::ReturnNode { .. }
            | Node::NextNode { .. }
            | Node::BreakNode { .. }
            | Node::RetryNode { .. }
            | Node::RedoNode { .. } => true,

            // Method calls: raise, fail, throw, exit, exit!, abort
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                self.is_flow_command(&call)
            }

            // begin/kwbegin: any expression in the sequence terminates
            Node::BeginNode { .. } => {
                let begin_node = node.as_begin_node().unwrap();
                if let Some(stmts) = begin_node.statements() {
                    stmts
                        .body()
                        .iter()
                        .any(|expr| self.is_flow_expression(&expr))
                } else {
                    false
                }
            }

            // if/elsif: both branches must terminate
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                self.check_if_flow(&if_node)
            }

            // case: all when branches + else must terminate
            Node::CaseNode { .. } => {
                let case_node = node.as_case_node().unwrap();
                self.check_case_flow(&case_node)
            }

            // case-in (pattern matching): all in branches + else must terminate
            Node::CaseMatchNode { .. } => {
                let case_match_node = node.as_case_match_node().unwrap();
                self.check_case_match_flow(&case_match_node)
            }

            // def/defs: register redefinition, not a flow expression
            Node::DefNode { .. } => {
                let def_node = node.as_def_node().unwrap();
                let method_name = String::from_utf8_lossy(def_node.name().as_slice());
                if FLOW_METHODS.contains(&method_name.as_ref()) {
                    self.redefined.insert(method_name.into_owned());
                }
                false
            }

            _ => false,
        }
    }

    /// Check if a CallNode is a flow command (raise/fail/throw/exit/exit!/abort).
    fn is_flow_command(&self, call: &ruby_prism::CallNode) -> bool {
        let method_name = String::from_utf8_lossy(call.name().as_slice());
        let method_str = method_name.as_ref();

        if !FLOW_METHODS.contains(&method_str) {
            return false;
        }

        self.report_on_flow_command(call, method_str)
    }

    /// Determine whether to report on a flow command, accounting for redefinitions
    /// and instance_eval context.
    fn report_on_flow_command(&self, call: &ruby_prism::CallNode, method_str: &str) -> bool {
        // If there's a receiver, check if it's Kernel
        if let Some(receiver) = call.receiver() {
            // Called on Kernel -> always report
            return self.is_kernel_receiver(&receiver);
        }

        // Inside instance_eval, we can't tell the type of self, so silence
        if self.instance_eval_depth > 0 {
            return false;
        }

        // If the method was redefined, don't report
        !self.redefined.contains(method_str)
    }

    /// Check if a receiver is `Kernel` (bare constant or `::Kernel`).
    fn is_kernel_receiver(&self, node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let const_node = node.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(const_node.name().as_slice());
                name.as_ref() == "Kernel"
            }
            Node::ConstantPathNode { .. } => {
                let path_node = node.as_constant_path_node().unwrap();
                let child_name = path_node
                    .name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()).to_string())
                    .unwrap_or_default();
                if child_name != "Kernel" {
                    return false;
                }
                path_node.parent().is_none()
            }
            _ => false,
        }
    }

    fn check_if_flow(&mut self, node: &ruby_prism::IfNode) -> bool {
        let if_branch_flow = match node.statements() {
            Some(stmts) => stmts
                .body()
                .iter()
                .any(|expr| self.is_flow_expression(&expr)),
            None => false,
        };

        if !if_branch_flow {
            return false;
        }

        match node.subsequent() {
            Some(else_clause) => self.is_flow_in_else_clause(&else_clause),
            None => false,
        }
    }

    fn is_flow_in_else_clause(&mut self, node: &Node) -> bool {
        match node {
            Node::ElseNode { .. } => {
                let else_node = node.as_else_node().unwrap();
                if let Some(stmts) = else_node.statements() {
                    stmts
                        .body()
                        .iter()
                        .any(|expr| self.is_flow_expression(&expr))
                } else {
                    false
                }
            }
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                self.check_if_flow(&if_node)
            }
            _ => false,
        }
    }

    fn check_case_flow(&mut self, node: &ruby_prism::CaseNode) -> bool {
        let else_flow = match node.else_clause() {
            Some(else_node) => {
                if let Some(stmts) = else_node.statements() {
                    stmts
                        .body()
                        .iter()
                        .any(|expr| self.is_flow_expression(&expr))
                } else {
                    false
                }
            }
            None => return false,
        };

        if !else_flow {
            return false;
        }

        node.conditions().iter().all(|cond| {
            if let Node::WhenNode { .. } = &cond {
                let when_node = cond.as_when_node().unwrap();
                if let Some(stmts) = when_node.statements() {
                    stmts
                        .body()
                        .iter()
                        .any(|expr| self.is_flow_expression(&expr))
                } else {
                    false
                }
            } else {
                false
            }
        })
    }

    fn check_case_match_flow(&mut self, node: &ruby_prism::CaseMatchNode) -> bool {
        let else_flow = match node.else_clause() {
            Some(else_node) => {
                if let Some(stmts) = else_node.statements() {
                    stmts
                        .body()
                        .iter()
                        .any(|expr| self.is_flow_expression(&expr))
                } else {
                    false
                }
            }
            None => return false,
        };

        if !else_flow {
            return false;
        }

        node.conditions().iter().all(|cond| {
            if let Node::InNode { .. } = &cond {
                let in_node = cond.as_in_node().unwrap();
                if let Some(stmts) = in_node.statements() {
                    stmts
                        .body()
                        .iter()
                        .any(|expr| self.is_flow_expression(&expr))
                } else {
                    false
                }
            } else {
                false
            }
        })
    }

    /// Check if a call node is `instance_eval`.
    fn is_instance_eval_call(&self, call_node: &ruby_prism::CallNode) -> bool {
        let name = String::from_utf8_lossy(call_node.name().as_slice());
        name.as_ref() == "instance_eval"
    }

    /// Process a statements body, checking consecutive expression pairs.
    /// This mirrors RuboCop's `on_begin` which checks `each_cons(2)`.
    fn check_statements(&mut self, node: &ruby_prism::StatementsNode) {
        let body: Vec<Node> = node.body().iter().collect();
        for pair in body.windows(2) {
            if self.is_flow_expression(&pair[0]) {
                let loc = pair[1].location();
                self.offenses.push(self.ctx.offense(
                    "Lint/UnreachableCode",
                    MSG,
                    Severity::Warning,
                    &loc,
                ));
            }
        }
    }
}

impl<'a> Visit<'_> for UnreachableCodeVisitor<'a> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode) {
        self.check_statements(node);
        ruby_prism::visit_statements_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let is_instance_eval = self.is_instance_eval_call(node);

        if is_instance_eval {
            if let Some(block) = node.block() {
                if let Node::BlockNode { .. } = block {
                    self.instance_eval_depth += 1;
                    ruby_prism::visit_call_node(self, node);
                    self.instance_eval_depth -= 1;
                    return;
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }
}
