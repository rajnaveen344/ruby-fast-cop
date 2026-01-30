//! Style/MethodCalledOnDoEndBlock - Checks for methods called on do...end blocks.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/method_called_on_do_end_block.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

/// Checks for methods called on a do...end block. The point of this check is that
/// it's easy to miss the call tacked on to the block when reading code.
///
/// # Examples
///
/// ```ruby
/// # bad
/// a do
///   b
/// end.c
///
/// # good
/// a { b }.c
///
/// # good
/// foo = a do
///   b
/// end
/// foo.c
/// ```
pub struct MethodCalledOnDoEndBlock;

impl MethodCalledOnDoEndBlock {
    pub fn new() -> Self {
        Self
    }

    /// Check if a Node is a do...end block
    fn is_do_end_block_node(&self, node: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        if let ruby_prism::Node::BlockNode { .. } = node {
            let block = node.as_block_node().unwrap();
            // Check if it uses 'do' keyword by looking at the opening
            let open_loc = block.opening_loc();
            let start = open_loc.start_offset();
            let end = open_loc.end_offset();
            if let Some(text) = ctx.source.get(start..end) {
                return text == "do";
            }
        }
        false
    }
}

impl Default for MethodCalledOnDoEndBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for MethodCalledOnDoEndBlock {
    fn name(&self) -> &'static str {
        "Style/MethodCalledOnDoEndBlock"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Check if the receiver is a do...end block
        // The AST structure for `a do...end.c` has the CallNode with block as receiver
        if let Some(receiver) = node.receiver() {
            // Check for CallNode that has a block attached (the do...end part)
            if let ruby_prism::Node::CallNode { .. } = &receiver {
                let recv_call = receiver.as_call_node().unwrap();
                if let Some(ref block) = recv_call.block() {
                    if self.is_do_end_block_node(block, ctx) {
                        let method_name = String::from_utf8_lossy(node.name().as_slice());
                        return vec![ctx.offense(
                            self.name(),
                            &format!(
                                "Avoid chaining a method call on a do...end block. Method `{}` called on block.",
                                method_name
                            ),
                            self.severity(),
                            &node.location(),
                        )];
                    }
                }
            }
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check(source: &str) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(MethodCalledOnDoEndBlock::new());
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    #[test]
    fn detects_method_on_do_end_block() {
        let source = r#"
a do
  b
end.c
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("do...end"));
    }

    #[test]
    fn allows_method_on_brace_block() {
        let offenses = check("a { b }.c");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_do_end_block_without_chained_method() {
        let source = r#"
a do
  b
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_assigning_block_result_then_calling() {
        let source = r#"
foo = a do
  b
end
foo.c
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }
}
