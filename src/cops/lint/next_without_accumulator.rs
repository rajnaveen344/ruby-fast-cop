//! Lint/NextWithoutAccumulator - Don't omit accumulator when calling `next` in a `reduce` block.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node};

const MSG: &str = "Use `next` with an accumulator argument in a `reduce`.";

#[derive(Default)]
pub struct NextWithoutAccumulator;

impl NextWithoutAccumulator {
    pub fn new() -> Self { Self }
}

impl Cop for NextWithoutAccumulator {
    fn name(&self) -> &'static str { "Lint/NextWithoutAccumulator" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut state = TraversalState {
            ctx,
            offenses: Vec::new(),
            reduce_depth: 0,
            nested_block_depth: 0,
        };
        let stmts = node.statements();
        for child in stmts.body().iter() {
            state.visit(&child);
        }
        state.offenses
    }
}

struct TraversalState<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    reduce_depth: usize,
    nested_block_depth: usize,
}

impl<'a> TraversalState<'a> {
    fn visit(&mut self, node: &Node) {
        match node {
            Node::NextNode { .. } => {
                self.check_next(node.as_next_node().unwrap());
            }
            Node::CallNode { .. } => {
                self.visit_call(node.as_call_node().unwrap());
            }
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for child in stmts.body().iter() {
                    self.visit(&child);
                }
            }
            Node::BeginNode { .. } => {
                let begin = node.as_begin_node().unwrap();
                if let Some(stmts) = begin.statements() {
                    for child in stmts.body().iter() {
                        self.visit(&child);
                    }
                }
            }
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                self.visit(&if_node.predicate());
                if let Some(stmts) = if_node.statements() {
                    for child in stmts.body().iter() {
                        self.visit(&child);
                    }
                }
                if let Some(else_node) = if_node.subsequent() {
                    self.visit(&else_node);
                }
            }
            Node::ElseNode { .. } => {
                let else_node = node.as_else_node().unwrap();
                if let Some(stmts) = else_node.statements() {
                    for child in stmts.body().iter() {
                        self.visit(&child);
                    }
                }
            }
            Node::DefNode { .. } => {
                // Don't descend into nested defs — `next` there is a separate scope
            }
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                self.visit(&n.value());
            }
            Node::BlockNode { .. } => {
                let block = node.as_block_node().unwrap();
                self.visit_block_body(block.body());
            }
            _ => {} // Other nodes not needed
        }
    }

    fn visit_call(&mut self, call: ruby_prism::CallNode) {
        // Visit receiver
        if let Some(recv) = call.receiver() {
            self.visit(&recv);
        }
        // Visit arguments
        if let Some(args) = call.arguments() {
            for arg in args.arguments().iter() {
                self.visit(&arg);
            }
        }

        // Check if this is a reduce/inject call with a block
        if let Some(block) = call.block() {
            if let Some(block_node) = block.as_block_node() {
                let is_reduce = Self::is_reduce_call(&call);
                if is_reduce {
                    self.reduce_depth += 1;
                    let prev_nested = self.nested_block_depth;
                    self.nested_block_depth = 0;
                    self.visit_block_body(block_node.body());
                    self.nested_block_depth = prev_nested;
                    self.reduce_depth -= 1;
                } else if self.reduce_depth > 0 {
                    self.nested_block_depth += 1;
                    self.visit_block_body(block_node.body());
                    self.nested_block_depth -= 1;
                } else {
                    self.visit_block_body(block_node.body());
                }
            }
        }
    }

    fn visit_block_body(&mut self, body: Option<Node>) {
        if let Some(body) = body {
            self.visit(&body);
        }
    }

    fn is_reduce_call(call: &ruby_prism::CallNode) -> bool {
        let method = String::from_utf8_lossy(call.name().as_slice());
        let method_str = method.as_ref();
        if method_str != "reduce" && method_str != "inject" {
            return false;
        }
        // Skip if first arg is a symbol (e.g. reduce(:+))
        if let Some(args) = call.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let Some(first) = arg_list.first() {
                if matches!(first, Node::SymbolNode { .. }) {
                    return false;
                }
            }
        }
        true
    }

    fn check_next(&mut self, next_node: ruby_prism::NextNode) {
        if self.reduce_depth == 0 || self.nested_block_depth > 0 {
            return;
        }

        // Must have no arguments
        if let Some(args) = next_node.arguments() {
            if args.arguments().len() > 0 {
                return;
            }
        }

        let loc = next_node.location();
        self.offenses.push(self.ctx.offense_with_range(
            "Lint/NextWithoutAccumulator",
            MSG,
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        ));
    }
}

crate::register_cop!("Lint/NextWithoutAccumulator", |_cfg| Some(Box::new(NextWithoutAccumulator::new())));
