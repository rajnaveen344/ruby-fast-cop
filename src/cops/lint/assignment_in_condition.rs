//! Lint/AssignmentInCondition cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

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
        self.check_condition(&predicate, ctx, &mut offenses, false);
        offenses
    }

    fn check_while(&self, node: &ruby_prism::WhileNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let predicate = node.predicate();
        self.check_condition(&predicate, ctx, &mut offenses, false);
        offenses
    }

    fn check_until(&self, node: &ruby_prism::UntilNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let predicate = node.predicate();
        self.check_condition(&predicate, ctx, &mut offenses, false);
        offenses
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let predicate = node.predicate();
        self.check_condition(&predicate, ctx, &mut offenses, false);
        offenses
    }
}

impl AssignmentInCondition {
    fn check_condition(
        &self,
        node: &ruby_prism::Node,
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
        inside_block: bool,
    ) {
        // Extract operator_loc for simple assignment nodes
        let operator_loc = match node {
            ruby_prism::Node::LocalVariableWriteNode { .. } =>
                Some(node.as_local_variable_write_node().unwrap().operator_loc()),
            ruby_prism::Node::InstanceVariableWriteNode { .. } =>
                Some(node.as_instance_variable_write_node().unwrap().operator_loc()),
            ruby_prism::Node::ClassVariableWriteNode { .. } =>
                Some(node.as_class_variable_write_node().unwrap().operator_loc()),
            ruby_prism::Node::GlobalVariableWriteNode { .. } =>
                Some(node.as_global_variable_write_node().unwrap().operator_loc()),
            ruby_prism::Node::ConstantWriteNode { .. } =>
                Some(node.as_constant_write_node().unwrap().operator_loc()),
            ruby_prism::Node::IndexOperatorWriteNode { .. } =>
                Some(node.as_index_operator_write_node().unwrap().binary_operator_loc()),
            ruby_prism::Node::CallOperatorWriteNode { .. } =>
                Some(node.as_call_operator_write_node().unwrap().binary_operator_loc()),
            _ => None,
        };

        if let Some(op_loc) = operator_loc {
            let is_safe = match node {
                ruby_prism::Node::LocalVariableWriteNode { .. } =>
                    self.is_safe_assignment_lvar(&node.as_local_variable_write_node().unwrap(), ctx),
                _ => self.is_safe_assignment_node(node, ctx),
            };
            if !is_safe {
                offenses.push(self.create_offense_at_operator_with_correction(
                    op_loc, node.location().start_offset(), node.location().end_offset(), ctx,
                ));
            }
            return;
        }

        match node {
            ruby_prism::Node::LocalVariableOrWriteNode { .. }
            | ruby_prism::Node::LocalVariableAndWriteNode { .. }
            | ruby_prism::Node::InstanceVariableOrWriteNode { .. }
            | ruby_prism::Node::InstanceVariableAndWriteNode { .. }
            | ruby_prism::Node::ClassVariableOrWriteNode { .. }
            | ruby_prism::Node::ClassVariableAndWriteNode { .. }
            | ruby_prism::Node::GlobalVariableOrWriteNode { .. }
            | ruby_prism::Node::GlobalVariableAndWriteNode { .. } => {}
            ruby_prism::Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    self.check_condition(&body, ctx, offenses, inside_block);
                }
            }
            // And/Or expressions - check both sides
            ruby_prism::Node::AndNode { .. } => {
                let and_node = node.as_and_node().unwrap();
                self.check_condition(&and_node.left(), ctx, offenses, inside_block);
                self.check_condition(&and_node.right(), ctx, offenses, inside_block);
            }
            ruby_prism::Node::OrNode { .. } => {
                let or_node = node.as_or_node().unwrap();
                self.check_condition(&or_node.left(), ctx, offenses, inside_block);
                self.check_condition(&or_node.right(), ctx, offenses, inside_block);
            }
            ruby_prism::Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method_name = String::from_utf8_lossy(call.name().as_slice());
                if method_name.ends_with('=') && method_name != "==" && method_name != "!=" {
                    if !self.is_safe_assignment_node(node, ctx) {
                        if let Some(eq_pos) = self.find_assignment_operator_position(&call, ctx) {
                            let mut offense = ctx.offense_with_range(
                                self.name(),
                                self.get_message(),
                                self.severity(),
                                eq_pos,
                                eq_pos + 1,
                            );
                            if self.allow_safe_assignment {
                                let correction = Correction {
                                    edits: vec![
                                        Edit {
                                            start_offset: node.location().start_offset(),
                                            end_offset: node.location().start_offset(),
                                            replacement: "(".to_string(),
                                        },
                                        Edit {
                                            start_offset: node.location().end_offset(),
                                            end_offset: node.location().end_offset(),
                                            replacement: ")".to_string(),
                                        },
                                    ],
                                };
                                offense = offense.with_correction(correction);
                            }
                            offenses.push(offense);
                        }
                    }
                }
            }
            ruby_prism::Node::BlockNode { .. } => {}
            ruby_prism::Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for stmt in stmts.body().iter() {
                    self.check_condition(&stmt, ctx, offenses, inside_block);
                }
            }
            _ => {}
        }
    }

    fn is_safe_assignment_lvar(&self, node: &ruby_prism::LocalVariableWriteNode, ctx: &CheckContext) -> bool {
        self.allow_safe_assignment && self.is_wrapped_in_parens(node.location().start_offset(), ctx)
    }

    fn is_safe_assignment_node(&self, node: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        self.allow_safe_assignment && self.is_wrapped_in_parens(node.location().start_offset(), ctx)
    }

    fn is_wrapped_in_parens(&self, start: usize, ctx: &CheckContext) -> bool {
        start > 0 && ctx.source[..start].trim_end().ends_with('(')
    }

    fn find_assignment_operator_position(
        &self,
        call: &ruby_prism::CallNode,
        ctx: &CheckContext,
    ) -> Option<usize> {
        let method_name = String::from_utf8_lossy(call.name().as_slice());
        if method_name == "[]=" {
            return call.closing_loc().and_then(|loc| self.find_equals_after(loc.end_offset(), ctx));
        }
        call.message_loc().and_then(|loc| self.find_equals_after(loc.end_offset(), ctx))
    }

    fn find_equals_after(&self, start: usize, ctx: &CheckContext) -> Option<usize> {
        let bytes = ctx.source.as_bytes();
        for i in start..bytes.len() {
            let c = bytes[i];
            if c == b'=' {
                // Make sure it's not == or !=
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    continue;
                }
                if i > 0 && bytes[i - 1] == b'!' {
                    continue;
                }
                return Some(i);
            }
            // Stop at end of line
            if c == b'\n' {
                break;
            }
        }
        None
    }

    fn get_message(&self) -> &'static str {
        if self.allow_safe_assignment {
            "Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition."
        } else {
            "Use `==` if you meant to do a comparison or move the assignment up out of the condition."
        }
    }

    fn create_offense_at_operator_with_correction(
        &self,
        operator_loc: ruby_prism::Location,
        node_start: usize,
        node_end: usize,
        ctx: &CheckContext,
    ) -> Offense {
        let mut offense = ctx.offense(self.name(), self.get_message(), self.severity(), &operator_loc);
        if self.allow_safe_assignment {
            let correction = Correction {
                edits: vec![
                    Edit {
                        start_offset: node_start,
                        end_offset: node_start,
                        replacement: "(".to_string(),
                    },
                    Edit {
                        start_offset: node_end,
                        end_offset: node_end,
                        replacement: ")".to_string(),
                    },
                ],
            };
            offense = offense.with_correction(correction);
        }
        offense
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

    #[test]
    fn detects_assignment_in_unless_modifier() {
        // Test unless modifier
        let source = "raise foo unless x = 1";
        let offenses = check(source);
        assert_eq!(offenses.len(), 1, "Should detect assignment in unless modifier");
    }

    #[test]
    fn detects_assignment_after_or_in_unless() {
        let offenses = check("raise StandardError unless (foo ||= bar) || a = b");
        assert_eq!(offenses.len(), 1, "Should detect 'a = b' in unless condition");
    }
}
