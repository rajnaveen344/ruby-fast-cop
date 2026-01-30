//! Style/AutoResourceCleanup - Checks for resource cleanup without blocks.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/auto_resource_cleanup.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

/// Checks that resources are opened with a block to ensure automatic cleanup.
///
/// # Examples
///
/// ```ruby
/// # bad
/// f = File.open('file')
///
/// # good
/// File.open('file') do |f|
///   # ...
/// end
///
/// # also good - block form
/// File.open('file') { |f| ... }
/// ```
pub struct AutoResourceCleanup;

impl AutoResourceCleanup {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AutoResourceCleanup {
    fn default() -> Self {
        Self::new()
    }
}

struct ResourceVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop_name: &'static str,
    offenses: Vec<Offense>,
}

impl<'a> ResourceVisitor<'a> {
    fn new(ctx: &'a CheckContext<'a>, cop_name: &'static str) -> Self {
        Self {
            ctx,
            cop_name,
            offenses: Vec::new(),
        }
    }

    fn is_resource_open_call(&self, node: &ruby_prism::CallNode) -> Option<String> {
        // Check if the method is 'open'
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        if method_name != "open" {
            return None;
        }

        // Check if receiver is File or Tempfile
        if let Some(receiver) = node.receiver() {
            if let ruby_prism::Node::ConstantReadNode { .. } = &receiver {
                let const_node = receiver.as_constant_read_node().unwrap();
                let const_name = String::from_utf8_lossy(const_node.name().as_slice());
                if const_name == "File" || const_name == "Tempfile" {
                    return Some(format!("{}.open", const_name));
                }
            }
        }

        None
    }
}

impl Visit<'_> for ResourceVisitor<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        // Check if the value being assigned is a File.open or Tempfile.open call
        let value = node.value();
        if let ruby_prism::Node::CallNode { .. } = &value {
            let call = value.as_call_node().unwrap();

            // Check if this is a resource open call without a block
            if let Some(resource_name) = self.is_resource_open_call(&call) {
                // Check if the call has a block
                if call.block().is_none() {
                    self.offenses.push(self.ctx.offense(
                        self.cop_name,
                        &format!("Use the block version of `{}`.", resource_name),
                        Severity::Convention,
                        &node.location(),
                    ));
                }
            }
        }

        ruby_prism::visit_local_variable_write_node(self, node);
    }
}

impl Cop for AutoResourceCleanup {
    fn name(&self) -> &'static str {
        "Style/AutoResourceCleanup"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = ResourceVisitor::new(ctx, self.name());
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check(source: &str) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(AutoResourceCleanup::new());
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    #[test]
    fn flags_file_open_assignment() {
        let offenses = check("f = File.open('test.txt')");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("File.open"));
    }

    #[test]
    fn flags_tempfile_open_assignment() {
        let offenses = check("f = Tempfile.open('temp')");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("Tempfile.open"));
    }

    #[test]
    fn allows_file_open_with_block() {
        let source = r#"
File.open('test.txt') do |f|
  f.read
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_file_open_with_brace_block() {
        let offenses = check("File.open('test.txt') { |f| f.read }");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_file_read() {
        // File.read doesn't need a block
        let offenses = check("content = File.read('test.txt')");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_non_file_open() {
        let offenses = check("x = SomeClass.open('test')");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_file_open_without_assignment() {
        // Not assigned to variable - likely used directly
        let offenses = check("File.open('test.txt')");
        assert_eq!(offenses.len(), 0);
    }
}
