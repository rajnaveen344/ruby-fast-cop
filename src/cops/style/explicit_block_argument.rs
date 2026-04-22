//! Style/ExplicitBlockArgument cop
//!
//! Enforces the use of explicit block arguments instead of passing arguments
//! through intermediate blocks via yield.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Consider using explicit block argument in the surrounding method's signature over `yield`.";

#[derive(Default)]
pub struct ExplicitBlockArgument;

impl ExplicitBlockArgument {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ExplicitBlockArgument {
    fn name(&self) -> &'static str {
        "Style/ExplicitBlockArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ExplicitBlockArgumentVisitor {
            ctx,
            offenses: Vec::new(),
            in_method: false,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct ExplicitBlockArgumentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_method: bool,
}

impl<'a> ExplicitBlockArgumentVisitor<'a> {
    fn param_name(node: &Node) -> Option<Vec<u8>> {
        match node {
            Node::RequiredParameterNode { .. } => {
                Some(node.as_required_parameter_node().unwrap().name().as_slice().to_vec())
            }
            _ => None,
        }
    }

    fn arg_name(node: &Node) -> Option<Vec<u8>> {
        match node {
            Node::LocalVariableReadNode { .. } => {
                Some(node.as_local_variable_read_node().unwrap().name().as_slice().to_vec())
            }
            _ => None,
        }
    }

    /// Check if a BlockNode contains only `{ yield args... }` matching block params.
    /// Returns true if it's an offense-worthy block.
    fn block_is_pure_yield(block: &ruby_prism::BlockNode) -> bool {
        // Get block parameters (explicit named params only)
        let block_params: Vec<_> = if let Some(params) = block.parameters() {
            match params {
                Node::BlockParametersNode { .. } => {
                    let bp = params.as_block_parameters_node().unwrap();
                    if let Some(inner) = bp.parameters() {
                        inner.requireds().iter().collect()
                    } else {
                        vec![]
                    }
                }
                _ => return false, // NumberedParametersNode or ItParametersNode
            }
        } else {
            vec![]
        };

        // Block body must be a single yield
        let body = match block.body() {
            Some(b) => b,
            None => return false,
        };

        let yield_node_raw = if let Some(stmts) = body.as_statements_node() {
            let mut iter = stmts.body().iter();
            let first = match iter.next() { Some(n) => n, None => return false };
            if iter.next().is_some() { return false; }
            first
        } else {
            body
        };

        let yield_node = match yield_node_raw.as_yield_node() {
            Some(y) => y,
            None => return false,
        };

        let yield_args: Vec<_> = if let Some(args) = yield_node.arguments() {
            args.arguments().iter().collect()
        } else {
            vec![]
        };

        // Case: empty block params, empty yield args → { yield }
        if block_params.is_empty() && yield_args.is_empty() {
            return true;
        }

        // Case: block params match yield args exactly
        if block_params.len() != yield_args.len() {
            return false;
        }

        block_params.iter().zip(yield_args.iter()).all(|(bp, ya)| {
            let bp_name = Self::param_name(bp);
            let ya_name = Self::arg_name(ya);
            bp_name.is_some() && ya_name.is_some() && bp_name == ya_name
        })
    }

    /// Get the block from a Node (Option<Node> form), convert to BlockNode
    fn get_block_from_opt(block_opt: Option<Node>) -> Option<ruby_prism::BlockNode> {
        block_opt?.as_block_node()
    }

    fn add_offense_at(&mut self, start: usize, end: usize) {
        self.offenses.push(self.ctx.offense_with_range(
            "Style/ExplicitBlockArgument",
            MSG,
            Severity::Convention,
            start,
            end,
        ));
    }

    /// Check CallNode that has a block — look for yielding block pattern
    fn check_call_with_block(&mut self, call: &ruby_prism::CallNode) {
        if !self.in_method {
            return;
        }

        let block = match Self::get_block_from_opt(call.block()) {
            Some(b) => b,
            None => return,
        };

        if Self::block_is_pure_yield(&block) {
            let start = call.location().start_offset();
            let end = call.location().end_offset();
            self.add_offense_at(start, end);
        }
    }

    fn check_super_with_block(&mut self, node_start: usize, node_end: usize, block_opt: Option<Node>) {
        if !self.in_method {
            return;
        }
        let block = match Self::get_block_from_opt(block_opt) {
            Some(b) => b,
            None => return,
        };
        if Self::block_is_pure_yield(&block) {
            self.add_offense_at(node_start, node_end);
        }
    }

    fn check_forwarding_super_block(&mut self, node_start: usize, node_end: usize, block_opt: Option<ruby_prism::BlockNode>) {
        if !self.in_method {
            return;
        }
        let block = match block_opt {
            Some(b) => b,
            None => return,
        };
        if Self::block_is_pure_yield(&block) {
            self.add_offense_at(node_start, node_end);
        }
    }
}

impl<'a> Visit<'_> for ExplicitBlockArgumentVisitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let was_in_method = self.in_method;
        self.in_method = true;
        ruby_prism::visit_def_node(self, node);
        self.in_method = was_in_method;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call_with_block(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_super_with_block(start, end, node.block());
        ruby_prism::visit_super_node(self, node);
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_forwarding_super_block(start, end, node.block());
        ruby_prism::visit_forwarding_super_node(self, node);
    }
}

crate::register_cop!("Style/ExplicitBlockArgument", |_cfg| {
    Some(Box::new(ExplicitBlockArgument::new()))
});
