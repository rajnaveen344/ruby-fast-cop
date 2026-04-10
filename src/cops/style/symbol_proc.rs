//! Style/SymbolProc - Use symbols as procs when possible.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/symbol_proc.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/SymbolProc";

pub struct SymbolProc {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
    allow_methods_with_arguments: bool,
    allow_comments: bool,
    active_support_extensions_enabled: bool,
}

impl SymbolProc {
    pub fn new() -> Self {
        Self {
            allowed_methods: vec!["define_method".to_string()],
            allowed_patterns: vec![],
            allow_methods_with_arguments: false,
            allow_comments: false,
            active_support_extensions_enabled: false,
        }
    }

    pub fn with_config(
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
        allow_methods_with_arguments: bool,
        allow_comments: bool,
        active_support_extensions_enabled: bool,
    ) -> Self {
        Self {
            allowed_methods,
            allowed_patterns,
            allow_methods_with_arguments,
            allow_comments,
            active_support_extensions_enabled,
        }
    }
}

impl Default for SymbolProc {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for SymbolProc {
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
        let mut visitor = SymbolProcVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct SymbolProcVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a SymbolProc,
    offenses: Vec<Offense>,
}

impl<'a> SymbolProcVisitor<'a> {
    /// Extract the body method name if the block body is `param.method_name` pattern.
    /// Returns (param_name_or_pattern, body_method_name) if pattern matches.
    fn extract_body_call_info(
        &self,
        body: &Node,
    ) -> Option<(String, String)> {
        // Body must be a single call node with no args
        // Try as StatementsNode first, then as direct CallNode
        let body_call = if let Some(stmts) = body.as_statements_node() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 {
                return None;
            }
            items[0].as_call_node()?
        } else if let Some(call) = body.as_call_node() {
            call
        } else {
            return None;
        };

        // No arguments on body call
        if body_call.arguments().is_some() {
            return None;
        }
        // No block on body call
        if body_call.block().is_some() {
            return None;
        }

        let body_method = String::from_utf8_lossy(body_call.name().as_slice()).to_string();

        // Get receiver
        let recv = body_call.receiver()?;
        let recv_name = match &recv {
            Node::LocalVariableReadNode { .. } => {
                let lv = recv.as_local_variable_read_node().unwrap();
                String::from_utf8_lossy(lv.name().as_slice()).to_string()
            }
            Node::ItLocalVariableReadNode { .. } => "it".to_string(),
            _ => return None,
        };

        Some((recv_name, body_method))
    }

    /// Check if a block's parameters match the symbol_proc pattern.
    /// Returns the expected parameter name if valid.
    fn check_block_params(
        &self,
        params: &Option<Node>,
    ) -> Option<(String, bool)> {
        let p = params.as_ref()?;

        match p {
            Node::BlockParametersNode { .. } => {
                let bp = p.as_block_parameters_node().unwrap();
                let params_node = bp.parameters()?;

                let requireds: Vec<_> = params_node.requireds().iter().collect();
                if params_node.optionals().iter().count() > 0
                    || params_node.rest().is_some()
                    || params_node.keywords().iter().count() > 0
                    || params_node.block().is_some()
                {
                    return None;
                }

                if requireds.len() != 1 {
                    return None;
                }

                // Check destructuring
                let is_destr = if let (Some(o), Some(c)) = (bp.opening_loc(), bp.closing_loc()) {
                    let param_src = self.ctx.src(o.start_offset(), c.end_offset());
                    param_src.contains(',')
                } else {
                    false
                };

                match &requireds[0] {
                    Node::RequiredParameterNode { .. } => {
                        let rpn = requireds[0].as_required_parameter_node().unwrap();
                        let name = String::from_utf8_lossy(rpn.name().as_slice()).to_string();
                        Some((name, is_destr))
                    }
                    _ => None,
                }
            }
            Node::NumberedParametersNode { .. } => {
                let np = p.as_numbered_parameters_node().unwrap();
                if np.maximum() != 1 {
                    return None;
                }
                Some(("_1".to_string(), false))
            }
            Node::ItParametersNode { .. } => {
                Some(("it".to_string(), false))
            }
            // For lambda literals, parameters might be a plain ParametersNode
            Node::ParametersNode { .. } => {
                let pn = p.as_parameters_node().unwrap();
                let requireds: Vec<_> = pn.requireds().iter().collect();
                if pn.optionals().iter().count() > 0
                    || pn.rest().is_some()
                    || pn.keywords().iter().count() > 0
                    || pn.block().is_some()
                {
                    return None;
                }
                if requireds.len() != 1 {
                    return None;
                }
                match &requireds[0] {
                    Node::RequiredParameterNode { .. } => {
                        let rpn = requireds[0].as_required_parameter_node().unwrap();
                        let name = String::from_utf8_lossy(rpn.name().as_slice()).to_string();
                        Some((name, false))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn is_proc_node(&self, call: &ruby_prism::CallNode) -> bool {
        let method = String::from_utf8_lossy(call.name().as_slice());
        if method == "new" {
            if let Some(recv) = call.receiver() {
                if let Some(const_read) = recv.as_constant_read_node() {
                    let name = String::from_utf8_lossy(const_read.name().as_slice());
                    return name == "Proc";
                }
                if let Some(const_path) = recv.as_constant_path_node() {
                    if const_path.parent().is_none() {
                        let name_loc = const_path.name_loc();
                        let name = self.ctx.src(name_loc.start_offset(), name_loc.end_offset());
                        return name == "Proc";
                    }
                }
            }
        }
        false
    }

    fn unsafe_hash_usage(&self, call: &ruby_prism::CallNode) -> bool {
        if let Some(recv) = call.receiver() {
            if matches!(recv, Node::HashNode { .. }) {
                let method = String::from_utf8_lossy(call.name().as_slice());
                return method == "reject" || method == "select";
            }
        }
        false
    }

    fn unsafe_array_usage(&self, call: &ruby_prism::CallNode) -> bool {
        if let Some(recv) = call.receiver() {
            if matches!(recv, Node::ArrayNode { .. }) {
                let method = String::from_utf8_lossy(call.name().as_slice());
                return method == "min" || method == "max";
            }
        }
        false
    }

    fn block_has_comments(&self, start: usize, end: usize) -> bool {
        let text = self.ctx.src(start, end);
        // Simple check: look for # that's not inside a string
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                return true;
            }
            // Check for inline comment (simplistic)
            if let Some(pos) = line.find('#') {
                // Not inside a string if we're after the block start
                let before = &line[..pos];
                let single_count = before.chars().filter(|&c| c == '\'').count();
                let double_count = before.chars().filter(|&c| c == '"').count();
                if single_count % 2 == 0 && double_count % 2 == 0 {
                    return true;
                }
            }
        }
        false
    }

    fn check_call_with_block(&mut self, call: &ruby_prism::CallNode, block: &ruby_prism::BlockNode) {
        let body = match block.body() {
            Some(b) => b,
            None => return,
        };

        let (recv_name, body_method) = match self.extract_body_call_info(&body) {
            Some(info) => info,
            None => return,
        };

        let params = block.parameters();
        let (param_name, is_destructuring) = match self.check_block_params(&params) {
            Some(info) => info,
            None => return,
        };

        // Receiver must match parameter
        if recv_name != param_name {
            return;
        }

        let call_method = String::from_utf8_lossy(call.name().as_slice()).to_string();

        // ActiveSupport check
        if self.cop.active_support_extensions_enabled {
            if self.is_proc_node(call) {
                return;
            }
            if call_method == "lambda" || call_method == "proc" {
                return;
            }
        }

        // Unsafe hash/array usage
        if self.unsafe_hash_usage(call) || self.unsafe_array_usage(call) {
            return;
        }

        // AllowedMethods/AllowedPatterns
        if is_method_allowed(&self.cop.allowed_methods, &self.cop.allowed_patterns, &call_method, None) {
            return;
        }

        // AllowMethodsWithArguments
        if self.cop.allow_methods_with_arguments {
            if let Some(args) = call.arguments() {
                if args.arguments().iter().count() > 0 {
                    return;
                }
            }
        }

        // Destructuring check
        if is_destructuring {
            return;
        }

        // AllowComments
        let block_start = block.opening_loc().start_offset();
        let block_end = block.closing_loc().end_offset();
        if self.cop.allow_comments && self.block_has_comments(block_start, block_end) {
            return;
        }

        let message = format!(
            "Pass `&:{}` as an argument to `{}` instead of a block.",
            body_method, call_method
        );
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &message, Severity::Convention, block_start, block_end,
        ));
    }

    fn check_super_with_block(&mut self, super_node: &Node, block: &ruby_prism::BlockNode) {
        let body = match block.body() {
            Some(b) => b,
            None => return,
        };

        let (recv_name, body_method) = match self.extract_body_call_info(&body) {
            Some(info) => info,
            None => return,
        };

        let params = block.parameters();
        let (param_name, _is_destructuring) = match self.check_block_params(&params) {
            Some(info) => info,
            None => return,
        };

        if recv_name != param_name {
            return;
        }

        // AllowMethodsWithArguments for super
        if self.cop.allow_methods_with_arguments {
            if let Some(sn) = super_node.as_super_node() {
                if sn.arguments().is_some() {
                    return;
                }
            }
        }

        let block_start = block.opening_loc().start_offset();
        let block_end = block.closing_loc().end_offset();

        if self.cop.allow_comments && self.block_has_comments(block_start, block_end) {
            return;
        }

        let message = format!(
            "Pass `&:{}` as an argument to `super` instead of a block.",
            body_method
        );
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &message, Severity::Convention, block_start, block_end,
        ));
    }

    fn check_lambda_with_block(&mut self, call: &ruby_prism::CallNode, block: &ruby_prism::BlockNode) {
        let body = match block.body() {
            Some(b) => b,
            None => return,
        };

        let (recv_name, body_method) = match self.extract_body_call_info(&body) {
            Some(info) => info,
            None => return,
        };

        let params = block.parameters();
        let (param_name, _) = match self.check_block_params(&params) {
            Some(info) => info,
            None => return,
        };

        if recv_name != param_name {
            return;
        }

        // ActiveSupport check
        if self.cop.active_support_extensions_enabled {
            return;
        }

        let block_start = block.opening_loc().start_offset();
        let block_end = block.closing_loc().end_offset();

        if self.cop.allow_comments && self.block_has_comments(block_start, block_end) {
            return;
        }

        let call_method = String::from_utf8_lossy(call.name().as_slice());

        let message = format!(
            "Pass `&:{}` as an argument to `{}` instead of a block.",
            body_method, call_method
        );
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &message, Severity::Convention, block_start, block_end,
        ));
    }
}

impl Visit<'_> for SymbolProcVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(block_node) = node.block() {
            if let Some(block) = block_node.as_block_node() {
                let method = String::from_utf8_lossy(node.name().as_slice());
                if method == "lambda" && node.receiver().is_none() {
                    if let Some(msg_loc) = node.message_loc() {
                        let msg = self.ctx.src(msg_loc.start_offset(), msg_loc.end_offset());
                        if msg == "->" {
                            self.check_lambda_with_block(node, &block);
                            ruby_prism::visit_call_node(self, node);
                            return;
                        }
                    }
                }
                self.check_call_with_block(node, &block);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        // SuperNode.block() returns Option<Node>
        if let Some(block_node) = node.block() {
            if let Some(block) = block_node.as_block_node() {
                self.check_super_with_block(&node.as_node(), &block);
            }
        }
        ruby_prism::visit_super_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        // Lambda literal: ->(x) { x.method }
        let body = match node.body() {
            Some(b) => b,
            None => { ruby_prism::visit_lambda_node(self, node); return; },
        };

        let (recv_name, body_method) = match self.extract_body_call_info(&body) {
            Some(info) => info,
            None => { ruby_prism::visit_lambda_node(self, node); return; },
        };

        // Parameters
        let params = node.parameters();
        let (param_name, _) = match self.check_block_params(&params) {
            Some(info) => info,
            None => { ruby_prism::visit_lambda_node(self, node); return; },
        };

        if recv_name != param_name {
            ruby_prism::visit_lambda_node(self, node);
            return;
        }

        // ActiveSupport check
        if self.cop.active_support_extensions_enabled {
            ruby_prism::visit_lambda_node(self, node);
            return;
        }

        let block_start = node.opening_loc().start_offset();
        let block_end = node.closing_loc().end_offset();

        if self.cop.allow_comments && self.block_has_comments(block_start, block_end) {
            ruby_prism::visit_lambda_node(self, node);
            return;
        }

        let message = format!(
            "Pass `&:{}` as an argument to `lambda` instead of a block.",
            body_method
        );
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &message, Severity::Convention, block_start, block_end,
        ));
        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        // ForwardingSuperNode.block() returns Option<BlockNode> directly
        if let Some(ref block) = node.block() {
            self.check_super_with_block(&node.as_node(), block);
        }
        ruby_prism::visit_forwarding_super_node(self, node);
    }
}
