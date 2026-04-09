//! Lint/Debugger cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const DEBUGGER_METHODS: &[&str] = &[
    "binding.irb", "Kernel.binding.irb",
    "byebug", "remote_byebug", "Kernel.byebug", "Kernel.remote_byebug",
    "page.save_and_open_page", "page.save_and_open_screenshot", "page.save_page", "page.save_screenshot",
    "save_and_open_page", "save_and_open_screenshot", "save_page", "save_screenshot",
    "binding.b", "binding.break", "Kernel.binding.b", "Kernel.binding.break",
    "binding.pry", "binding.remote_pry", "binding.pry_remote",
    "Kernel.binding.pry", "Kernel.binding.remote_pry", "Kernel.binding.pry_remote",
    "Pry.rescue", "pry",
    "debugger", "Kernel.debugger",
    "jard",
    "binding.console",
];

const DEBUGGER_REQUIRES: &[&str] = &["debug/open", "debug/start"];

pub struct Debugger {
    debugger_methods: Vec<String>,
    debugger_requires: Vec<String>,
}

impl Debugger {
    pub fn new() -> Self {
        Self::with_config(
            DEBUGGER_METHODS.iter().map(|s| s.to_string()).collect(),
            DEBUGGER_REQUIRES.iter().map(|s| s.to_string()).collect(),
        )
    }

    pub fn with_config(methods: Vec<String>, requires: Vec<String>) -> Self {
        Self {
            debugger_methods: methods,
            debugger_requires: requires,
        }
    }

    pub fn default_methods() -> Vec<String> {
        DEBUGGER_METHODS.iter().map(|s| s.to_string()).collect()
    }

    pub fn default_requires() -> Vec<String> {
        DEBUGGER_REQUIRES.iter().map(|s| s.to_string()).collect()
    }
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for Debugger {
    fn name(&self) -> &'static str {
        "Lint/Debugger"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = DebuggerVisitor {
            ctx,
            debugger: self,
            offenses: Vec::new(),
            call_ancestor_depth: 0,
            block_ancestor_depth: 0,
            parent_is_call: false,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct DebuggerVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    debugger: &'a Debugger,
    offenses: Vec<Offense>,
    call_ancestor_depth: usize,
    block_ancestor_depth: usize,
    parent_is_call: bool,
}

impl<'a> DebuggerVisitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let is_method = self.is_debugger_method(node);
        let is_require = self.is_debugger_require(node);
        if !is_method && !is_require { return; }
        if is_method && self.assumed_usage_context(node) { return; }

        let source = self.call_source_without_block(node);
        let (start, end) = self.call_range_without_block(node);
        self.offenses.push(self.ctx.offense_with_range(
            self.debugger.name(),
            &format!("Remove debugger entry point `{}`.", source),
            self.debugger.severity(),
            start,
            end,
        ));
    }

    fn assumed_usage_context(&self, node: &ruby_prism::CallNode) -> bool {
        if node.arguments().is_some() { return false; }
        if self.call_ancestor_depth == 0 { return false; }
        self.parent_is_call || self.block_ancestor_depth == 0
    }

    fn is_debugger_method(&self, node: &ruby_prism::CallNode) -> bool {
        let chained_name = chained_method_name(node, self.ctx);
        self.debugger.debugger_methods.iter().any(|m| m == &chained_name)
    }

    fn is_debugger_require(&self, node: &ruby_prism::CallNode) -> bool {
        if node_name!(node) != "require" { return false; }
        if node.receiver().is_some() { return false; }
        let args = match node.arguments() { Some(a) => a, None => return false };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 { return false; }
        let str_node = match arg_list[0].as_string_node() { Some(s) => s, None => return false };
        let loc = str_node.content_loc();
        self.ctx.source.as_bytes().get(loc.start_offset()..loc.end_offset())
            .map_or(false, |bytes| {
                let content = String::from_utf8_lossy(bytes);
                self.debugger.debugger_requires.iter().any(|r| r == content.as_ref())
            })
    }

    fn call_source_without_block(&self, node: &ruby_prism::CallNode) -> String {
        let (start, end) = self.call_range_without_block(node);
        self.ctx.source.get(start..end).unwrap_or("").to_string()
    }

    fn call_range_without_block(&self, node: &ruby_prism::CallNode) -> (usize, usize) {
        let start = node.location().start_offset();
        if let Some(closing) = node.closing_loc() { return (start, closing.end_offset()); }
        if let Some(args) = node.arguments() {
            if let Some(last_arg) = args.arguments().iter().last() {
                return (start, last_arg.location().end_offset());
            }
        }
        if let Some(msg_loc) = node.message_loc() { return (start, msg_loc.end_offset()); }
        (start, node.location().end_offset())
    }
}

impl Visit<'_> for DebuggerVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);

        let prev_parent_is_call = self.parent_is_call;
        let prev_call_depth = self.call_ancestor_depth;
        self.parent_is_call = true;
        self.call_ancestor_depth += 1;

        ruby_prism::visit_call_node(self, node);

        self.parent_is_call = prev_parent_is_call;
        self.call_ancestor_depth = prev_call_depth;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let prev_parent = self.parent_is_call;
        self.parent_is_call = false;
        self.block_ancestor_depth += 1;

        ruby_prism::visit_block_node(self, node);

        self.parent_is_call = prev_parent;
        self.block_ancestor_depth -= 1;
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        let prev_parent = self.parent_is_call;
        self.parent_is_call = false;
        self.block_ancestor_depth += 1;

        ruby_prism::visit_lambda_node(self, node);

        self.parent_is_call = prev_parent;
        self.block_ancestor_depth -= 1;
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        let prev_parent = self.parent_is_call;
        self.parent_is_call = false;
        self.block_ancestor_depth += 1;

        ruby_prism::visit_begin_node(self, node);

        self.parent_is_call = prev_parent;
        self.block_ancestor_depth -= 1;
    }
}

fn chained_method_name(node: &ruby_prism::CallNode, ctx: &CheckContext) -> String {
    let method_name = node_name!(node).to_string();

    let mut parts = vec![method_name];
    let mut current_receiver = node.receiver();

    loop {
        match current_receiver {
            Some(ref recv) => match recv {
                ruby_prism::Node::CallNode { .. } => {
                    let call_node = recv.as_call_node().unwrap();
                    parts.push(node_name!(call_node).to_string());
                    current_receiver = call_node.receiver();
                }
                ruby_prism::Node::ConstantReadNode { .. } => {
                    parts.push(node_name!(recv.as_constant_read_node().unwrap()).to_string());
                    break;
                }
                ruby_prism::Node::ConstantPathNode { .. } => { parts.push(full_const_path(recv, ctx)); break; }
                _ => break,
            },
            None => break,
        }
    }

    parts.reverse();
    parts.join(".")
}

fn full_const_path(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    match node {
        ruby_prism::Node::ConstantReadNode { .. } => {
            node_name!(node.as_constant_read_node().unwrap()).to_string()
        }
        ruby_prism::Node::ConstantPathNode { .. } => {
            let path_node = node.as_constant_path_node().unwrap();
            let name = path_node.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string()).unwrap_or_default();
            match path_node.parent() {
                Some(parent) => format!("{}::{}", full_const_path(&parent, ctx), name),
                None => name,
            }
        }
        _ => ctx.source.get(node.location().start_offset()..node.location().end_offset()).unwrap_or("").to_string()
    }
}
