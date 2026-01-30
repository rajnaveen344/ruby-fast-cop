//! Lint/AssignmentInCondition - Checks for assignments in conditions of if/while/until.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/assignment_in_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

pub struct AssignmentInCondition {
    allow_safe_assignment: bool,
}

impl AssignmentInCondition {
    pub fn new(allow_safe_assignment: bool) -> Self {
        Self {
            allow_safe_assignment,
        }
    }
}

impl Default for AssignmentInCondition {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Cop for AssignmentInCondition {
    fn name(&self) -> &'static str {
        "Lint/AssignmentInCondition"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let predicate = node.predicate();
        self.check_condition(&predicate, ctx, &mut offenses);
        offenses
    }
}

impl AssignmentInCondition {
    fn check_condition(
        &self,
        node: &ruby_prism::Node,
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        match node {
            // Local variable assignment: x = 1
            ruby_prism::Node::LocalVariableWriteNode { .. } => {
                if !self.is_safe_assignment(node, ctx) {
                    offenses.push(self.create_offense(node, ctx));
                }
            }
            // Instance variable assignment: @x = 1
            ruby_prism::Node::InstanceVariableWriteNode { .. } => {
                if !self.is_safe_assignment(node, ctx) {
                    offenses.push(self.create_offense(node, ctx));
                }
            }
            // Class variable assignment: @@x = 1
            ruby_prism::Node::ClassVariableWriteNode { .. } => {
                if !self.is_safe_assignment(node, ctx) {
                    offenses.push(self.create_offense(node, ctx));
                }
            }
            // Global variable assignment: $x = 1
            ruby_prism::Node::GlobalVariableWriteNode { .. } => {
                if !self.is_safe_assignment(node, ctx) {
                    offenses.push(self.create_offense(node, ctx));
                }
            }
            // Constant assignment: X = 1
            ruby_prism::Node::ConstantWriteNode { .. } => {
                if !self.is_safe_assignment(node, ctx) {
                    offenses.push(self.create_offense(node, ctx));
                }
            }
            // Multi-assignment: a, b = 1, 2
            ruby_prism::Node::MultiWriteNode { .. } => {
                if !self.is_safe_assignment(node, ctx) {
                    offenses.push(self.create_offense(node, ctx));
                }
            }
            // Operator assignment: x += 1, x ||= 1
            ruby_prism::Node::LocalVariableOperatorWriteNode { .. }
            | ruby_prism::Node::LocalVariableOrWriteNode { .. }
            | ruby_prism::Node::LocalVariableAndWriteNode { .. } => {
                if !self.is_safe_assignment(node, ctx) {
                    offenses.push(self.create_offense(node, ctx));
                }
            }
            // Parenthesized expression - check inside
            ruby_prism::Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    // If allow_safe_assignment is true, parentheses make it safe
                    // If allow_safe_assignment is false, still check inside parens
                    if !self.allow_safe_assignment {
                        // When safe assignment is not allowed, check inside the parens
                        self.check_condition_ignoring_parens(&body, ctx, offenses);
                    }
                    // When safe assignment IS allowed, parens make it safe, so don't report
                }
            }
            // And/Or expressions - check both sides
            ruby_prism::Node::AndNode { .. } => {
                let and_node = node.as_and_node().unwrap();
                self.check_condition(&and_node.left(), ctx, offenses);
                self.check_condition(&and_node.right(), ctx, offenses);
            }
            ruby_prism::Node::OrNode { .. } => {
                let or_node = node.as_or_node().unwrap();
                self.check_condition(&or_node.left(), ctx, offenses);
                self.check_condition(&or_node.right(), ctx, offenses);
            }
            _ => {}
        }
    }

    /// Check condition but don't treat parentheses as making assignments safe
    fn check_condition_ignoring_parens(
        &self,
        node: &ruby_prism::Node,
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        match node {
            ruby_prism::Node::LocalVariableWriteNode { .. }
            | ruby_prism::Node::InstanceVariableWriteNode { .. }
            | ruby_prism::Node::ClassVariableWriteNode { .. }
            | ruby_prism::Node::GlobalVariableWriteNode { .. }
            | ruby_prism::Node::ConstantWriteNode { .. }
            | ruby_prism::Node::MultiWriteNode { .. }
            | ruby_prism::Node::LocalVariableOperatorWriteNode { .. }
            | ruby_prism::Node::LocalVariableOrWriteNode { .. }
            | ruby_prism::Node::LocalVariableAndWriteNode { .. } => {
                offenses.push(self.create_offense(node, ctx));
            }
            // Handle nested parentheses
            ruby_prism::Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    self.check_condition_ignoring_parens(&body, ctx, offenses);
                }
            }
            // Handle statements node (wrapper for body content)
            ruby_prism::Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for stmt in stmts.body().iter() {
                    self.check_condition_ignoring_parens(&stmt, ctx, offenses);
                }
            }
            _ => {}
        }
    }

    fn is_safe_assignment(&self, node: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        if !self.allow_safe_assignment {
            return false;
        }

        // Check if the assignment is wrapped in parentheses
        // This is a simplified check - we look at the source
        let loc = node.location();
        let start = loc.start_offset();

        // Check if there's a '(' before the assignment
        if start > 0 {
            if let Some(prev_char) = ctx.source.as_bytes().get(start.saturating_sub(1)) {
                if *prev_char == b'(' {
                    return true;
                }
            }
        }

        false
    }

    fn create_offense(&self, node: &ruby_prism::Node, ctx: &CheckContext) -> Offense {
        let message = if self.allow_safe_assignment {
            "Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition."
        } else {
            "Use `==` if you meant to do a comparison or move the assignment up out of the condition."
        };

        ctx.offense(self.name(), message, self.severity(), &node.location())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_config(source: &str, allow_safe: bool) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(AssignmentInCondition::new(allow_safe));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_config(source, true)
    }

    #[test]
    fn detects_assignment_in_if() {
        let offenses = check("if x = 1; end");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("=="));
    }

    #[test]
    fn allows_comparison_in_if() {
        let offenses = check("if x == 1; end");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_safe_assignment_when_enabled() {
        let offenses = check("if (x = 1); end");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_safe_assignment_when_disabled() {
        let offenses = check_with_config("if (x = 1); end", false);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_instance_var_assignment() {
        let offenses = check("if @x = 1; end");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_assignment_in_complex_condition() {
        let offenses = check("if foo && (x = bar); end");
        assert_eq!(offenses.len(), 0); // Wrapped in parens, so safe
    }

    #[test]
    fn allows_method_calls() {
        let offenses = check("if x.nil?; end");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_regular_conditions() {
        let offenses = check(
            r#"
if foo
  bar
end
"#,
        );
        assert_eq!(offenses.len(), 0);
    }
}
