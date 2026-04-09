//! Style/RedundantCondition - Checks for unnecessary conditional expressions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantCondition";
const MSG: &str = "Use double pipes `||` instead.";
const REDUNDANT_CONDITION: &str = "This condition is not needed.";

pub struct RedundantCondition {
    allowed_methods: Vec<String>,
}

impl RedundantCondition {
    pub fn new() -> Self {
        Self {
            allowed_methods: vec!["infinite?".to_string(), "nonzero?".to_string()],
        }
    }

    pub fn with_config(allowed_methods: Vec<String>) -> Self {
        Self { allowed_methods }
    }
}

impl Default for RedundantCondition {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for RedundantCondition {
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
        let mut visitor = RedundantConditionVisitor {
            ctx,
            allowed_methods: &self.allowed_methods,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct RedundantConditionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    allowed_methods: &'a [String],
    offenses: Vec<Offense>,
}

impl<'a> RedundantConditionVisitor<'a> {
    fn src(&self, node: &Node) -> &'a str {
        let loc = node.location();
        self.ctx.src(loc.start_offset(), loc.end_offset())
    }

    fn is_ternary(&self, node: &ruby_prism::IfNode) -> bool {
        // In Prism, ternary `b ? b : c` has if_keyword_loc pointing to `?`
        // Modifier if `bar if bar` has if_keyword_loc pointing to `if`
        // Regular if `if b; ... end` has if_keyword_loc pointing to `if`
        // Check: keyword is not "if" and not "elsif"
        if let Some(kw_loc) = node.if_keyword_loc() {
            let kw = self.ctx.src(kw_loc.start_offset(), kw_loc.end_offset());
            kw != "if" && kw != "elsif"
        } else {
            // No if_keyword_loc - this IS a ternary in Prism
            true
        }
    }

    fn is_modifier_if(&self, node: &ruby_prism::IfNode) -> bool {
        // Modifier if: has "if" keyword but no "end" keyword
        node.end_keyword_loc().is_none() && !self.is_ternary(node)
    }

    fn is_elsif(&self, node: &ruby_prism::IfNode) -> bool {
        let start = node.location().start_offset();
        self.ctx.source[start..].starts_with("elsif")
    }

    fn check_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Skip modifier form
        if self.is_modifier_if(node) {
            return;
        }
        // Skip elsif
        if self.is_elsif(node) {
            return;
        }
        if !self.offense_if(node) {
            return;
        }

        let is_ternary = self.is_ternary(node);

        let message = if !is_ternary && node.subsequent().is_none() {
            REDUNDANT_CONDITION
        } else {
            MSG
        };

        // Offense range
        let (start, end) = if is_ternary {
            if self.branches_have_method_if(node) {
                // Full ternary range
                (node.location().start_offset(), node.location().end_offset())
            } else {
                // Range from ? to :  (question mark to colon end)
                self.ternary_question_colon_range(node)
            }
        } else {
            (node.location().start_offset(), node.predicate().location().end_offset())
        };

        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, message, Severity::Convention, start, end,
        ));
    }

    fn ternary_question_colon_range(&self, node: &ruby_prism::IfNode) -> (usize, usize) {
        // Find ? and : in the ternary expression
        let cond_end = node.predicate().location().end_offset();
        let node_end = node.location().end_offset();
        let src = self.ctx.src(cond_end, node_end);

        // Find ? after condition
        let q_pos = src.find('?').map(|p| cond_end + p);
        // Find : after ?
        let c_pos = if let Some(qp) = q_pos {
            let after_q = self.ctx.src(qp + 1, node_end);
            // Find the colon - skip over the if_branch
            if let Some(stmts) = node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    let branch_end = last.location().end_offset();
                    let after_branch = self.ctx.src(branch_end, node_end);
                    after_branch.find(':').map(|p| branch_end + p + 1)
                } else {
                    after_q.find(':').map(|p| qp + 1 + p + 1)
                }
            } else {
                after_q.find(':').map(|p| qp + 1 + p + 1)
            }
        } else {
            None
        };

        match (q_pos, c_pos) {
            (Some(q), Some(c)) => (q, c),
            _ => (node.location().start_offset(), node.predicate().location().end_offset()),
        }
    }

    fn check_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if node.end_keyword_loc().is_none() {
            return;
        }
        if !self.offense_unless(node) {
            return;
        }
        let start = node.location().start_offset();
        let end = node.predicate().location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention, start, end,
        ));
    }

    fn offense_if(&self, node: &ruby_prism::IfNode) -> bool {
        let condition = node.predicate();
        let is_ternary = self.is_ternary(node);

        // Must have if_branch statements
        let if_stmts = match node.statements() {
            Some(s) => s,
            None => return false,
        };
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        if if_body.len() != 1 {
            return false;
        }
        let if_branch = &if_body[0];

        // Check subsequent
        if let Some(ref sub) = node.subsequent() {
            // elsif -> skip
            if matches!(sub, Node::IfNode { .. }) {
                return false;
            }
        }

        // Get else branch info
        let else_branch_single = self.get_else_single_node_info(node.subsequent());

        // Check for if_type / []= in else branch
        if let Some((ref _src, ref else_node_type, ref _else_src)) = else_branch_single {
            if *else_node_type == ElseNodeType::IfType {
                return false;
            }
            if *else_node_type == ElseNodeType::HashKeyAssign {
                return false;
            }
        }

        let cond_src = self.src(&condition);
        let if_src = self.src(if_branch);

        // Direct match: condition == if_branch
        if cond_src == if_src {
            // For non-ternary: else branch must be a single expression
            // (RuboCop: begin nodes with multiple statements are NOT flagged)
            if !is_ternary {
                if let Some(ref sub) = node.subsequent() {
                    if !self.else_has_single_expression(sub) {
                        return false;
                    }
                }
            }
            return true;
        }

        // if a.nil? then true else a end / a.nil? ? true : a
        if self.if_branch_is_true_type_and_else_is_not(&condition, if_branch, node.subsequent()) {
            return true;
        }

        // Branches have assignment
        if !is_ternary && self.branches_have_assignment_if(node) {
            return true;
        }

        // Branches have method (works for both ternary and non-ternary)
        if self.branches_have_method_if(node) {
            return true;
        }

        false
    }

    fn offense_unless(&self, node: &ruby_prism::UnlessNode) -> bool {
        let condition = node.predicate();
        let else_clause = match node.else_clause() {
            Some(c) => c,
            None => return false,
        };
        let else_stmts = match else_clause.statements() {
            Some(s) => s,
            None => return false,
        };
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        if else_body.len() != 1 {
            return false;
        }
        let cond_src = self.src(&condition);
        let else_src = self.src(&else_body[0]);
        if cond_src != else_src {
            return false;
        }
        let body_stmts = match node.statements() {
            Some(s) => s,
            None => return false,
        };
        let body: Vec<_> = body_stmts.body().iter().collect();
        if body.len() != 1 {
            return false;
        }
        if !self.is_single_line(&body[0]) {
            return false;
        }
        true
    }

    fn if_branch_is_true_type_and_else_is_not(
        &self,
        condition: &Node,
        if_branch: &Node,
        subsequent: Option<Node>,
    ) -> bool {
        let call = match condition.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let method_name = String::from_utf8_lossy(call.name().as_slice());
        if !method_name.ends_with('?') {
            return false;
        }
        if self.allowed_methods.iter().any(|m| m == method_name.as_ref()) {
            return false;
        }
        if !matches!(if_branch, Node::TrueNode { .. }) {
            return false;
        }
        let sub = match subsequent {
            Some(s) => s,
            None => return false,
        };
        let else_node = match sub.as_else_node() {
            Some(en) => en,
            None => return false,
        };
        let else_stmts = match else_node.statements() {
            Some(s) => s,
            None => return false,
        };
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        if else_body.len() != 1 {
            return false;
        }
        !matches!(&else_body[0], Node::TrueNode { .. })
    }

    fn branches_have_assignment_if(&self, node: &ruby_prism::IfNode) -> bool {
        let condition = node.predicate();
        let if_stmts = match node.statements() {
            Some(s) => s,
            None => return false,
        };
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        if if_body.len() != 1 { return false; }

        let sub = match node.subsequent() {
            Some(s) => s,
            None => return false,
        };
        let else_node = match sub.as_else_node() {
            Some(en) => en,
            None => return false,
        };
        let else_stmts = match else_node.statements() {
            Some(s) => s,
            None => return false,
        };
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        if else_body.len() != 1 { return false; }

        let if_name = self.assignment_name(&if_body[0]);
        let else_name = self.assignment_name(&else_body[0]);

        match (if_name, else_name) {
            (Some(in_), Some(en)) if in_ == en => {
                let if_val_src = self.assignment_value_src(&if_body[0]);
                let cond_src = self.src(&condition);
                if_val_src.map_or(false, |vs| vs == cond_src)
            }
            _ => false,
        }
    }

    fn branches_have_method_if(&self, node: &ruby_prism::IfNode) -> bool {
        let condition = node.predicate();
        let if_stmts = match node.statements() {
            Some(s) => s,
            None => return false,
        };
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        if if_body.len() != 1 { return false; }

        let sub = match node.subsequent() {
            Some(s) => s,
            None => return false,
        };
        let else_node = match sub.as_else_node() {
            Some(en) => en,
            None => return false,
        };
        let else_stmts = match else_node.statements() {
            Some(s) => s,
            None => return false,
        };
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        if else_body.len() != 1 { return false; }

        let if_branch = &if_body[0];
        let else_branch = &else_body[0];

        if !self.is_single_argument_method(if_branch) || !self.is_single_argument_method(else_branch) {
            return false;
        }
        if !self.same_method(if_branch, else_branch) {
            return false;
        }
        if self.is_hash_key_access(if_branch) {
            return false;
        }

        let if_call = if_branch.as_call_node().unwrap();
        if let Some(args) = if_call.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1 {
                if self.argument_with_operator(&arg_list[0]) {
                    return false;
                }
                let cond_src = self.src(&condition);
                let arg_src = self.src(&arg_list[0]);
                return cond_src == arg_src;
            }
        }
        false
    }

    fn is_single_argument_method(&self, node: &Node) -> bool {
        let call = match node.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let name = String::from_utf8_lossy(call.name().as_slice());
        if name == "[]" { return false; }
        if let Some(args) = call.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1 && !self.argument_with_operator(&arg_list[0]) {
                return true;
            }
        }
        false
    }

    fn argument_with_operator(&self, node: &Node) -> bool {
        matches!(
            node,
            Node::SplatNode { .. }
                | Node::BlockArgumentNode { .. }
                | Node::ForwardingArgumentsNode { .. }
        ) || {
            if let Some(kw_hash) = node.as_keyword_hash_node() {
                let elements: Vec<_> = kw_hash.elements().iter().collect();
                if let Some(first) = elements.first() {
                    matches!(first, Node::AssocSplatNode { .. } | Node::ForwardingArgumentsNode { .. })
                } else {
                    false
                }
            } else {
                false
            }
        }
    }

    fn same_method(&self, a: &Node, b: &Node) -> bool {
        let a_call = match a.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let b_call = match b.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let a_name = String::from_utf8_lossy(a_call.name().as_slice());
        let b_name = String::from_utf8_lossy(b_call.name().as_slice());
        if a_name != b_name { return false; }
        match (a_call.receiver(), b_call.receiver()) {
            (Some(ar), Some(br)) => self.src(&ar) == self.src(&br),
            (None, None) => true,
            _ => false,
        }
    }

    fn is_hash_key_access(&self, node: &Node) -> bool {
        if let Some(call) = node.as_call_node() {
            let name = String::from_utf8_lossy(call.name().as_slice());
            name == "[]"
        } else {
            false
        }
    }

    fn assignment_name(&self, node: &Node) -> Option<String> {
        match node {
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                Some(String::from_utf8_lossy(n.name().as_slice()).to_string())
            }
            Node::InstanceVariableWriteNode { .. } => {
                let n = node.as_instance_variable_write_node().unwrap();
                Some(String::from_utf8_lossy(n.name().as_slice()).to_string())
            }
            Node::ClassVariableWriteNode { .. } => {
                let n = node.as_class_variable_write_node().unwrap();
                Some(String::from_utf8_lossy(n.name().as_slice()).to_string())
            }
            Node::GlobalVariableWriteNode { .. } => {
                let n = node.as_global_variable_write_node().unwrap();
                Some(String::from_utf8_lossy(n.name().as_slice()).to_string())
            }
            Node::ConstantWriteNode { .. } => {
                let n = node.as_constant_write_node().unwrap();
                Some(String::from_utf8_lossy(n.name().as_slice()).to_string())
            }
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                let name = String::from_utf8_lossy(c.name().as_slice());
                if name.ends_with('=') && name != "==" && name != "!=" && name != "[]=" {
                    if let Some(recv) = c.receiver() {
                        let recv_src = self.src(&recv);
                        Some(format!("{}.{}", recv_src, name))
                    } else {
                        Some(name.to_string())
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn assignment_value_src(&self, node: &Node) -> Option<&'a str> {
        match node {
            Node::LocalVariableWriteNode { .. } => {
                Some(self.src(&node.as_local_variable_write_node().unwrap().value()))
            }
            Node::InstanceVariableWriteNode { .. } => {
                Some(self.src(&node.as_instance_variable_write_node().unwrap().value()))
            }
            Node::ClassVariableWriteNode { .. } => {
                Some(self.src(&node.as_class_variable_write_node().unwrap().value()))
            }
            Node::GlobalVariableWriteNode { .. } => {
                Some(self.src(&node.as_global_variable_write_node().unwrap().value()))
            }
            Node::ConstantWriteNode { .. } => {
                Some(self.src(&node.as_constant_write_node().unwrap().value()))
            }
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                let name = String::from_utf8_lossy(c.name().as_slice());
                if name.ends_with('=') && name != "==" && name != "!=" && name != "[]=" {
                    if let Some(args) = c.arguments() {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if arg_list.len() == 1 {
                            return Some(self.src(&arg_list[0]));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn is_single_line(&self, node: &Node) -> bool {
        let loc = node.location();
        self.ctx.same_line(loc.start_offset(), loc.end_offset())
    }

    fn else_has_single_expression(&self, sub: &Node) -> bool {
        let else_node = match sub.as_else_node() {
            Some(en) => en,
            None => return false,
        };
        let stmts = match else_node.statements() {
            Some(s) => s,
            None => return false,
        };
        let body: Vec<_> = stmts.body().iter().collect();
        body.len() == 1
    }

    fn get_else_single_node_info(&self, subsequent: Option<Node>) -> Option<(String, ElseNodeType, String)> {
        let sub = subsequent?;
        let else_node = sub.as_else_node()?;
        let stmts = else_node.statements()?;
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() != 1 { return None; }

        let node_type = if matches!(&body[0], Node::IfNode { .. }) {
            ElseNodeType::IfType
        } else if let Some(call) = body[0].as_call_node() {
            let name = String::from_utf8_lossy(call.name().as_slice());
            if name == "[]=" {
                ElseNodeType::HashKeyAssign
            } else {
                ElseNodeType::Normal
            }
        } else {
            ElseNodeType::Normal
        };

        Some((self.src(&body[0]).to_string(), node_type, self.src(&body[0]).to_string()))
    }
}

// Need to define ElseNodeType outside the impl block
#[derive(PartialEq)]
enum ElseNodeType {
    Normal,
    IfType,
    HashKeyAssign,
}

impl Visit<'_> for RedundantConditionVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if_node(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless_node(node);
        ruby_prism::visit_unless_node(self, node);
    }
}
