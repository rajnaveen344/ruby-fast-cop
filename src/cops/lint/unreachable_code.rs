//! Lint/UnreachableCode cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const MSG: &str = "Unreachable code detected.";
const FLOW_METHODS: &[&str] = &["raise", "fail", "throw", "exit", "exit!", "abort"];

#[derive(Default)]
pub struct UnreachableCode;

impl UnreachableCode {
    pub fn new() -> Self { Self }
}

impl Cop for UnreachableCode {
    fn name(&self) -> &'static str { "Lint/UnreachableCode" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = UnreachableCodeVisitor { ctx, offenses: Vec::new(), redefined: HashSet::new(), instance_eval_depth: 0 };
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
    fn stmts_have_flow(&mut self, stmts: &ruby_prism::StatementsNode) -> bool {
        stmts.body().iter().any(|expr| self.is_flow_expression(&expr))
    }

    fn is_flow_expression(&mut self, node: &Node) -> bool {
        match node {
            Node::ReturnNode { .. } | Node::NextNode { .. } | Node::BreakNode { .. }
            | Node::RetryNode { .. } | Node::RedoNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method_name = node_name!(call);
                if !FLOW_METHODS.contains(&method_name.as_ref()) { return false; }
                if let Some(receiver) = call.receiver() { return self.is_kernel_receiver(&receiver); }
                if self.instance_eval_depth > 0 { return false; }
                !self.redefined.contains(method_name.as_ref())
            }
            Node::BeginNode { .. } => node.as_begin_node().unwrap().statements()
                .map_or(false, |stmts| self.stmts_have_flow(&stmts)),
            Node::IfNode { .. } => self.check_if_flow(&node.as_if_node().unwrap()),
            Node::CaseNode { .. } => self.check_case_flow(&node.as_case_node().unwrap()),
            Node::CaseMatchNode { .. } => self.check_case_match_flow(&node.as_case_match_node().unwrap()),
            Node::DefNode { .. } => {
                let name = node_name!(node.as_def_node().unwrap());
                if FLOW_METHODS.contains(&name.as_ref()) { self.redefined.insert(name.into_owned()); }
                false
            }
            _ => false,
        }
    }

    fn is_kernel_receiver(&self, node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } =>
                node_name!(node.as_constant_read_node().unwrap()) == "Kernel",
            Node::ConstantPathNode { .. } => {
                let path = node.as_constant_path_node().unwrap();
                path.name().map_or(false, |n| String::from_utf8_lossy(n.as_slice()) == "Kernel")
                    && path.parent().is_none()
            }
            _ => false,
        }
    }

    fn check_if_flow(&mut self, node: &ruby_prism::IfNode) -> bool {
        let if_flow = node.statements().map_or(false, |s| self.stmts_have_flow(&s));
        if !if_flow { return false; }
        match node.subsequent() {
            Some(Node::ElseNode { .. }) => node.subsequent().unwrap().as_else_node().unwrap()
                .statements().map_or(false, |s| self.stmts_have_flow(&s)),
            Some(Node::IfNode { .. }) => self.check_if_flow(&node.subsequent().unwrap().as_if_node().unwrap()),
            _ => false,
        }
    }

    fn check_case_flow(&mut self, node: &ruby_prism::CaseNode) -> bool {
        let else_flow = node.else_clause().and_then(|e| e.statements())
            .map_or(false, |s| self.stmts_have_flow(&s));
        if !else_flow { return false; }
        node.conditions().iter().all(|cond|
            cond.as_when_node().and_then(|w| w.statements())
                .map_or(false, |s| self.stmts_have_flow(&s)))
    }

    fn check_case_match_flow(&mut self, node: &ruby_prism::CaseMatchNode) -> bool {
        let else_flow = node.else_clause().and_then(|e| e.statements())
            .map_or(false, |s| self.stmts_have_flow(&s));
        if !else_flow { return false; }
        node.conditions().iter().all(|cond|
            cond.as_in_node().and_then(|i| i.statements())
                .map_or(false, |s| self.stmts_have_flow(&s)))
    }

    fn check_statements(&mut self, node: &ruby_prism::StatementsNode) {
        let body: Vec<Node> = node.body().iter().collect();
        for pair in body.windows(2) {
            if self.is_flow_expression(&pair[0]) {
                self.offenses.push(self.ctx.offense("Lint/UnreachableCode", MSG, Severity::Warning, &pair[1].location()));
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
        if node_name!(node) == "instance_eval" {
            if let Some(Node::BlockNode { .. }) = node.block() {
                self.instance_eval_depth += 1;
                ruby_prism::visit_call_node(self, node);
                self.instance_eval_depth -= 1;
                return;
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/UnreachableCode", |_cfg| {
    Some(Box::new(UnreachableCode::new()))
});
