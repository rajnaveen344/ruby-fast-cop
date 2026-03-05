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
        if String::from_utf8_lossy(node.name().as_slice()) != "require" { return false; }
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
    let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

    let mut parts = vec![method_name];
    let mut current_receiver = node.receiver();

    loop {
        match current_receiver {
            Some(ref recv) => match recv {
                ruby_prism::Node::CallNode { .. } => {
                    let call_node = recv.as_call_node().unwrap();
                    parts.push(String::from_utf8_lossy(call_node.name().as_slice()).to_string());
                    current_receiver = call_node.receiver();
                }
                ruby_prism::Node::ConstantReadNode { .. } => {
                    parts.push(String::from_utf8_lossy(recv.as_constant_read_node().unwrap().name().as_slice()).to_string());
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
            String::from_utf8_lossy(node.as_constant_read_node().unwrap().name().as_slice()).to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_source_with_cops;

    fn check(source: &str) -> Vec<Offense> {
        let cops: Vec<Box<dyn crate::cops::Cop>> = vec![Box::new(Debugger::new())];
        check_source_with_cops(source, "test.rb", &cops)
    }

    fn offense_at(offenses: &[Offense], line: u32) -> Option<&Offense> {
        offenses.iter().find(|o| o.location.line == line)
    }

    // Kernel methods
    #[test]
    fn detects_binding_irb() {
        let offenses = check("binding.irb");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("binding.irb"));
    }

    // Byebug
    #[test]
    fn detects_byebug() {
        let offenses = check("byebug");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_remote_byebug() {
        let offenses = check("remote_byebug");
        assert_eq!(offenses.len(), 1);
    }

    // Pry
    #[test]
    fn detects_binding_pry() {
        let offenses = check("binding.pry");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_binding_remote_pry() {
        let offenses = check("binding.remote_pry");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_pry_rescue() {
        let offenses = check("Pry.rescue { }");
        assert_eq!(offenses.len(), 1);
    }

    // Rails
    #[test]
    fn detects_debugger() {
        let offenses = check("debugger");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].cop_name, "Lint/Debugger");
    }

    // RubyJard
    #[test]
    fn detects_jard() {
        let offenses = check("jard");
        assert_eq!(offenses.len(), 1);
    }

    // WebConsole
    #[test]
    fn detects_binding_console() {
        let offenses = check("binding.console");
        assert_eq!(offenses.len(), 1);
    }

    // debug.rb
    #[test]
    fn detects_binding_break() {
        let offenses = check("binding.break");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_binding_b() {
        let offenses = check("binding.b");
        assert_eq!(offenses.len(), 1);
    }

    // Kernel prefixed
    #[test]
    fn detects_kernel_debugger() {
        let offenses = check("Kernel.debugger");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_kernel_binding_pry() {
        let offenses = check("Kernel.binding.pry");
        assert_eq!(offenses.len(), 1);
    }

    // Debugger requires
    #[test]
    fn detects_require_debug_start() {
        let offenses = check("require 'debug/start'");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_require_debug_open() {
        let offenses = check("require 'debug/open'");
        assert_eq!(offenses.len(), 1);
    }

    // Multiple debuggers
    #[test]
    fn detects_multiple_debuggers() {
        let source = r#"
def foo
  debugger
  puts "hello"
  binding.pry
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 2);
        assert!(offense_at(&offenses, 3).is_some());
        assert!(offense_at(&offenses, 5).is_some());
    }

    // Should NOT match
    #[test]
    fn ignores_debugger_as_string() {
        let offenses = check(r#"puts "debugger""#);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn ignores_debugger_as_symbol() {
        let offenses = check(":debugger");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn ignores_debugger_in_comment() {
        let offenses = check("# debugger");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn ignores_unrelated_require() {
        let offenses = check("require 'json'");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn ignores_method_with_similar_name() {
        let offenses = check("my_debugger");
        assert_eq!(offenses.len(), 0);
    }
}
