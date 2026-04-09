use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const BINARY_OPERATORS: &[&str] = &[
    "*", "/", "%", "+", "-", "==", "===", "!=", "<", ">", "<=", ">=", "<=>",
];

const UNARY_OPERATORS: &[&str] = &["+@", "-@", "~", "!"];

const NONMUTATING_METHODS_WITH_BANG: &[&str] = &[
    "capitalize", "chomp", "chop", "compact", "delete_prefix", "delete_suffix",
    "downcase", "encode", "flatten", "gsub", "lstrip", "merge", "next",
    "reject", "reverse", "rotate", "rstrip", "scrub", "select", "shuffle",
    "slice", "sort", "sort_by", "squeeze", "strip", "sub", "succ", "swapcase",
    "tr", "tr_s", "transform_values", "unicode_normalize", "uniq", "upcase",
];

const METHODS_REPLACEABLE_BY_EACH: &[&str] = &["collect", "map"];

pub struct Void {
    check_methods_with_no_side_effects: bool,
}

impl Void {
    pub fn new(check_methods_with_no_side_effects: bool) -> Self {
        Self { check_methods_with_no_side_effects }
    }
}

impl Cop for Void {
    fn name(&self) -> &'static str { "Lint/Void" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
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
    /// Push an offense, optionally attaching a correction when `allow_correct` is true.
    fn push_offense(&mut self, offense: Offense, correction: Option<Correction>, allow_correct: bool) {
        self.offenses.push(if allow_correct {
            match correction {
                Some(c) => offense.with_correction(c),
                None => offense,
            }
        } else {
            offense
        });
    }

    fn check_statements(&mut self, stmts: &[Node], void_context: bool, inside_each_block: bool) {
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
            self.check_expression(expr, true);
        }
    }

    fn check_void_op(&mut self, node: &Node, inside_each_block: bool) {
        let call = match extract_call_through_parens(node) {
            Some(c) => c,
            None => return,
        };

        let method_name = node_name!(call);
        let method_str = method_name.as_ref();

        let is_binary = BINARY_OPERATORS.contains(&method_str);
        let is_unary = UNARY_OPERATORS.contains(&method_str);

        if !is_binary && !is_unary {
            return;
        }

        if is_binary && call.call_operator_loc().is_some() {
            let has_args = call.arguments().map_or(false, |args| {
                args.arguments().iter().next().is_some()
            });
            if !has_args {
                return;
            }
        }

        if inside_each_block {
            return;
        }

        if let Some(msg_loc) = call.message_loc() {
            let message = format!("Operator `{}` used in void context.", method_str);
            let offense = self.ctx.offense_with_range(
                "Lint/Void", &message, Severity::Warning,
                msg_loc.start_offset(), msg_loc.end_offset(),
            );
            let correction = self.autocorrect_void_op(&call);
            self.push_offense(offense, correction, true);
        }
    }

    fn autocorrect_void_op(&self, call: &ruby_prism::CallNode) -> Option<Correction> {
        let has_args = call.arguments().map_or(false, |args| args.arguments().iter().next().is_some());

        if !has_args {
            let receiver = call.receiver()?;
            let recv_src = &self.ctx.source[receiver.location().start_offset()..receiver.location().end_offset()];
            Some(Correction::replace(
                call.location().start_offset(),
                call.location().end_offset(),
                recv_src.to_string(),
            ))
        } else if let Some(dot_loc) = call.call_operator_loc() {
            let msg_loc = call.message_loc()?;
            Some(Correction::replace(dot_loc.start_offset(), msg_loc.end_offset(), "\n".to_string()))
        } else {
            let msg_loc = call.message_loc()?;
            let source = self.ctx.source.as_bytes();
            let mut ws_start = msg_loc.start_offset();
            while ws_start > 0 && matches!(source[ws_start - 1], b' ' | b'\t') {
                ws_start -= 1;
            }
            let mut ws_end = msg_loc.end_offset();
            while ws_end < source.len() && matches!(source[ws_end], b' ' | b'\t') {
                ws_end += 1;
            }
            Some(Correction::replace(ws_start, ws_end, "\n".to_string()))
        }
    }

    fn check_expression(&mut self, expr: &Node, allow_correct: bool) {
        match expr {
            Node::IfNode { .. } => self.check_if_expression(&expr.as_if_node().unwrap()),
            Node::UnlessNode { .. } => self.check_unless_expression(&expr.as_unless_node().unwrap()),
            Node::CaseNode { .. } => self.check_case_expression(&expr.as_case_node().unwrap()),
            Node::CaseMatchNode { .. } => self.check_case_match_expression(&expr.as_case_match_node().unwrap()),
            _ => self.check_void_expression_nodes(expr, allow_correct),
        }
    }

    fn check_void_expression_nodes(&mut self, expr: &Node, allow_correct: bool) {
        self.check_literal(expr, allow_correct);
        self.check_var(expr, allow_correct);
        self.check_self(expr, allow_correct);
        self.check_void_expression(expr, allow_correct);
        if self.check_methods {
            self.check_nonmutating(expr);
        }
    }

    fn check_single_stmt_body(&mut self, stmts: Option<ruby_prism::StatementsNode>, no_correct: bool) {
        if let Some(body) = stmts {
            let stmts: Vec<_> = body.body().iter().collect();
            if stmts.len() == 1 {
                if no_correct {
                    self.check_void_expression_nodes(&stmts[0], false);
                } else {
                    self.check_expression(&stmts[0], false);
                }
            }
        }
    }

    fn check_if_expression(&mut self, node: &ruby_prism::IfNode) {
        self.check_single_stmt_body(node.statements(), true);
    }

    fn check_unless_expression(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_single_stmt_body(node.statements(), true);
    }

    fn check_case_expression(&mut self, case_node: &ruby_prism::CaseNode) {
        for cond in case_node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                self.check_single_stmt_body(cond.as_when_node().unwrap().statements(), false);
            }
        }
        if let Some(else_clause) = case_node.else_clause() {
            self.check_single_stmt_body(else_clause.statements(), false);
        }
    }

    fn check_case_match_expression(&mut self, case_match: &ruby_prism::CaseMatchNode) {
        for cond in case_match.conditions().iter() {
            if let Node::InNode { .. } = &cond {
                self.check_single_stmt_body(cond.as_in_node().unwrap().statements(), false);
            }
        }
        if let Some(else_clause) = case_match.else_clause() {
            self.check_single_stmt_body(else_clause.statements(), false);
        }
    }

    fn check_literal(&mut self, node: &Node, allow_correct: bool) {
        if !entirely_literal(node) {
            return;
        }
        match node {
            Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. }
            | Node::RangeNode { .. } | Node::NilNode { .. } => return,
            _ => {}
        }
        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            &format!("Literal `{}` used in void context.", source_text),
            Severity::Warning, loc.start_offset(), loc.end_offset(),
        );
        let correction = self.autocorrect_void_expression(node);
        self.push_offense(offense, correction, allow_correct);
    }

    fn check_var(&mut self, node: &Node, allow_correct: bool) {
        let (is_variable, is_const) = match node {
            Node::LocalVariableReadNode { .. } | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. } | Node::GlobalVariableReadNode { .. }
            | Node::NumberedReferenceReadNode { .. } | Node::BackReferenceReadNode { .. }
            | Node::SourceEncodingNode { .. } => (true, false),
            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => (false, true),
            _ => return,
        };
        if !is_variable && !is_const {
            return;
        }
        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        let kind = if is_const { "Constant" } else { "Variable" };
        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            &format!("{} `{}` used in void context.", kind, source_text),
            Severity::Warning, loc.start_offset(), loc.end_offset(),
        );
        let correction = self.autocorrect_void_expression(node);
        self.push_offense(offense, correction, allow_correct);
    }

    fn check_self(&mut self, node: &Node, allow_correct: bool) {
        if !matches!(node, Node::SelfNode { .. }) {
            return;
        }
        let loc = node.location();
        let offense = self.ctx.offense_with_range(
            "Lint/Void", "`self` used in void context.",
            Severity::Warning, loc.start_offset(), loc.end_offset(),
        );
        let correction = self.autocorrect_void_expression(node);
        self.push_offense(offense, correction, allow_correct);
    }

    fn check_void_expression(&mut self, node: &Node, allow_correct: bool) {
        let is_defined = matches!(node, Node::DefinedNode { .. });
        let is_lambda_or_proc = is_lambda_or_proc(node);
        if !is_defined && !is_lambda_or_proc {
            return;
        }
        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        let offense = self.ctx.offense_with_range(
            "Lint/Void",
            &format!("`{}` used in void context.", source_text),
            Severity::Warning, loc.start_offset(), loc.end_offset(),
        );
        let correction = self.autocorrect_void_expression(node);
        self.push_offense(offense, correction, allow_correct);
    }

    fn check_nonmutating(&mut self, node: &Node) {
        let call = match node {
            Node::CallNode { .. } => node.as_call_node().unwrap(),
            _ => return,
        };
        let method_name = node_name!(call).to_string();
        let is_nonmutating_bang = NONMUTATING_METHODS_WITH_BANG.contains(&method_name.as_str());
        let is_replaceable_by_each = METHODS_REPLACEABLE_BY_EACH.contains(&method_name.as_str());
        if !is_nonmutating_bang && !is_replaceable_by_each {
            return;
        }
        let suggestion = if is_replaceable_by_each { "each".to_string() } else { format!("{}!", method_name) };
        let message = format!("Method `#{}` used in void context. Did you mean `#{}`?", method_name, suggestion);

        let loc = node.location();
        let offense_end = if call.block().is_some() {
            let source = self.ctx.source.as_bytes();
            let mut end = loc.start_offset();
            while end < loc.end_offset() && source[end] != b'\n' {
                end += 1;
            }
            end
        } else {
            loc.end_offset()
        };

        let offense = self.ctx.offense_with_range("Lint/Void", &message, Severity::Warning, loc.start_offset(), offense_end);

        if let Some(msg_loc) = call.message_loc() {
            let correction = Correction::replace(msg_loc.start_offset(), msg_loc.end_offset(), suggestion);
            self.offenses.push(offense.with_correction(correction));
        } else {
            self.offenses.push(offense);
        }
    }

    fn autocorrect_void_expression(&self, node: &Node) -> Option<Correction> {
        let loc = node.location();
        let source = self.ctx.source.as_bytes();
        let mut remove_start = loc.start_offset();
        while remove_start > 0 && matches!(source[remove_start - 1], b' ' | b'\t') {
            remove_start -= 1;
        }
        if remove_start > 0 && source[remove_start - 1] == b'\n' {
            remove_start -= 1;
            if remove_start > 0 && source[remove_start - 1] == b'\r' {
                remove_start -= 1;
            }
        }
        Some(Correction::delete(remove_start, loc.end_offset()))
    }

    fn is_void_method(def_node: &ruby_prism::DefNode) -> bool {
        let name = node_name!(def_node);
        let name_str = name.as_ref();
        name_str == "initialize"
            || (name_str.ends_with('=')
                && !matches!(name_str, "==" | "===" | "!=" | "<=" | ">=" | "<=>"))
    }

    fn check_block_body(&mut self, block_node: &Node, is_each: bool, _is_tap: bool) {
        let block = match block_node {
            Node::BlockNode { .. } => block_node.as_block_node().unwrap(),
            _ => return,
        };
        if let Some(body) = block.body() {
            match &body {
                Node::StatementsNode { .. } => {
                    let stmts: Vec<_> = body.as_statements_node().unwrap().body().iter().collect();
                    if is_each {
                        self.check_statements(&stmts, false, true);
                    } else {
                        self.check_statements(&stmts, true, false);
                    }
                }
                _ => {
                    if !is_each {
                        self.check_void_op(&body, false);
                        self.check_expression(&body, true);
                    }
                }
            }
        }
    }
}

impl Visit<'_> for VoidVisitor<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        let body: Vec<_> = node.statements().body().iter().collect();
        self.check_statements(&body, false, false);
        ruby_prism::visit_program_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if let Some(body) = node.body() {
            if let Node::StatementsNode { .. } = &body {
                let stmts: Vec<_> = body.as_statements_node().unwrap().body().iter().collect();
                self.check_statements(&stmts, VoidVisitor::is_void_method(node), false);
            }
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
        if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            self.check_statements(&body, true, false);
        }
        ruby_prism::visit_ensure_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(block_node) = node.block() {
            if let Node::BlockNode { .. } = &block_node {
                let method_name = node_name!(node);
                self.check_block_body(&block_node, method_name == "each", method_name == "tap");
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            self.check_statements(&body, true, false);
        }
        ruby_prism::visit_for_node(self, node);
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        if let Some(body) = node.body() {
            if let Node::StatementsNode { .. } = &body {
                let stmts: Vec<_> = body.as_statements_node().unwrap().body().iter().collect();
                self.check_statements(&stmts, false, false);
            }
        }
        ruby_prism::visit_parentheses_node(self, node);
    }

    fn visit_keyword_hash_node(&mut self, _node: &ruby_prism::KeywordHashNode) {}
}

fn extract_call_through_parens<'a>(node: &'a Node<'a>) -> Option<ruby_prism::CallNode<'a>> {
    match node {
        Node::CallNode { .. } => node.as_call_node(),
        Node::ParenthesesNode { .. } => {
            let paren = node.as_parentheses_node().unwrap();
            let body = paren.body()?;
            if let Node::StatementsNode { .. } = &body {
                let stmts: Vec<_> = body.as_statements_node().unwrap().body().iter().collect();
                if stmts.len() == 1 {
                    match &stmts[0] {
                        Node::CallNode { .. } => return stmts[0].as_call_node(),
                        Node::ParenthesesNode { .. } => {
                            let inner_paren = stmts[0].as_parentheses_node().unwrap();
                            if let Some(inner_body) = inner_paren.body() {
                                if let Node::StatementsNode { .. } = &inner_body {
                                    let inner_nodes: Vec<_> = inner_body.as_statements_node().unwrap().body().iter().collect();
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

fn is_lambda_or_proc(node: &Node) -> bool {
    match node {
        Node::LambdaNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let method_name = node_name!(call);
            let method_str = method_name.as_ref();

            if call.receiver().is_none()
                && matches!(method_str, "lambda" | "proc")
                && call.block().is_some()
            {
                return true;
            }

            if method_str == "new" {
                if let Some(recv) = call.receiver() {
                    if let Node::ConstantReadNode { .. } = &recv {
                        let name = node_name!(recv.as_constant_read_node().unwrap());
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

fn entirely_literal(node: &Node) -> bool {
    match node {
        Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. } | Node::StringNode { .. } | Node::InterpolatedStringNode { .. }
        | Node::SymbolNode { .. } | Node::InterpolatedSymbolNode { .. }
        | Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
        | Node::TrueNode { .. } | Node::FalseNode { .. } | Node::NilNode { .. }
        | Node::SourceLineNode { .. } | Node::SourceFileNode { .. } | Node::RangeNode { .. }
        | Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. } => true,

        Node::ArrayNode { .. } => {
            node.as_array_node().unwrap().elements().iter().all(|e| entirely_literal(&e))
        }

        Node::HashNode { .. } => {
            node.as_hash_node().unwrap().elements().iter().all(|e| {
                if let Node::AssocNode { .. } = &e {
                    let assoc = e.as_assoc_node().unwrap();
                    entirely_literal(&assoc.key()) && entirely_literal(&assoc.value())
                } else {
                    false
                }
            })
        }

        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let method_name = node_name!(call);
            method_name.as_ref() == "freeze"
                && call.receiver().map_or(false, |recv| entirely_literal(&recv))
        }

        _ => false,
    }
}
