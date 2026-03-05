//! Lint/Void - Checks for operators, variables, literals, lambda, proc and nonmutating
//! methods used in void context.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/void.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

/// Binary operators checked in void context.
const BINARY_OPERATORS: &[&str] = &[
    "*", "/", "%", "+", "-", "==", "===", "!=", "<", ">", "<=", ">=", "<=>",
];

/// Unary operators checked in void context.
const UNARY_OPERATORS: &[&str] = &["+@", "-@", "~", "!"];

/// Nonmutating methods that have a bang (!) version.
const NONMUTATING_METHODS_WITH_BANG: &[&str] = &[
    "capitalize", "chomp", "chop", "compact", "delete_prefix", "delete_suffix",
    "downcase", "encode", "flatten", "gsub", "lstrip", "merge", "next",
    "reject", "reverse", "rotate", "rstrip", "scrub", "select", "shuffle",
    "slice", "sort", "sort_by", "squeeze", "strip", "sub", "succ", "swapcase",
    "tr", "tr_s", "transform_values", "unicode_normalize", "uniq", "upcase",
];

/// Methods replaceable by `each`.
const METHODS_REPLACEABLE_BY_EACH: &[&str] = &["collect", "map"];

pub struct Void {
    check_methods_with_no_side_effects: bool,
}

impl Void {
    pub fn new(check_methods_with_no_side_effects: bool) -> Self {
        Self {
            check_methods_with_no_side_effects,
        }
    }
}

impl Cop for Void {
    fn name(&self) -> &'static str {
        "Lint/Void"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = VoidVisitor {
            ctx,
            check_methods: self.check_methods_with_no_side_effects,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct VoidVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    check_methods: bool,
    offenses: Vec<Offense>,
}

impl<'a> VoidVisitor<'a> {
    /// Check a sequence of statements for void expressions.
    /// `void_context`: all statements are void (e.g., initialize, setter, for, ensure)
    /// `inside_each_block`: we're in an each block (last statement is not void)
    fn check_statements(
        &mut self,
        stmts: &[Node],
        void_context: bool,
        inside_each_block: bool,
    ) {
        if stmts.is_empty() {
            return;
        }

        let check_count = if void_context && !inside_each_block {
            stmts.len()
        } else {
            stmts.len().saturating_sub(1)
        };

        for expr in &stmts[..check_count] {
            self.check_void_op(expr, inside_each_block);
            self.check_expression(expr);
        }
    }

    /// Check a node for void operators.
    fn check_void_op(&mut self, node: &Node, inside_each_block: bool) {
        let call = match extract_call_through_parens(node) {
            Some(c) => c,
            None => return,
        };

        let method_name = String::from_utf8_lossy(call.name().as_slice());
        let method_str = method_name.as_ref();

        let is_binary = BINARY_OPERATORS.contains(&method_str);
        let is_unary = UNARY_OPERATORS.contains(&method_str);

        if !is_binary && !is_unary {
            return;
        }

        // For binary operators called via dot with no arguments, skip (it's a method call)
        if is_binary && call.call_operator_loc().is_some() {
            let has_args = if let Some(args) = call.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                !arg_list.is_empty()
            } else {
                false
            };
            if !has_args {
                return;
            }
        }

        if inside_each_block {
            return;
        }

        let message = format!("Operator `{}` used in void context.", method_str);
        if let Some(msg_loc) = call.message_loc() {
            let offense = self.ctx.offense_with_range(
                "Lint/Void",
                &message,
                Severity::Warning,
                msg_loc.start_offset(),
                msg_loc.end_offset(),
            );

            let correction = self.autocorrect_void_op(&call);

            match correction {
                Some(c) => self.offenses.push(offense.with_correction(c)),
                None => self.offenses.push(offense),
            }
        }
    }

    fn autocorrect_void_op(&self, call: &ruby_prism::CallNode) -> Option<Correction> {
        let has_args = call.arguments().map_or(false, |args| {
            args.arguments().iter().count() > 0
        });

        if !has_args {
            // Unary or no-arg: replace entire call with just receiver source
            let receiver = call.receiver()?;
            let recv_src = &self.ctx.source[receiver.location().start_offset()..receiver.location().end_offset()];
            Some(Correction::replace(
                call.location().start_offset(),
                call.location().end_offset(),
                recv_src.to_string(),
            ))
        } else {
            let msg_loc = call.message_loc()?;
            if let Some(dot_loc) = call.call_operator_loc() {
                // Called via dot: `a.*(b)` -> `a\n(b)`
                Some(Correction::replace(
                    dot_loc.start_offset(),
                    msg_loc.end_offset(),
                    "\n".to_string(),
                ))
            } else {
                // Infix: `a * b` -> `a\nb`
                let source = self.ctx.source.as_bytes();
                let mut ws_start = msg_loc.start_offset();
                while ws_start > 0 && (source[ws_start - 1] == b' ' || source[ws_start - 1] == b'\t') {
                    ws_start -= 1;
                }
                let mut ws_end = msg_loc.end_offset();
                while ws_end < source.len() && (source[ws_end] == b' ' || source[ws_end] == b'\t') {
                    ws_end += 1;
                }
                Some(Correction::replace(ws_start, ws_end, "\n".to_string()))
            }
        }
    }

    /// Check a node for void expressions (literal, var, self, defined?, lambda/proc, nonmutating).
    /// Like check_expression but skips autocorrection (for nodes inside conditionals).
    fn check_expression_no_correct(&mut self, expr: &Node) {
        match expr {
            Node::IfNode { .. } => {
                let if_node = expr.as_if_node().unwrap();
                self.check_if_expression(&if_node);
            }
            Node::UnlessNode { .. } => {
                let unless_node = expr.as_unless_node().unwrap();
                self.check_unless_expression(&unless_node);
            }
            Node::CaseNode { .. } => {
                let case_node = expr.as_case_node().unwrap();
                self.check_case_expression(&case_node);
            }
            Node::CaseMatchNode { .. } => {
                let case_match = expr.as_case_match_node().unwrap();
                self.check_case_match_expression(&case_match);
            }
            _ => {
                self.check_void_expression_nodes_no_correct(expr);
            }
        }
    }

    fn check_expression(&mut self, expr: &Node) {
        match expr {
            Node::IfNode { .. } => {
                let if_node = expr.as_if_node().unwrap();
                self.check_if_expression(&if_node);
            }
            Node::UnlessNode { .. } => {
                let unless_node = expr.as_unless_node().unwrap();
                self.check_unless_expression(&unless_node);
            }
            Node::CaseNode { .. } => {
                let case_node = expr.as_case_node().unwrap();
                self.check_case_expression(&case_node);
            }
            Node::CaseMatchNode { .. } => {
                let case_match = expr.as_case_match_node().unwrap();
                self.check_case_match_expression(&case_match);
            }
            _ => {
                self.check_void_expression_nodes(expr);
            }
        }
    }

    fn check_void_expression_nodes(&mut self, expr: &Node) {
        self.check_void_expression_nodes_inner(expr, false);
    }

    fn check_void_expression_nodes_no_correct(&mut self, expr: &Node) {
        self.check_void_expression_nodes_inner(expr, true);
    }

    fn check_void_expression_nodes_inner(&mut self, expr: &Node, skip_correction: bool) {
        self.check_literal(expr, skip_correction);
        self.check_var(expr, skip_correction);
        self.check_self(expr, skip_correction);
        self.check_void_expression(expr, skip_correction);
        if self.check_methods {
            self.check_nonmutating(expr);
        }
    }

    fn check_if_expression(&mut self, if_node: &ruby_prism::IfNode) {
        if let Some(body) = if_node.statements() {
            let stmts: Vec<_> = body.body().iter().collect();
            if stmts.len() == 1 {
                self.check_void_expression_nodes_no_correct(&stmts[0]);
            }
        }
    }

    fn check_unless_expression(&mut self, unless_node: &ruby_prism::UnlessNode) {
        if let Some(body) = unless_node.statements() {
            let stmts: Vec<_> = body.body().iter().collect();
            if stmts.len() == 1 {
                self.check_void_expression_nodes_no_correct(&stmts[0]);
            }
        }
    }

    fn check_case_expression(&mut self, case_node: &ruby_prism::CaseNode) {
        for cond in case_node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                let when = cond.as_when_node().unwrap();
                if let Some(body) = when.statements() {
                    let stmts: Vec<_> = body.body().iter().collect();
                    if stmts.len() == 1 {
                        self.check_expression_no_correct(&stmts[0]);
                    }
                }
            }
        }
        if let Some(else_clause) = case_node.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if body.len() == 1 {
                    self.check_expression_no_correct(&body[0]);
                }
            }
        }
    }

    fn check_case_match_expression(&mut self, case_match: &ruby_prism::CaseMatchNode) {
        for cond in case_match.conditions().iter() {
            if let Node::InNode { .. } = &cond {
                let in_node = cond.as_in_node().unwrap();
                if let Some(body) = in_node.statements() {
                    let stmts: Vec<_> = body.body().iter().collect();
                    if stmts.len() == 1 {
                        self.check_expression_no_correct(&stmts[0]);
                    }
                }
            }
        }
        if let Some(else_clause) = case_match.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if body.len() == 1 {
                    self.check_expression_no_correct(&body[0]);
                }
            }
        }
    }

    fn check_literal(&mut self, node: &Node, skip_correction: bool) {
        if !entirely_literal(node) {
            return;
        }
        match node {
            Node::XStringNode { .. }
            | Node::InterpolatedXStringNode { .. }
            | Node::RangeNode { .. }
            | Node::NilNode { .. } => return,
            _ => {}
        }

        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        let message = format!("Literal `{}` used in void context.", source_text);

        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            &message,
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        );

        if skip_correction {
            self.offenses.push(offense);
        } else {
            let correction = self.autocorrect_void_expression(node);
            match correction {
                Some(c) => self.offenses.push(offense.with_correction(c)),
                None => self.offenses.push(offense),
            }
        }
    }

    fn check_var(&mut self, node: &Node, skip_correction: bool) {
        let (is_variable, is_const) = match node {
            Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::NumberedReferenceReadNode { .. }
            | Node::BackReferenceReadNode { .. } => (true, false),
            Node::ConstantReadNode { .. }
            | Node::ConstantPathNode { .. } => (false, true),
            Node::SourceEncodingNode { .. } => (true, false),
            _ => return,
        };

        if !is_variable && !is_const {
            return;
        }

        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];

        let template = if is_const {
            format!("Constant `{}` used in void context.", source_text)
        } else {
            format!("Variable `{}` used in void context.", source_text)
        };

        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            &template,
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        );

        if skip_correction {
            self.offenses.push(offense);
        } else {
            let correction = self.autocorrect_void_expression(node);
            match correction {
                Some(c) => self.offenses.push(offense.with_correction(c)),
                None => self.offenses.push(offense),
            }
        }
    }

    fn check_self(&mut self, node: &Node, skip_correction: bool) {
        if !matches!(node, Node::SelfNode { .. }) {
            return;
        }

        let loc = node.location();
        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            "`self` used in void context.",
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        );

        if skip_correction {
            self.offenses.push(offense);
        } else {
            let correction = self.autocorrect_void_expression(node);
            match correction {
                Some(c) => self.offenses.push(offense.with_correction(c)),
                None => self.offenses.push(offense),
            }
        }
    }

    fn check_void_expression(&mut self, node: &Node, skip_correction: bool) {
        let is_defined = matches!(node, Node::DefinedNode { .. });
        let is_lambda_or_proc = is_lambda_or_proc(node);

        if !is_defined && !is_lambda_or_proc {
            return;
        }

        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        let message = format!("`{}` used in void context.", source_text);

        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            &message,
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        );

        if skip_correction {
            self.offenses.push(offense);
        } else {
            let correction = self.autocorrect_void_expression(node);
            match correction {
                Some(c) => self.offenses.push(offense.with_correction(c)),
                None => self.offenses.push(offense),
            }
        }
    }

    fn check_nonmutating(&mut self, node: &Node) {
        let call = match node {
            Node::CallNode { .. } => node.as_call_node().unwrap(),
            _ => return,
        };

        let method_name = String::from_utf8_lossy(call.name().as_slice()).to_string();

        let is_nonmutating_bang = NONMUTATING_METHODS_WITH_BANG.contains(&method_name.as_str());
        let is_replaceable_by_each = METHODS_REPLACEABLE_BY_EACH.contains(&method_name.as_str());

        if !is_nonmutating_bang && !is_replaceable_by_each {
            return;
        }

        let suggestion = if is_replaceable_by_each {
            "each".to_string()
        } else {
            format!("{}!", method_name)
        };

        let message = format!(
            "Method `#{}` used in void context. Did you mean `#{}`?",
            method_name, suggestion
        );

        // Determine offense range: if the call has a block, span to end of first line
        let loc = node.location();
        let offense_start = loc.start_offset();
        let offense_end = if call.block().is_some() {
            // Find end of first line from the call start
            let source = self.ctx.source.as_bytes();
            let mut end = loc.start_offset();
            while end < loc.end_offset() && source[end] != b'\n' {
                end += 1;
            }
            end
        } else {
            loc.end_offset()
        };

        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            &message,
            Severity::Warning,
            offense_start,
            offense_end,
        );

        if let Some(msg_loc) = call.message_loc() {
            let correction = Correction::replace(
                msg_loc.start_offset(),
                msg_loc.end_offset(),
                suggestion,
            );
            self.offenses.push(offense.with_correction(correction));
        } else {
            self.offenses.push(offense);
        }
    }

    /// Autocorrect void expression: remove the node with leading whitespace (left side only).
    /// Matches RuboCop's `range_with_surrounding_space(side: :left)`.
    fn autocorrect_void_expression(&self, node: &Node) -> Option<Correction> {
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        let source = self.ctx.source.as_bytes();

        // Find leading whitespace
        let mut remove_start = start;
        while remove_start > 0
            && (source[remove_start - 1] == b' ' || source[remove_start - 1] == b'\t')
        {
            remove_start -= 1;
        }
        // Consume preceding newline (left side only, matching RuboCop's side: :left)
        if remove_start > 0 && source[remove_start - 1] == b'\n' {
            remove_start -= 1;
            if remove_start > 0 && source[remove_start - 1] == b'\r' {
                remove_start -= 1;
            }
        }

        Some(Correction::delete(remove_start, end))
    }

    /// Is this def in void context? (initialize or setter method)
    fn is_void_method(def_node: &ruby_prism::DefNode) -> bool {
        let name = String::from_utf8_lossy(def_node.name().as_slice());
        let name_str = name.as_ref();

        if name_str == "initialize" {
            return true;
        }

        // Setter methods (end with = but not ==, ===, !=, <=, >=, <=>)
        if name_str.ends_with('=')
            && name_str != "=="
            && name_str != "==="
            && name_str != "!="
            && name_str != "<="
            && name_str != ">="
            && name_str != "<=>"
        {
            return true;
        }

        false
    }

    /// Check block body statements.
    /// `is_each`: parent call is `each` (last stmt is NOT void)
    /// `is_tap`: parent call is `tap` (ALL stmts are void, like initialize)
    fn check_block_body(
        &mut self,
        block_node: &Node,
        is_each: bool,
        is_tap: bool,
    ) {
        let block = match block_node {
            Node::BlockNode { .. } => block_node.as_block_node().unwrap(),
            _ => return,
        };
        if let Some(body) = block.body() {
            match &body {
                Node::StatementsNode { .. } => {
                    let stmts_node = body.as_statements_node().unwrap();
                    let stmts: Vec<_> = stmts_node.body().iter().collect();
                    if is_each {
                        self.check_statements(&stmts, false, true);
                    } else if is_tap {
                        // tap blocks: ALL statements are void (like initialize)
                        self.check_statements(&stmts, true, false);
                    } else {
                        self.check_statements(&stmts, true, false);
                    }
                }
                _ => {
                    if !is_each {
                        self.check_void_op(&body, false);
                        self.check_expression(&body);
                    }
                }
            }
        }
    }
}

impl Visit<'_> for VoidVisitor<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        // ProgramNode.statements() returns StatementsNode directly (not Option)
        let stmts = node.statements();
        let body: Vec<_> = stmts.body().iter().collect();
        self.check_statements(&body, false, false);
        ruby_prism::visit_program_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let is_void = VoidVisitor::is_void_method(node);

        if let Some(body) = node.body() {
            if let Node::StatementsNode { .. } = &body {
                let stmts_node = body.as_statements_node().unwrap();
                let stmts: Vec<_> = stmts_node.body().iter().collect();
                self.check_statements(&stmts, is_void, false);
            }
            // BeginNode inside def is handled by visit_begin_node
        }

        ruby_prism::visit_def_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            self.check_statements(&body, false, false);
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode) {
        // Ensure block: all statements are void
        if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            self.check_statements(&body, true, false);
        }
        ruby_prism::visit_ensure_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check if this call has a block
        if let Some(block_node) = node.block() {
            if let Node::BlockNode { .. } = &block_node {
                let method_name = String::from_utf8_lossy(node.name().as_slice());
                let method_str = method_name.as_ref();
                let is_each = method_str == "each";
                let is_tap = method_str == "tap";
                self.check_block_body(&block_node, is_each, is_tap);
            }
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        // For loops: all statements are void
        if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            self.check_statements(&body, true, false);
        }
        ruby_prism::visit_for_node(self, node);
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        // Handle `(stmt1; stmt2)` as begin block
        if let Some(body) = node.body() {
            if let Node::StatementsNode { .. } = &body {
                let stmts_node = body.as_statements_node().unwrap();
                let stmts: Vec<_> = stmts_node.body().iter().collect();
                self.check_statements(&stmts, false, false);
            }
        }
        ruby_prism::visit_parentheses_node(self, node);
    }

    fn visit_keyword_hash_node(&mut self, _node: &ruby_prism::KeywordHashNode) {
        // Don't recurse into keyword hash nodes - they are arguments, not void contexts
    }
}

/// Extract a CallNode from a node, unwrapping up to 2 levels of parentheses.
/// Returns the CallNode if found, None otherwise.
fn extract_call_through_parens<'a>(node: &'a Node<'a>) -> Option<ruby_prism::CallNode<'a>> {
    match node {
        Node::CallNode { .. } => node.as_call_node(),
        Node::ParenthesesNode { .. } => {
            let paren = node.as_parentheses_node().unwrap();
            let body = paren.body()?;
            if let Node::StatementsNode { .. } = &body {
                let stmts = body.as_statements_node().unwrap();
                let stmts_body: Vec<_> = stmts.body().iter().collect();
                if stmts_body.len() == 1 {
                    match &stmts_body[0] {
                        Node::CallNode { .. } => return stmts_body[0].as_call_node(),
                        Node::ParenthesesNode { .. } => {
                            // Second level of parens
                            let inner_paren = stmts_body[0].as_parentheses_node().unwrap();
                            if let Some(inner_body) = inner_paren.body() {
                                if let Node::StatementsNode { .. } = &inner_body {
                                    let inner_stmts = inner_body.as_statements_node().unwrap();
                                    let inner_nodes: Vec<_> = inner_stmts.body().iter().collect();
                                    if inner_nodes.len() == 1 {
                                        if let Node::CallNode { .. } = &inner_nodes[0] {
                                            return inner_nodes[0].as_call_node();
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Check if a node is a lambda or proc (without .call).
fn is_lambda_or_proc(node: &Node) -> bool {
    match node {
        Node::LambdaNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let method_name = String::from_utf8_lossy(call.name().as_slice());
            let method_str = method_name.as_ref();

            // `lambda { }` or `proc { }` (no receiver, with block)
            if call.receiver().is_none()
                && (method_str == "lambda" || method_str == "proc")
                && call.block().is_some()
            {
                return true;
            }

            // `Proc.new { }` (receiver is Proc, method is new, with block)
            if method_str == "new" {
                if let Some(recv) = call.receiver() {
                    if let Node::ConstantReadNode { .. } = &recv {
                        let const_name = recv.as_constant_read_node().unwrap();
                        let name = String::from_utf8_lossy(const_name.name().as_slice());
                        if name.as_ref() == "Proc" && call.block().is_some() {
                            return true;
                        }
                    }
                }
            }

            false
        }
        _ => false,
    }
}

/// Check if a node is entirely literal.
fn entirely_literal(node: &Node) -> bool {
    match node {
        Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. }
        | Node::StringNode { .. }
        | Node::InterpolatedStringNode { .. }
        | Node::SymbolNode { .. }
        | Node::InterpolatedSymbolNode { .. }
        | Node::RegularExpressionNode { .. }
        | Node::InterpolatedRegularExpressionNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::NilNode { .. }
        | Node::SourceLineNode { .. }
        | Node::SourceFileNode { .. }
        | Node::RangeNode { .. }
        | Node::XStringNode { .. }
        | Node::InterpolatedXStringNode { .. } => true,

        Node::ArrayNode { .. } => {
            let arr = node.as_array_node().unwrap();
            arr.elements().iter().all(|e| entirely_literal(&e))
        }

        Node::HashNode { .. } => {
            let hash = node.as_hash_node().unwrap();
            hash.elements().iter().all(|e| {
                if let Node::AssocNode { .. } = &e {
                    let assoc = e.as_assoc_node().unwrap();
                    entirely_literal(&assoc.key()) && entirely_literal(&assoc.value())
                } else {
                    false
                }
            })
        }

        // `.freeze` or `&.freeze` on a literal
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let method_name = String::from_utf8_lossy(call.name().as_slice());
            if method_name.as_ref() == "freeze" {
                if let Some(recv) = call.receiver() {
                    return entirely_literal(&recv);
                }
            }
            false
        }

        _ => false,
    }
}
