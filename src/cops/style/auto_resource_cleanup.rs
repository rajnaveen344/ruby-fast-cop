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
#[derive(Default)]
pub struct AutoResourceCleanup;

impl AutoResourceCleanup {
    pub fn new() -> Self {
        Self
    }

    fn is_resource_open_call(node: &ruby_prism::CallNode) -> Option<String> {
        // Check if the method is 'open'
        let method_name = node_name!(node);
        if method_name != "open" {
            return None;
        }

        // Check if receiver is File or Tempfile
        if let Some(receiver) = node.receiver() {
            // Handle simple constant: File.open
            if let ruby_prism::Node::ConstantReadNode { .. } = &receiver {
                let const_node = receiver.as_constant_read_node().unwrap();
                let const_name = node_name!(const_node);
                if const_name == "File" || const_name == "Tempfile" {
                    return Some(format!("{}.open", const_name));
                }
            }
            // Handle constant path: ::File.open
            if let ruby_prism::Node::ConstantPathNode { .. } = &receiver {
                let path_node = receiver.as_constant_path_node().unwrap();
                // Check if it's a root constant (::File)
                if path_node.parent().is_none() {
                    if let Some(name) = path_node.name() {
                        let const_name = String::from_utf8_lossy(name.as_slice());
                        if const_name == "File" || const_name == "Tempfile" {
                            return Some(format!("::{}.open", const_name));
                        }
                    }
                }
            }
        }

        None
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

    fn check_open_call(&mut self, node: &ruby_prism::CallNode) {
        // Check if this is a resource open call without a block
        if let Some(resource_name) = AutoResourceCleanup::is_resource_open_call(node) {
            // Check if the call has a block
            if node.block().is_none() {
                self.offenses.push(self.ctx.offense(
                    self.cop_name,
                    &format!("Use the block version of `{}`.", resource_name),
                    Severity::Convention,
                    &node.location(),
                ));
            }
        }
    }
}

impl Visit<'_> for ResourceVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = node_name!(node);

        // If this is a .close call, check if the receiver is a File.open call
        // In that case, we don't flag it (e.g., File.open("f").close is okay)
        if method_name == "close" {
            if let Some(receiver) = node.receiver() {
                if let ruby_prism::Node::CallNode { .. } = &receiver {
                    let call = receiver.as_call_node().unwrap();
                    if AutoResourceCleanup::is_resource_open_call(&call).is_some() {
                        // Don't flag the File.open part - it's immediately closed
                        // But we still need to visit other nodes
                        return;
                    }
                }
            }
        }

        // Check if this is a resource open call that needs a block
        self.check_open_call(node);

        // Continue visiting child nodes
        ruby_prism::visit_call_node(self, node);
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

crate::register_cop!("Style/AutoResourceCleanup", |_cfg| {
    Some(Box::new(AutoResourceCleanup::new()))
});
