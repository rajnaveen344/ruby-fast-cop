//! Style/RedundantCondition - Checks for unnecessary conditional expressions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
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
        // Collect comment ranges for "no autocorrect when has comment" check
        let comment_ranges: Vec<(usize, usize)> = {
            let result = ruby_prism::parse(ctx.source.as_bytes());
            result
                .comments()
                .map(|c| {
                    let loc = c.location();
                    (loc.start_offset(), loc.end_offset())
                })
                .collect()
        };
        let mut visitor = RedundantConditionVisitor {
            ctx,
            allowed_methods: &self.allowed_methods,
            comment_ranges,
            offenses: Vec::new(),
            in_call_arg: false,
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct RedundantConditionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    allowed_methods: &'a [String],
    comment_ranges: Vec<(usize, usize)>,
    offenses: Vec<Offense>,
    /// Whether the current if-node is a direct argument inside a call (parent is send)
    in_call_arg: bool,
}

impl<'a> RedundantConditionVisitor<'a> {
    fn src(&self, node: &Node) -> &'a str {
        let loc = node.location();
        self.ctx.src(loc.start_offset(), loc.end_offset())
    }

    fn is_ternary(&self, node: &ruby_prism::IfNode) -> bool {
        if let Some(kw_loc) = node.if_keyword_loc() {
            let kw = self.ctx.src(kw_loc.start_offset(), kw_loc.end_offset());
            kw != "if" && kw != "elsif"
        } else {
            true
        }
    }

    fn is_modifier_if(&self, node: &ruby_prism::IfNode) -> bool {
        node.end_keyword_loc().is_none() && !self.is_ternary(node)
    }

    fn is_elsif(&self, node: &ruby_prism::IfNode) -> bool {
        let start = node.location().start_offset();
        self.ctx.source[start..].starts_with("elsif")
    }

    fn range_has_comment(&self, start: usize, end: usize) -> bool {
        self.comment_ranges.iter().any(|(s, _)| *s >= start && *s < end)
    }

    fn node_has_comment(&self, node: &Node) -> bool {
        let loc = node.location();
        self.range_has_comment(loc.start_offset(), loc.end_offset())
    }

    /// Any descendant of the if-node contains a comment → skip autocorrect
    fn if_node_has_descendant_comment(&self, node: &ruby_prism::IfNode) -> bool {
        let loc = node.location();
        self.range_has_comment(loc.start_offset(), loc.end_offset())
    }

    fn check_if_node(&mut self, node: &ruby_prism::IfNode, parent_is_call: bool) {
        if self.is_modifier_if(node) {
            return;
        }
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

        let (start, end) = if is_ternary {
            if self.branches_have_method_if(node) {
                (node.location().start_offset(), node.location().end_offset())
            } else {
                self.ternary_question_colon_range(node)
            }
        } else {
            (node.location().start_offset(), node.predicate().location().end_offset())
        };

        let has_comment = self.if_node_has_descendant_comment(node);

        let correction = if has_comment {
            None
        } else {
            Some(self.compute_if_correction(node, is_ternary, parent_is_call))
        };

        let offense = self.ctx.offense_with_range(
            COP_NAME, message, Severity::Convention, start, end,
        );
        let offense = if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        };
        self.offenses.push(offense);
    }

    fn compute_if_correction(
        &self,
        node: &ruby_prism::IfNode,
        is_ternary: bool,
        parent_is_call: bool,
    ) -> Correction {
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        // Case 1: ternary, branches_have_method → replace full node with merged call
        if is_ternary && self.branches_have_method_if(node) {
            let replacement = self.make_ternary_form_branches_have_method(node);
            return Correction::replace(node_start, node_end, replacement);
        }

        // Case 2: ternary simple → replace "? b :" with "||"
        if is_ternary {
            return self.correct_ternary(node);
        }

        // Case 3: no else branch (redundant_condition) → replace whole node with condition source
        if node.subsequent().is_none() {
            let cond_src = self.src(&node.predicate());
            return Correction::replace(node_start, node_end, cond_src.to_string());
        }

        // Case 4: branches_have_assignment (must come before branches_have_method
        // since assignment methods like `bar=` match both)
        if self.branches_have_assignment_if(node) {
            return self.correct_branches_have_assignment(node, parent_is_call);
        }

        // Case 5: branches_have_arithmetic_op
        if self.branches_have_arithmetic_op(node) {
            return self.correct_arithmetic_op(node, parent_is_call);
        }

        // Case 6: branches_have_method → special handling
        if self.branches_have_method_if(node) {
            return self.correct_branches_have_method(node, parent_is_call);
        }

        // Case 7: if_branch is `true` and cond is predicate with unparenthesized args
        // e.g. `if foo? arg; true; else; bar; end` → `foo?(arg) || bar`
        if self.is_true_branch_predicate_needs_parens(node) {
            if let Some(c) = self.compute_true_branch_correction(node, false, parent_is_call) {
                return c;
            }
        }

        // Case 8: simple cond==if_branch, else exists → "cond || else_src"
        self.correct_simple(node, parent_is_call)
    }

    fn correct_ternary(&self, node: &ruby_prism::IfNode) -> Correction {
        // Replace "? <if_branch> :" with "||"
        let (q_pos, c_pos) = self.ternary_question_colon_range(node);

        let mut edits = vec![
            Edit {
                start_offset: q_pos,
                end_offset: c_pos,
                replacement: "||".to_string(),
            }
        ];

        // If else branch is range type → wrap in parens
        if self.is_else_branch_range_node(node) {
            if let Some((else_start, else_end)) = self.get_else_branch_offsets(node) {
                let else_src = self.ctx.src(else_start, else_end);
                edits.push(Edit {
                    start_offset: else_start,
                    end_offset: else_end,
                    replacement: format!("({})", else_src),
                });
            }
        }

        Correction { edits }
    }

    fn correct_simple(&self, node: &ruby_prism::IfNode, parent_is_call: bool) -> Correction {
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let cond_src = self.src(&node.predicate());

        let else_src = self.get_else_src_for_simple(node);

        let ternary_form = format!("{} || {}", cond_src, else_src);
        let replacement = if parent_is_call {
            format!("({})", ternary_form)
        } else {
            ternary_form
        };

        Correction::replace(node_start, node_end, replacement)
    }

    fn get_else_src_for_simple(&self, node: &ruby_prism::IfNode) -> String {
        let sub = match node.subsequent() {
            Some(s) => s,
            None => return String::new(),
        };
        let else_node = match sub.as_else_node() {
            Some(en) => en,
            None => return String::new(),
        };
        let stmts = match else_node.statements() {
            Some(s) => s,
            None => return String::new(),
        };
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() != 1 { return String::new(); }
        self.else_source_simple(&body[0])
    }

    fn correct_branches_have_method(&self, node: &ruby_prism::IfNode, parent_is_call: bool) -> Correction {
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        let if_stmts = node.statements().unwrap();
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        let if_branch = &if_body[0];
        let if_call = if_branch.as_call_node().unwrap();

        let sub = node.subsequent().unwrap();
        let else_n = sub.as_else_node().unwrap();
        let else_stmts = else_n.statements().unwrap();
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        let else_branch = &else_body[0];

        let else_call = else_branch.as_call_node().unwrap();
        let else_args = else_call.arguments().unwrap();
        let else_arg_list: Vec<_> = else_args.arguments().iter().collect();
        let else_arg = &else_arg_list[0];

        let else_arg_src = self.else_source_if_has_method(else_arg);

        let if_branch_start = if_branch.location().start_offset();

        // Build replacement starting from if_branch (not from node_start/if keyword)
        let replacement = if let Some(open_loc) = if_call.opening_loc() {
            // parenthesized call: bar(foo) / X.find(x)
            let if_args = if_call.arguments().unwrap();
            let if_arg_list: Vec<_> = if_args.arguments().iter().collect();
            let if_arg = &if_arg_list[0];
            let if_arg_src = self.src(if_arg);
            let open_offset = open_loc.start_offset();
            // prefix = everything from if_branch start up to '('
            let prefix = self.ctx.src(if_branch_start, open_offset);
            format!("{}({} || {})", prefix, if_arg_src, else_arg_src)
        } else {
            // unparenthesized: bar foo / bar 1..2
            let if_args = if_call.arguments().unwrap();
            let if_arg_list: Vec<_> = if_args.arguments().iter().collect();
            let if_arg = &if_arg_list[0];
            let if_arg_src = self.src(if_arg);

            let method_end = if_call.message_loc()
                .map(|l| l.end_offset())
                .unwrap_or(if_call.location().start_offset());

            // prefix = from if_branch start to end of method name
            let prefix = self.ctx.src(if_branch_start, method_end);
            let inner = format!("{} || {}", if_arg_src, else_arg_src);
            format!("{} {}", prefix, inner)
        };

        let replacement = if parent_is_call {
            format!("({})", replacement)
        } else {
            replacement
        };

        Correction::replace(node_start, node_end, replacement)
    }

    fn else_source_if_has_method(&self, else_arg: &Node) -> String {
        if self.require_parentheses(else_arg) {
            format!("({})", self.src(else_arg))
        } else if self.require_braces(else_arg) {
            format!("{{ {} }}", self.src(else_arg))
        } else {
            self.src(else_arg).to_string()
        }
    }

    fn correct_branches_have_assignment(&self, node: &ruby_prism::IfNode, parent_is_call: bool) -> Correction {
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        let if_stmts = node.statements().unwrap();
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        let if_branch = &if_body[0];

        let sub = node.subsequent().unwrap();
        let else_n = sub.as_else_node().unwrap();
        let else_stmts = else_n.statements().unwrap();
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        let else_branch = &else_body[0];

        // if_branch_src = whole assignment like "@value = foo"
        let if_branch_src = self.src(if_branch);

        // Get the rhs of the else assignment and format it
        let else_val_src = self.else_assignment_value_src(else_branch);

        // Replacement = "<if_branch_src> || <else_val_src>"
        // e.g. "@value = foo || 'bar'"
        let replacement = format!("{} || {}", if_branch_src, else_val_src);
        let replacement = if parent_is_call {
            format!("({})", replacement)
        } else {
            replacement
        };

        Correction::replace(node_start, node_end, replacement)
    }

    fn else_assignment_value_src(&self, else_branch: &Node) -> String {
        // Extract rhs of assignment and apply require_parens/braces logic
        match else_branch {
            Node::LocalVariableWriteNode { .. } => {
                let v = else_branch.as_local_variable_write_node().unwrap().value();
                self.format_else_value(&v)
            }
            Node::InstanceVariableWriteNode { .. } => {
                let v = else_branch.as_instance_variable_write_node().unwrap().value();
                self.format_else_value(&v)
            }
            Node::ClassVariableWriteNode { .. } => {
                let v = else_branch.as_class_variable_write_node().unwrap().value();
                self.format_else_value(&v)
            }
            Node::GlobalVariableWriteNode { .. } => {
                let v = else_branch.as_global_variable_write_node().unwrap().value();
                self.format_else_value(&v)
            }
            Node::ConstantWriteNode { .. } => {
                let v = else_branch.as_constant_write_node().unwrap().value();
                self.format_else_value(&v)
            }
            Node::CallNode { .. } => {
                let c = else_branch.as_call_node().unwrap();
                let name = String::from_utf8_lossy(c.name().as_slice()).to_string();
                if name.ends_with('=') && name != "==" && name != "!=" && name != "[]=" {
                    if let Some(args) = c.arguments() {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if let Some(arg) = arg_list.first() {
                            return self.format_else_value(arg);
                        }
                    }
                }
                self.src(else_branch).to_string()
            }
            _ => self.src(else_branch).to_string(),
        }
    }

    fn format_else_value(&self, node: &Node) -> String {
        if self.require_parentheses(node) {
            format!("({})", self.src(node))
        } else if self.require_braces(node) {
            format!("{{ {} }}", self.src(node))
        } else {
            self.src(node).to_string()
        }
    }


    fn correct_arithmetic_op(&self, node: &ruby_prism::IfNode, parent_is_call: bool) -> Correction {
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        let if_stmts = node.statements().unwrap();
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        let if_branch = &if_body[0];
        let if_call = if_branch.as_call_node().unwrap();

        let sub = node.subsequent().unwrap();
        let else_node = sub.as_else_node().unwrap();
        let else_stmts = else_node.statements().unwrap();
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        let else_branch = &else_body[0];
        let else_call = else_branch.as_call_node().unwrap();

        let cond_src = self.src(&node.predicate());

        // Get receiver and operator from if_branch
        let receiver_src = if let Some(recv) = if_call.receiver() {
            self.src(&recv).to_string()
        } else {
            String::new()
        };
        let method_name = String::from_utf8_lossy(if_call.name().as_slice()).to_string();

        // Get if arg (the condition itself)
        let if_args = if_call.arguments().unwrap();
        let if_arg_list: Vec<_> = if_args.arguments().iter().collect();

        // Get else arg
        let else_args = else_call.arguments().unwrap();
        let else_arg_list: Vec<_> = else_args.arguments().iter().collect();
        let else_arg = &else_arg_list[0];
        let else_arg_src = self.src(else_arg);

        // RuboCop: arithmetic_op: `receiver op (if_arg || else_arg)`
        // e.g. @value - (foo || 'bar')
        // receiver_src is the if_branch receiver (e.g. "@value")
        let replacement = format!("{} {} ({} || {})", receiver_src, method_name, cond_src, else_arg_src);
        let replacement = if parent_is_call {
            format!("({})", replacement)
        } else {
            replacement
        };

        // Replace the whole if...end with just the merged expression
        Correction::replace(node_start, node_end, replacement)
    }

    fn make_ternary_form_branches_have_method(&self, node: &ruby_prism::IfNode) -> String {
        // For ternary: foo ? bar(foo) : bar(quux) → bar(foo || quux)
        let if_stmts = node.statements().unwrap();
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        let if_branch = &if_body[0];
        let if_call = if_branch.as_call_node().unwrap();

        let sub = node.subsequent().unwrap();
        let else_n = sub.as_else_node().unwrap();
        let else_stmts = else_n.statements().unwrap();
        let else_body: Vec<_> = else_stmts.body().iter().collect();
        let else_branch = &else_body[0];
        let else_call = else_branch.as_call_node().unwrap();

        let else_args = else_call.arguments().unwrap();
        let else_arg_list: Vec<_> = else_args.arguments().iter().collect();
        let else_arg = &else_arg_list[0];
        let else_arg_src = self.else_source_if_has_method(else_arg);

        let if_branch_start = if_branch.location().start_offset();

        if let Some(open_loc) = if_call.opening_loc() {
            let if_args = if_call.arguments().unwrap();
            let if_arg_list: Vec<_> = if_args.arguments().iter().collect();
            let if_arg = &if_arg_list[0];
            let if_arg_src = self.src(if_arg);
            let open_offset = open_loc.start_offset();
            // prefix = from if_branch start up to '(' (includes receiver.method portion)
            let prefix = self.ctx.src(if_branch_start, open_offset);
            format!("{}({} || {})", prefix, if_arg_src, else_arg_src)
        } else {
            // Shouldn't reach here for branches_have_method ternary (all have parens)
            self.src(if_branch).to_string()
        }
    }

    fn else_source_simple(&self, else_node: &Node) -> String {
        // without_argument_parentheses_method?
        if self.without_argument_parentheses_method(else_node) {
            let call = else_node.as_call_node().unwrap();
            let method_name = String::from_utf8_lossy(call.name().as_slice()).to_string();
            let args = call.arguments().unwrap();
            let arg_list: Vec<_> = args.arguments().iter().collect();
            let args_src: Vec<&str> = arg_list.iter().map(|a| self.src(a)).collect();
            return format!("{}({})", method_name, args_src.join(", "));
        }

        if self.require_parentheses(else_node) {
            return format!("({})", self.src(else_node));
        }

        self.src(else_node).to_string()
    }

    fn require_parentheses(&self, node: &Node) -> bool {
        // modifier if/unless, range, rescue_modifier, semantic and/or
        match node {
            Node::IfNode { .. } => {
                // modifier form
                let ifn = node.as_if_node().unwrap();
                self.is_modifier_if(&ifn) || !ifn.end_keyword_loc().is_some()
            }
            Node::UnlessNode { .. } => {
                let un = node.as_unless_node().unwrap();
                un.end_keyword_loc().is_none()
            }
            Node::WhileNode { .. } => {
                let wn = node.as_while_node().unwrap();
                // modifier while has no begin/end keywords in the traditional sense
                // actually check: modifier while = while_keyword on right of body
                // In Prism: modifier while has keyword_loc after the body
                let kw_start = wn.keyword_loc().start_offset();
                let body_start = if let Some(body) = wn.statements() {
                    body.location().start_offset()
                } else { kw_start };
                kw_start > body_start
            }
            Node::UntilNode { .. } => {
                let un = node.as_until_node().unwrap();
                let kw_start = un.keyword_loc().start_offset();
                let body_start = if let Some(body) = un.statements() {
                    body.location().start_offset()
                } else { kw_start };
                kw_start > body_start
            }
            Node::RangeNode { .. } => true,
            Node::RescueModifierNode { .. } => true,
            Node::AndNode { .. } => {
                // check if "and" keyword form (semantic)
                let an = node.as_and_node().unwrap();
                let op = self.ctx.src(an.operator_loc().start_offset(), an.operator_loc().end_offset());
                op == "and"
            }
            Node::OrNode { .. } => {
                let on = node.as_or_node().unwrap();
                let op = self.ctx.src(on.operator_loc().start_offset(), on.operator_loc().end_offset());
                op == "or"
            }
            _ => false,
        }
    }

    fn require_braces(&self, node: &Node) -> bool {
        // A bare keyword hash (no braces) needs braces when used as || RHS
        // In Prism: `KeywordHashNode` = hash without braces (unbraced keyword args)
        if node.as_keyword_hash_node().is_some() {
            return true;
        }
        false
    }

    fn without_argument_parentheses_method(&self, node: &Node) -> bool {
        let call = match node.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        // Has arguments, not parenthesized, not operator method, not assignment method
        if call.arguments().is_none() {
            return false;
        }
        if call.opening_loc().is_some() {
            return false;
        }
        let name = String::from_utf8_lossy(call.name().as_slice()).to_string();
        // operator method: name is an operator symbol
        let is_operator = self.is_operator_method_name(&name);
        if is_operator {
            return false;
        }
        if name.ends_with('=') {
            return false;
        }
        // Must have no receiver (bare method call)
        // Actually RuboCop also allows with receiver — let's check args count > 0
        let args = call.arguments().unwrap();
        let arg_list: Vec<_> = args.arguments().iter().collect();
        !arg_list.is_empty()
    }

    fn is_operator_method_name(&self, name: &str) -> bool {
        matches!(name, "+" | "-" | "*" | "/" | "%" | "**" | "<<" | ">>" |
            "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>" |
            "&" | "|" | "^" | "~" | "[]" | "[]=" | "=~" | "!~")
    }

    fn branches_have_arithmetic_op(&self, node: &ruby_prism::IfNode) -> bool {
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

        let if_call = match if_branch.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let else_call = match else_branch.as_call_node() {
            Some(c) => c,
            None => return false,
        };

        // arithmetic = operator method
        let if_name = String::from_utf8_lossy(if_call.name().as_slice()).to_string();
        if !self.is_operator_method_name(&if_name) || if_name == "[]" || if_name == "[]=" {
            return false;
        }

        let else_name = String::from_utf8_lossy(else_call.name().as_slice()).to_string();
        if if_name != else_name { return false; }

        // Same receiver
        match (if_call.receiver(), else_call.receiver()) {
            (Some(ir), Some(er)) => {
                if self.src(&ir) != self.src(&er) { return false; }
            }
            (None, None) => {}
            _ => return false,
        }

        // If branch's arg is the condition
        let if_args = match if_call.arguments() {
            Some(a) => a,
            None => return false,
        };
        let if_arg_list: Vec<_> = if_args.arguments().iter().collect();
        if if_arg_list.len() != 1 { return false; }
        let cond_src = self.src(&condition);
        let if_arg_src = self.src(&if_arg_list[0]);
        cond_src == if_arg_src
    }

    fn ternary_question_colon_range(&self, node: &ruby_prism::IfNode) -> (usize, usize) {
        let cond_end = node.predicate().location().end_offset();
        let node_end = node.location().end_offset();
        let src = self.ctx.src(cond_end, node_end);

        let q_pos = src.find('?').map(|p| cond_end + p);
        let c_pos = if let Some(qp) = q_pos {
            if let Some(stmts) = node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    let branch_end = last.location().end_offset();
                    let after_branch = self.ctx.src(branch_end, node_end);
                    after_branch.find(':').map(|p| branch_end + p + 1)
                } else {
                    let after_q = self.ctx.src(qp + 1, node_end);
                    after_q.find(':').map(|p| qp + 1 + p + 1)
                }
            } else {
                let after_q = self.ctx.src(qp + 1, node_end);
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

    fn get_else_branch_offsets(&self, node: &ruby_prism::IfNode) -> Option<(usize, usize)> {
        let sub = node.subsequent()?;
        let else_node = sub.as_else_node()?;
        let stmts = else_node.statements()?;
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() == 1 {
            let loc = body[0].location();
            Some((loc.start_offset(), loc.end_offset()))
        } else {
            None
        }
    }

    fn is_else_branch_range_node(&self, node: &ruby_prism::IfNode) -> bool {
        let sub = match node.subsequent() {
            Some(s) => s,
            None => return false,
        };
        let else_node = match sub.as_else_node() {
            Some(en) => en,
            None => return false,
        };
        let stmts = match else_node.statements() {
            Some(s) => s,
            None => return false,
        };
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() != 1 { return false; }
        matches!(&body[0], Node::RangeNode { .. })
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

        let has_comment = self.range_has_comment(node.location().start_offset(), node.location().end_offset());
        let correction = if has_comment {
            None
        } else {
            Some(self.compute_unless_correction(node))
        };

        let offense = self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention, start, end,
        );
        let offense = if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        };
        self.offenses.push(offense);
    }

    fn compute_unless_correction(&self, node: &ruby_prism::UnlessNode) -> Correction {
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        let cond_src = self.src(&node.predicate());

        // unless b; body; else; b; end → b || body
        // The "body" is the unless statements (the true branch of unless = if-not)
        let body_src = if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() == 1 {
                let bs = self.src(&body[0]);
                // Check if needs parens (is it multi-line / complex?)
                bs.to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let replacement = format!("{} || {}", cond_src, body_src);
        Correction::replace(node_start, node_end, replacement)
    }

    fn offense_if(&self, node: &ruby_prism::IfNode) -> bool {
        let condition = node.predicate();
        let is_ternary = self.is_ternary(node);

        let if_stmts = match node.statements() {
            Some(s) => s,
            None => return false,
        };
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        if if_body.len() != 1 {
            return false;
        }
        let if_branch = &if_body[0];

        if let Some(ref sub) = node.subsequent() {
            if matches!(sub, Node::IfNode { .. }) {
                return false;
            }
        }

        let else_branch_single = self.get_else_single_node_info(node.subsequent());

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

        if cond_src == if_src {
            if !is_ternary {
                if let Some(ref sub) = node.subsequent() {
                    if !self.else_has_single_expression(sub) {
                        return false;
                    }
                }
            }
            return true;
        }

        if self.if_branch_is_true_type_and_else_is_not(&condition, if_branch, node.subsequent()) {
            return true;
        }

        if !is_ternary && self.branches_have_assignment_if(node) {
            return true;
        }

        if !is_ternary && self.branches_have_arithmetic_op(node) {
            return true;
        }

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

    fn compute_true_branch_correction(
        &self,
        node: &ruby_prism::IfNode,
        is_ternary: bool,
        parent_is_call: bool,
    ) -> Option<Correction> {
        // if a.zero?; true; else; a; end → a.zero? || a
        // if foo? arg; true; else; bar; end → foo?(arg) || bar
        let condition = node.predicate();
        let call = condition.as_call_node()?;

        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        // Get else branch
        let else_src = if is_ternary {
            // Get else node from ternary
            let sub = node.subsequent()?;
            let else_node = sub.as_else_node()?;
            let stmts = else_node.statements()?;
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() != 1 { return None; }
            self.src(&body[0]).to_string()
        } else {
            let sub = node.subsequent()?;
            let else_node = sub.as_else_node()?;
            let stmts = else_node.statements()?;
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() != 1 { return None; }
            self.src(&body[0]).to_string()
        };

        // Build condition source (with parens if needed)
        let cond_with_parens = if call.arguments().is_some() && call.opening_loc().is_none() {
            // without_arg_parens predicate → wrap args
            self.wrap_predicate_args(call)
        } else {
            self.src(&condition).to_string()
        };

        let ternary_form = format!("{} || {}", cond_with_parens, else_src);
        let replacement = if parent_is_call {
            format!("({})", ternary_form)
        } else {
            ternary_form
        };

        Some(Correction::replace(node_start, node_end, replacement))
    }

    fn is_true_branch_predicate_needs_parens(&self, node: &ruby_prism::IfNode) -> bool {
        // Returns true if: if_branch is TrueNode AND condition is a predicate call without parens AND has args
        let if_stmts = match node.statements() {
            Some(s) => s,
            None => return false,
        };
        let if_body: Vec<_> = if_stmts.body().iter().collect();
        if if_body.len() != 1 { return false; }
        if !matches!(&if_body[0], Node::TrueNode { .. }) { return false; }
        let call = match node.predicate().as_call_node() {
            Some(c) => c,
            None => return false,
        };
        // Has args but no opening paren
        call.arguments().is_some() && call.opening_loc().is_none()
    }

    fn wrap_predicate_args(&self, call: ruby_prism::CallNode) -> String {
        // foo? arg → foo?(arg)
        let name = String::from_utf8_lossy(call.name().as_slice()).to_string();
        let method_end = call.message_loc()
            .map(|l| l.end_offset())
            .unwrap_or_else(|| call.location().end_offset());
        let prefix = if let Some(recv) = call.receiver() {
            let recv_src = self.src(&recv);
            let call_op = if let Some(op) = call.call_operator_loc() {
                self.ctx.src(op.start_offset(), op.end_offset())
            } else {
                "."
            };
            format!("{}{}{}", recv_src, call_op, name)
        } else {
            name.clone()
        };
        // Get args source
        let args_src = if let Some(args) = call.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            arg_list.iter().map(|a| self.src(a)).collect::<Vec<_>>().join(", ")
        } else {
            String::new()
        };
        format!("{}({})", prefix, args_src)
    }
}

#[derive(PartialEq)]
enum ElseNodeType {
    Normal,
    IfType,
    HashKeyAssign,
}

impl Visit<'_> for RedundantConditionVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let parent_is_call = self.in_call_arg;
        // Reset for children
        self.in_call_arg = false;
        self.check_if_node(node, parent_is_call);
        ruby_prism::visit_if_node(self, node);
        self.in_call_arg = parent_is_call;
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless_node(node);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check if arguments contain if-nodes — mark them as "parent is call"
        let saved = self.in_call_arg;
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() {
                if matches!(arg, Node::IfNode { .. }) {
                    self.in_call_arg = true;
                    self.visit(&arg);
                    self.in_call_arg = saved;
                } else {
                    self.in_call_arg = false;
                    self.visit(&arg);
                }
            }
        }
        // Visit receiver and other parts without marking
        self.in_call_arg = false;
        if let Some(recv) = node.receiver() {
            self.visit(&recv);
        }
        if let Some(block) = node.block() {
            self.visit(&block);
        }
        self.in_call_arg = saved;
    }
}

crate::register_cop!("Style/RedundantCondition", |cfg| {
    let cop_config = cfg.get_cop_config("Style/RedundantCondition");
    let allowed_methods = cop_config
        .and_then(|c| c.raw.get("AllowedMethods"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(|| vec!["infinite?".to_string(), "nonzero?".to_string()]);
    Some(Box::new(RedundantCondition::with_config(allowed_methods)))
});
