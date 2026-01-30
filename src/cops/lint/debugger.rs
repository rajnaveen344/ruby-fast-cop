//! Lint/Debugger - Checks for debugger calls that should not be kept for production code.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/debugger.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

/// Default debugger methods from RuboCop's default.yml
const DEBUGGER_METHODS: &[&str] = &[
    // Kernel
    "binding.irb",
    "Kernel.binding.irb",
    // Byebug
    "byebug",
    "remote_byebug",
    "Kernel.byebug",
    "Kernel.remote_byebug",
    // Capybara
    "page.save_and_open_page",
    "page.save_and_open_screenshot",
    "page.save_page",
    "page.save_screenshot",
    "save_and_open_page",
    "save_and_open_screenshot",
    "save_page",
    "save_screenshot",
    // debug.rb
    "binding.b",
    "binding.break",
    "Kernel.binding.b",
    "Kernel.binding.break",
    // Pry
    "binding.pry",
    "binding.remote_pry",
    "binding.pry_remote",
    "Kernel.binding.pry",
    "Kernel.binding.remote_pry",
    "Kernel.binding.pry_remote",
    "Pry.rescue",
    "pry",
    // Rails
    "debugger",
    "Kernel.debugger",
    // RubyJard
    "jard",
    // WebConsole
    "binding.console",
];

/// Default debugger requires from RuboCop's default.yml
const DEBUGGER_REQUIRES: &[&str] = &["debug/open", "debug/start"];

pub struct Debugger;

impl Debugger {
    pub fn new() -> Self {
        Self
    }

    /// Build the chained method name like "binding.pry" or "Kernel.binding.irb"
    fn chained_method_name(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> String {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        let mut parts = vec![method_name];
        let mut current_receiver = node.receiver();

        loop {
            match current_receiver {
                Some(ref recv) => match recv {
                    ruby_prism::Node::CallNode { .. } => {
                        let call_node = recv.as_call_node().unwrap();
                        let name =
                            String::from_utf8_lossy(call_node.name().as_slice()).to_string();
                        parts.push(name);
                        current_receiver = call_node.receiver();
                    }
                    ruby_prism::Node::ConstantReadNode { .. } => {
                        let loc = recv.location();
                        if let Some(bytes) = ctx
                            .source
                            .as_bytes()
                            .get(loc.start_offset()..loc.end_offset())
                        {
                            let name = String::from_utf8_lossy(bytes).to_string();
                            parts.push(name);
                        }
                        break;
                    }
                    _ => break,
                },
                None => break,
            }
        }

        parts.reverse();
        parts.join(".")
    }

    /// Check if this is a debugger method call
    fn is_debugger_method(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> bool {
        let chained_name = self.chained_method_name(node, ctx);
        DEBUGGER_METHODS.contains(&chained_name.as_str())
    }

    /// Check if this is a require statement for a debugger
    fn is_debugger_require(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> bool {
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        if method_name != "require" {
            return false;
        }

        // Check if receiver is nil (bare require call)
        if node.receiver().is_some() {
            return false;
        }

        // Get the first argument
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1 {
                if let ruby_prism::Node::StringNode { .. } = &arg_list[0] {
                    let str_node = arg_list[0].as_string_node().unwrap();
                    let loc = str_node.content_loc();
                    if let Some(bytes) = ctx
                        .source
                        .as_bytes()
                        .get(loc.start_offset()..loc.end_offset())
                    {
                        let content = String::from_utf8_lossy(bytes);
                        return DEBUGGER_REQUIRES.contains(&content.as_ref());
                    }
                }
            }
        }

        false
    }

    /// Get the source text for the node
    fn node_source(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> String {
        let loc = node.location();
        ctx.source
            .get(loc.start_offset()..loc.end_offset())
            .unwrap_or("")
            .to_string()
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

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if self.is_debugger_method(node, ctx) || self.is_debugger_require(node, ctx) {
            let source = self.node_source(node, ctx);
            let message = format!("Remove debugger entry point `{}`.", source);
            vec![ctx.offense(
                self.name(),
                &message,
                self.severity(),
                &node.location(),
            )]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_source;

    fn check(source: &str) -> Vec<Offense> {
        check_source(source, "test.rb")
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
