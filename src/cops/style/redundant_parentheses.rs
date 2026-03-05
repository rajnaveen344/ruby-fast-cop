use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantParentheses";

pub struct RedundantParentheses {
    ternary_parentheses_required: bool,
    allow_in_multiline_conditions: bool,
}

impl RedundantParentheses {
    pub fn new() -> Self {
        Self {
            ternary_parentheses_required: false,
            allow_in_multiline_conditions: false,
        }
    }

    pub fn with_config(
        ternary_parentheses_required: bool,
        allow_in_multiline_conditions: bool,
    ) -> Self {
        Self {
            ternary_parentheses_required,
            allow_in_multiline_conditions,
        }
    }
}

impl Default for RedundantParentheses {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for RedundantParentheses {
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
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
            parent_stack: Vec::new(),
            ternary_parentheses_required: self.ternary_parentheses_required,
            allow_in_multiline_conditions: self.allow_in_multiline_conditions,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PK {
    Program,
    Statements,
    CallArgs,
    CallReceiver,
    Condition,
    CaseCondition,
    DefBody,
    BlockBody,
    IfBody,
    Assignment,
    OpAssignment,
    BinaryOp,
    UnaryOp,
    FlowControl,
    KeywordCall,
    Splat,
    Array,
    Hash,
    RangeOperand,
    Interpolation,
    Ternary,
    TernaryCondition,
    PinExpression,
    ExponentBase,
    ExponentPower,
    MethodCallUnparen,
    EndlessMethodBody,
    Other,
}

#[derive(Debug, Clone)]
struct PC {
    kind: PK,
    operator: Option<String>,
    call_has_parens: bool,
    arg_count: usize,
    is_first_arg: bool,
}

impl PC {
    fn new(kind: PK) -> Self {
        Self { kind, operator: None, call_has_parens: false, arg_count: 0, is_first_arg: false }
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    parent_stack: Vec<PC>,
    ternary_parentheses_required: bool,
    allow_in_multiline_conditions: bool,
}

fn get_inner_offsets(body: &Node) -> (usize, usize) {
    if let Node::StatementsNode { .. } = body {
        let stmts = body.as_statements_node().unwrap();
        let mut iter = stmts.body().iter();
        if let Some(first) = iter.next() {
            if iter.next().is_none() {
                let loc = first.location();
                return (loc.start_offset(), loc.end_offset());
            }
        }
    }
    let loc = body.location();
    (loc.start_offset(), loc.end_offset())
}

fn with_inner<F, R>(body: &Node, f: F) -> R
where
    F: FnOnce(&Node) -> R,
{
    if let Node::StatementsNode { .. } = body {
        let stmts = body.as_statements_node().unwrap();
        let mut iter = stmts.body().iter();
        if let Some(first) = iter.next() {
            if iter.next().is_none() {
                return f(&first);
            }
        }
    }
    f(body)
}

fn is_compound(body: &Node) -> bool {
    if let Node::StatementsNode { .. } = body {
        body.as_statements_node().unwrap().body().iter().count() > 1
    } else {
        false
    }
}

fn is_single_statement(body: &Node) -> bool {
    if let Node::StatementsNode { .. } = body {
        body.as_statements_node().unwrap().body().iter().count() <= 1
    } else {
        true
    }
}

fn is_unary_op(call: &ruby_prism::CallNode) -> bool {
    let m = String::from_utf8_lossy(call.name().as_slice());
    call.receiver().is_some() && call.arguments().is_none() && matches!(m.as_ref(), "!" | "~" | "-@" | "+@")
}

fn is_unary_op_node(node: &Node) -> bool {
    matches!(node, Node::CallNode{..}) && is_unary_op(&node.as_call_node().unwrap())
}

fn is_comparison_op(m: &str) -> bool {
    matches!(m, "==" | "===" | "!=" | ">" | ">=" | "<" | "<=" | "<=>" | "=~" | "!~")
}

fn is_operator_method(m: &str) -> bool {
    matches!(m, "+" | "-" | "*" | "/" | "%" | "**" | "&" | "|" | "^" | "<<" | ">>"
        | "==" | "===" | "!=" | ">" | ">=" | "<" | "<=" | "<=>" | "=~" | "!~" | "[]" | "[]=")
}

fn is_assignment(node: &Node) -> bool {
    matches!(node, Node::LocalVariableWriteNode{..} | Node::InstanceVariableWriteNode{..}
        | Node::ClassVariableWriteNode{..} | Node::GlobalVariableWriteNode{..}
        | Node::ConstantWriteNode{..} | Node::ConstantPathWriteNode{..} | Node::MultiWriteNode{..})
}

fn is_kw_logical_op(op: &str) -> bool { op == "and" || op == "or" }
fn is_logical_op(op: &str) -> bool { matches!(op, "&&" | "||" | "and" | "or") }
fn is_arith_op(op: &str) -> bool { matches!(op, "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^") }

impl<'a> Visitor<'a> {
    fn push(&mut self, ctx: PC) { self.parent_stack.push(ctx); }
    fn pop(&mut self) { self.parent_stack.pop(); }
    fn parent(&self) -> Option<&PC> { self.parent_stack.last() }
    fn with_ctx(&mut self, kind: PK, node: &Node) {
        self.push(PC::new(kind)); self.vn(node); self.pop();
    }
    fn has_ancestor(&self, kind: PK) -> bool {
        self.parent_stack.iter().any(|p| p.kind == kind)
    }

    fn src(&self, s: usize, e: usize) -> &str { &self.ctx.source[s..e] }

    fn line_of(&self, offset: usize) -> usize {
        1 + self.ctx.source.as_bytes()[..offset].iter().filter(|&&b| b == b'\n').count()
    }

    fn paren_offense_location(&self, open: usize, close: usize) -> crate::offense::Location {
        let loc = crate::offense::Location::from_offsets(self.ctx.source, open, close);
        if loc.line == loc.last_line {
            return loc;
        }
        let bytes = self.ctx.source.as_bytes();
        let mut end_of_first_line = open;
        while end_of_first_line < bytes.len() && bytes[end_of_first_line] != b'\n' {
            end_of_first_line += 1;
        }
        let mut col = 0u32;
        for (idx, ch) in self.ctx.source.char_indices() {
            if idx >= end_of_first_line { break; }
            if ch == '\n' { col = 0; } else { col += 1; }
        }
        crate::offense::Location::new(loc.line, loc.column, loc.line, col)
    }

    fn classify(&self, node: &Node) -> &'static str {
        match node {
            Node::IntegerNode{..} | Node::FloatNode{..} | Node::RationalNode{..}
            | Node::ImaginaryNode{..} | Node::StringNode{..} | Node::SymbolNode{..}
            | Node::RegularExpressionNode{..} | Node::NilNode{..} | Node::TrueNode{..}
            | Node::FalseNode{..} | Node::ArrayNode{..} | Node::HashNode{..}
            | Node::RangeNode{..} | Node::InterpolatedStringNode{..}
            | Node::InterpolatedSymbolNode{..} | Node::InterpolatedRegularExpressionNode{..}
            | Node::XStringNode{..} | Node::InterpolatedXStringNode{..} => "a literal",

            Node::SourceFileNode{..} | Node::SourceLineNode{..} | Node::SourceEncodingNode{..}
            | Node::SelfNode{..} | Node::RedoNode{..} | Node::RetryNode{..}
            | Node::ReturnNode{..} | Node::BreakNode{..} | Node::NextNode{..}
            | Node::YieldNode{..} | Node::SuperNode{..} | Node::ForwardingSuperNode{..}
            | Node::DefinedNode{..} => "a keyword",

            Node::LocalVariableReadNode{..} | Node::InstanceVariableReadNode{..}
            | Node::ClassVariableReadNode{..} | Node::GlobalVariableReadNode{..}
            | Node::NumberedReferenceReadNode{..} | Node::BackReferenceReadNode{..} => "a variable",

            Node::ConstantReadNode{..} | Node::ConstantPathNode{..} => "a constant",

            Node::LocalVariableWriteNode{..} | Node::InstanceVariableWriteNode{..}
            | Node::ClassVariableWriteNode{..} | Node::GlobalVariableWriteNode{..}
            | Node::ConstantWriteNode{..} | Node::ConstantPathWriteNode{..}
            | Node::MultiWriteNode{..} => "an assignment",

            Node::AndNode{..} | Node::OrNode{..} => "a logical expression",
            Node::MatchPredicateNode{..} | Node::MatchRequiredNode{..} => "a one-line pattern matching",
            Node::RescueModifierNode{..} => "a one-line rescue",
            Node::LambdaNode{..} => "an expression",

            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                if is_comparison_op(&m) { return "a comparison expression"; }
                if is_unary_op(&call) { return "a unary operation"; }
                if call.receiver().is_none()
                    && (m == "lambda" || m == "proc")
                    && call.block().is_some()
                { return "an expression"; }
                "a method call"
            }

            Node::ParenthesesNode{..} => {
                let pn = node.as_parentheses_node().unwrap();
                if let Some(b) = pn.body() {
                    with_inner(&b, |inner| self.classify(inner))
                } else { "a method call" }
            }

            Node::StatementsNode{..} => {
                let stmts = node.as_statements_node().unwrap();
                let mut iter = stmts.body().iter();
                if let Some(first) = iter.next() {
                    self.classify(&first)
                } else { "a method call" }
            }

            _ => "a method call",
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn should_skip(&self, inner: &Node, body: &Node, pk: PK, parent: &Option<PC>, open: usize, close: usize) -> bool {
        if pk == PK::PinExpression {
            match inner {
                Node::LocalVariableReadNode{..} | Node::InstanceVariableReadNode{..}
                | Node::ClassVariableReadNode{..} | Node::GlobalVariableReadNode{..} => {}
                _ => return true,
            }
        }

        if pk == PK::FlowControl || pk == PK::KeywordCall {
            if self.line_of(open) != self.line_of(close - 1) { return true; }
        }

        if pk == PK::Condition && open > 0 {
            let before = self.ctx.source.as_bytes()[open - 1];
            if before != b' ' && before != b'\t' && before != b'\n' { return true; }
        }

        if self.touches_keyword(open, close) { return true; }
        if pk == PK::RangeOperand { return true; }
        if matches!(inner, Node::RangeNode{..}) && pk != PK::CallArgs { return true; }
        if pk == PK::ExponentBase && self.is_neg_num(inner) { return true; }

        if pk == PK::MethodCallUnparen {
            if let Some(p) = parent {
                if !p.call_has_parens {
                    if p.is_first_arg && self.starts_with_hash(inner) { return true; }
                    if p.arg_count == 1 { return true; }
                }
            }
        }

        if pk == PK::Splat { return true; }

        if pk == PK::CallArgs {
            if let Some(p) = parent {
                if p.call_has_parens {
                    if self.is_modifier_cond(inner) { return true; }
                    if self.is_unparen_call(inner) { return true; }
                    if matches!(inner, Node::RescueModifierNode{..}) { return true; }
                    if is_compound(body) { return true; }
                }
            }
        }

        if pk == PK::Condition && is_assignment(inner) { return true; }

        if self.is_kw_with_bare_args(inner) {
            if let Some(p) = parent {
                if p.kind == PK::BinaryOp || p.kind == PK::CallReceiver { return true; }
            }
            return true;
        }

        if is_unary_op_node(inner) {
            if let Some(p) = parent {
                if p.kind == PK::CallReceiver { return true; }
            }
        }

        if pk == PK::UnaryOp {
            if let Some(p) = parent {
                if p.operator.as_deref() == Some("-@") || p.operator.as_deref() == Some("+@") {
                    if let Node::CallNode{..} = inner {
                        if inner.as_call_node().unwrap().receiver().is_some() { return true; }
                    }
                }
            }
        }

        if pk == PK::BinaryOp {
            if let Some(p) = parent {
                if self.logical_parens_needed(inner, p) { return true; }
                let op = p.operator.as_deref().unwrap_or("");
                if is_arith_op(op) && matches!(inner, Node::AndNode{..} | Node::OrNode{..}) { return true; }
                if is_logical_op(op) {
                    if let Node::CallNode{..} = inner {
                        let m = String::from_utf8_lossy(inner.as_call_node().unwrap().name().as_slice());
                        if is_comparison_op(&m) { return true; }
                    }
                }
                if op == "=~" && matches!(inner, Node::AndNode{..} | Node::OrNode{..}) { return true; }
                if self.is_kw_logical(inner) && is_kw_logical_op(op) { return true; }
                if is_comparison_op(op) {
                    if let Node::CallNode{..} = inner {
                        let m = String::from_utf8_lossy(inner.as_call_node().unwrap().name().as_slice());
                        if is_arith_op(&m) { return true; }
                    }
                }
            }
            if self.is_unparen_call(inner) { return true; }
            if self.is_unary_with_unparen(inner) { return true; }
            if is_compound(body) { return true; }
        }

        if pk == PK::CallReceiver {
            match inner {
                Node::AndNode{..} | Node::OrNode{..} => return true,
                Node::CallNode{..} => {
                    let call = inner.as_call_node().unwrap();
                    let m = String::from_utf8_lossy(call.name().as_slice());
                    if is_comparison_op(&m) || m == "&" { return true; }
                    if self.has_do_end(inner) { return true; }
                }
                _ => {}
            }
            if self.starts_with_hash(inner) && self.has_ancestor(PK::MethodCallUnparen) { return true; }
        }

        if pk == PK::Ternary {
            if matches!(inner, Node::AndNode{..} | Node::OrNode{..}
                | Node::CaseNode{..} | Node::RescueModifierNode{..}) { return true; }
        }

        if matches!(pk, PK::Assignment | PK::OpAssignment) {
            if self.is_kw_logical(inner) || is_compound(body) { return true; }
        }

        if self.has_do_end_chain(inner) {
            if matches!(pk, PK::MethodCallUnparen | PK::Hash | PK::Assignment | PK::Other) { return true; }
        }
        if self.is_lambda_proc_do_end(inner) { return true; }
        if pk == PK::MethodCallUnparen && self.has_do_end(inner) { return true; }

        if matches!(inner, Node::RescueModifierNode{..})
            && matches!(pk, PK::Condition | PK::Array | PK::Hash
                | PK::TernaryCondition | PK::Ternary | PK::CaseCondition) { return true; }

        if matches!(inner, Node::MatchPredicateNode{..} | Node::MatchRequiredNode{..})
            && matches!(pk, PK::CallArgs | PK::BinaryOp | PK::Assignment | PK::OpAssignment
                | PK::EndlessMethodBody) { return true; }

        if self.ternary_parentheses_required && pk == PK::TernaryCondition { return true; }

        if self.allow_in_multiline_conditions {
            if matches!(inner, Node::AndNode{..} | Node::OrNode{..}) {
                if self.line_of(open) != self.line_of(close - 1) { return true; }
            }
        }

        false
    }

    fn determine_inner_type(&self, body: &Node, pk: PK, parent: &Option<PC>) -> &'static str {
        with_inner(body, |inner| {
            let base_type = self.classify(inner);
            if base_type == "a keyword" { return "a keyword"; }
            if base_type == "a literal" {
                let is_single_range = matches!(inner, Node::RangeNode{..}) && !is_compound(body);
                if !is_single_range { return "a literal"; }
            }
            if base_type == "a variable" { return "a variable"; }
            if base_type == "a constant" { return "a constant"; }
            if pk == PK::BlockBody { return "block body"; }
            if pk == PK::DefBody { return base_type; }
            if base_type == "an assignment" { return "an assignment"; }
            if base_type == "an expression" { return "an expression"; }
            if base_type == "a one-line pattern matching" { return "a one-line pattern matching"; }
            if pk == PK::Interpolation { return "an interpolated expression"; }
            if pk == PK::CallArgs {
                if let Some(p) = parent {
                    if p.call_has_parens { return "a method argument"; }
                }
            }
            if base_type == "a one-line rescue" { return "a one-line rescue"; }
            base_type
        })
    }

    fn check_parens(&mut self, node: &ruby_prism::ParenthesesNode) {
        let body = match node.body() {
            Some(b) => b,
            None => return,
        };

        let open = node.location().start_offset();
        let close = node.location().end_offset();
        let pk = self.parent().map(|p| p.kind).unwrap_or(PK::Program);
        let parent = self.parent().cloned();

        let should_skip = with_inner(&body, |inner| self.should_skip(inner, &body, pk, &parent, open, close));
        if should_skip { return; }

        let should_skip_nested = with_inner(&body, |inner| {
            if let Node::ParenthesesNode{..} = inner {
                let inner_pn = inner.as_parentheses_node().unwrap();
                if let Some(inner_body) = inner_pn.body() {
                    let is_range = with_inner(&inner_body, |deep| matches!(deep, Node::RangeNode{..}));
                    if is_range { return false; }
                }
                return true;
            }
            false
        });
        if should_skip_nested { return; }

        let should_skip_call = with_inner(&body, |inner| {
            if let Node::CallNode{..} = inner {
                let call = inner.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                if is_operator_method(&m) && !is_unary_op(&call) && call.call_operator_loc().is_none() {
                    if matches!(pk, PK::BinaryOp | PK::ExponentBase | PK::ExponentPower) { return true; }
                }
            }
            false
        });
        if should_skip_call { return; }

        let inner_type = self.determine_inner_type(&body, pk, &parent);
        let msg = format!("Don't use parentheses around {}.", inner_type);
        let loc = self.paren_offense_location(open, close);
        let mut offense = Offense::new(COP_NAME, &msg, Severity::Convention, loc, self.ctx.filename);

        let (inner_start, inner_end) = get_inner_offsets(&body);
        let inner_text = self.src(inner_start, inner_end);
        let is_heredoc = inner_start + 2 <= self.ctx.source.len()
            && &self.ctx.source[inner_start..inner_start + 2] == "<<";

        if is_heredoc {
            let src = self.ctx.source.as_bytes();
            let mut edits = Vec::new();
            edits.push(Edit { start_offset: open, end_offset: inner_start, replacement: String::new() });

            let close_paren = close - 1;
            let mut term_end = close_paren;
            while term_end > inner_end && src[term_end - 1] != b'\n' {
                term_end -= 1;
            }

            let mut line_end = close;
            while line_end < src.len() && src[line_end] != b'\n' { line_end += 1; }
            if line_end < src.len() { line_end += 1; }

            let trailing = &self.ctx.source[close..line_end];
            if trailing.contains(',') {
                edits.push(Edit { start_offset: inner_end, end_offset: inner_end, replacement: ",".to_string() });
                edits.push(Edit { start_offset: term_end, end_offset: line_end, replacement: String::new() });
            } else {
                let mut paren_end = close;
                if paren_end < src.len() && src[paren_end] == b'\n' { paren_end += 1; }
                edits.push(Edit { start_offset: term_end, end_offset: paren_end, replacement: String::new() });
            }
            offense = offense.with_correction(Correction { edits });
        } else {
            let replacement = if close < self.ctx.source.len()
                && self.ctx.source.as_bytes()[close] == b'?'
            {
                format!("{} ", inner_text)
            } else {
                inner_text.to_string()
            };
            offense = offense.with_correction(Correction::replace(open, close, &replacement));
        }

        self.offenses.push(offense);
    }

    fn is_neg_num(&self, node: &Node) -> bool {
        match node {
            Node::IntegerNode{..} | Node::FloatNode{..} => {
                let loc = node.location();
                self.src(loc.start_offset(), loc.end_offset()).starts_with('-')
            }
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                m == "-@" && call.receiver().map_or(false, |r| matches!(r, Node::IntegerNode{..} | Node::FloatNode{..}))
            }
            _ => false,
        }
    }

    fn starts_with_hash(&self, node: &Node) -> bool {
        match node {
            Node::HashNode{..} => true,
            Node::CallNode{..} => {
                node.as_call_node().unwrap().receiver().map(|r| self.starts_with_hash(&r)).unwrap_or(false)
            }
            _ => false,
        }
    }

    fn touches_keyword(&self, open: usize, close: usize) -> bool {
        if open > 0 {
            let b = self.ctx.source.as_bytes()[open - 1];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'?' { return true; }
        }
        if close < self.ctx.source.len() {
            let a = self.ctx.source.as_bytes()[close];
            if a.is_ascii_alphanumeric() || a == b'_' { return true; }
        }
        false
    }

    fn is_modifier_cond(&self, node: &Node) -> bool {
        let (prefix, loc) = match node {
            Node::IfNode{..} => ("if", node.as_if_node().unwrap().location()),
            Node::UnlessNode{..} => ("unless", node.as_unless_node().unwrap().location()),
            Node::WhileNode{..} => ("while", node.as_while_node().unwrap().location()),
            Node::UntilNode{..} => ("until", node.as_until_node().unwrap().location()),
            _ => return false,
        };
        !self.src(loc.start_offset(), loc.end_offset()).starts_with(prefix)
    }

    fn is_unparen_call(&self, node: &Node) -> bool {
        match node {
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                if is_unary_op(&call) || (is_operator_method(&m) && call.call_operator_loc().is_none()) { return false; }
                call.arguments().map_or(false, |a| a.arguments().iter().count() > 0) && call.opening_loc().is_none()
            }
            Node::SuperNode{..} | Node::YieldNode{..} | Node::DefinedNode{..} => self.has_bare_kw_args(node),
            _ => false,
        }
    }

    fn has_bare_kw_args(&self, node: &Node) -> bool {
        match node {
            Node::SuperNode{..} => { let s = node.as_super_node().unwrap(); s.arguments().is_some() && s.lparen_loc().is_none() }
            Node::YieldNode{..} => { let y = node.as_yield_node().unwrap(); y.arguments().is_some() && y.lparen_loc().is_none() }
            Node::DefinedNode{..} => node.as_defined_node().unwrap().lparen_loc().is_none(),
            _ => false,
        }
    }

    fn has_flow_kw_space(&self, args: Option<ruby_prism::ArgumentsNode>, kw_end: usize) -> bool {
        args.is_some() && self.ctx.source.as_bytes().get(kw_end).map(|&b| b == b' ').unwrap_or(false)
    }

    fn is_kw_with_bare_args(&self, node: &Node) -> bool {
        match node {
            Node::DefinedNode{..} | Node::SuperNode{..} | Node::YieldNode{..} => self.has_bare_kw_args(node),
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                m == "!" && self.src(call.location().start_offset(), call.location().end_offset()).starts_with("not ")
            }
            Node::AliasMethodNode{..} | Node::AliasGlobalVariableNode{..} => true,
            Node::WhileNode{..} => {
                let l = node.as_while_node().unwrap().location();
                !self.src(l.start_offset(), l.end_offset()).starts_with("while")
            }
            Node::UntilNode{..} => {
                let l = node.as_until_node().unwrap().location();
                !self.src(l.start_offset(), l.end_offset()).starts_with("until")
            }
            Node::ReturnNode{..} => { let r = node.as_return_node().unwrap(); self.has_flow_kw_space(r.arguments(), r.keyword_loc().end_offset()) }
            Node::BreakNode{..} => { let b = node.as_break_node().unwrap(); self.has_flow_kw_space(b.arguments(), b.keyword_loc().end_offset()) }
            Node::NextNode{..} => { let n = node.as_next_node().unwrap(); self.has_flow_kw_space(n.arguments(), n.keyword_loc().end_offset()) }
            _ => false,
        }
    }

    fn is_kw_logical(&self, node: &Node) -> bool {
        match node {
            Node::AndNode{..} => {
                let ol = node.as_and_node().unwrap().operator_loc();
                self.src(ol.start_offset(), ol.end_offset()) == "and"
            }
            Node::OrNode{..} => {
                let ol = node.as_or_node().unwrap().operator_loc();
                self.src(ol.start_offset(), ol.end_offset()) == "or"
            }
            _ => false,
        }
    }

    fn logical_parens_needed(&self, inner: &Node, parent: &PC) -> bool {
        let pop = parent.operator.as_deref().unwrap_or("");
        if !is_logical_op(pop) { return false; }
        let (iop, inner_is_and) = match inner {
            Node::AndNode{..} => {
                let ol = inner.as_and_node().unwrap().operator_loc();
                (self.src(ol.start_offset(), ol.end_offset()).to_string(), true)
            }
            Node::OrNode{..} => {
                let ol = inner.as_or_node().unwrap().operator_loc();
                (self.src(ol.start_offset(), ol.end_offset()).to_string(), false)
            }
            _ => return false,
        };
        if is_kw_logical_op(&iop) != is_kw_logical_op(pop) { return true; }
        let parent_is_and = pop == "&&" || pop == "and";
        inner_is_and != parent_is_and
    }

    fn has_do_end_block(call: &ruby_prism::CallNode, source: &str) -> bool {
        if let Some(block) = call.block() {
            if let Node::BlockNode{..} = block {
                let ol = block.as_block_node().unwrap().opening_loc();
                return &source[ol.start_offset()..ol.end_offset()] == "do";
            }
        }
        false
    }

    fn has_do_end_chain(&self, node: &Node) -> bool {
        if let Node::CallNode{..} = node {
            if let Some(recv) = node.as_call_node().unwrap().receiver() {
                return self.has_do_end(&recv);
            }
        }
        false
    }

    fn is_lambda_proc_do_end(&self, node: &Node) -> bool {
        if let Node::CallNode{..} = node {
            let call = node.as_call_node().unwrap();
            let m = String::from_utf8_lossy(call.name().as_slice());
            if (m == "lambda" || m == "proc") && call.receiver().is_none() {
                return Self::has_do_end_block(&call, self.ctx.source);
            }
        }
        false
    }

    fn has_do_end(&self, node: &Node) -> bool {
        if let Node::CallNode{..} = node {
            let call = node.as_call_node().unwrap();
            if Self::has_do_end_block(&call, self.ctx.source) { return true; }
            if let Some(recv) = call.receiver() {
                return self.has_do_end(&recv);
            }
        }
        false
    }

    fn is_unary_with_unparen(&self, node: &Node) -> bool {
        if let Node::CallNode{..} = node {
            let call = node.as_call_node().unwrap();
            let m = String::from_utf8_lossy(call.name().as_slice());
            if m == "!" || m == "~" {
                if let Some(recv) = call.receiver() {
                    return self.has_unparen_args(&recv);
                }
            }
        }
        false
    }

    fn has_unparen_args(&self, node: &Node) -> bool {
        match node {
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                call.arguments().map_or(false, |a| a.arguments().iter().count() > 0) && call.opening_loc().is_none()
            }
            Node::SuperNode{..} | Node::YieldNode{..} | Node::DefinedNode{..} => self.has_bare_kw_args(node),
            _ => false,
        }
    }

    fn visit_call_children(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        if let Some(recv) = node.receiver() {
            if is_operator_method(&method_name) {
                let mut ctx = PC::new(if method_name == "**" { PK::ExponentBase } else { PK::BinaryOp });
                ctx.operator = Some(method_name.clone());
                self.push(ctx);
                self.vn(&recv);
                self.pop();
            } else {
                self.push(PC::new(PK::CallReceiver));
                self.vn(&recv);
                self.pop();
            }
        }

        let has_parens = node.opening_loc().is_some();
        let arg_count = node.arguments().map(|a| a.arguments().iter().count()).unwrap_or(0);

        if let Some(args) = node.arguments() {
            for (idx, arg) in args.arguments().iter().enumerate() {
                if is_operator_method(&method_name) {
                    let mut ctx = PC::new(if method_name == "**" { PK::ExponentPower } else { PK::BinaryOp });
                    ctx.operator = Some(method_name.clone());
                    self.push(ctx);
                } else {
                    let mut ctx = PC::new(if has_parens { PK::CallArgs } else { PK::MethodCallUnparen });
                    ctx.call_has_parens = has_parens;
                    ctx.arg_count = arg_count;
                    ctx.is_first_arg = idx == 0;
                    self.push(ctx);
                }
                self.vn(&arg);
                self.pop();
            }
        }

        if let Some(block) = node.block() { self.vn(&block); }
    }

    fn vn(&mut self, node: &Node) {
        match node {
            Node::ParenthesesNode{..} => {
                let pn = node.as_parentheses_node().unwrap();
                self.check_parens(&pn);
                if let Some(body) = pn.body() {
                    self.with_ctx(PK::Statements, &body);
                }
            }
            Node::CallNode{..} => {
                let cn = node.as_call_node().unwrap();
                if is_unary_op(&cn) {
                    if let Some(recv) = cn.receiver() {
                        let mut ctx = PC::new(PK::UnaryOp);
                        ctx.operator = Some(String::from_utf8_lossy(cn.name().as_slice()).to_string());
                        self.push(ctx); self.vn(&recv); self.pop();
                    }
                } else {
                    self.visit_call_children(&cn);
                }
            }
            Node::IfNode{..} => {
                let n = node.as_if_node().unwrap();
                let l = n.location();
                let s = self.src(l.start_offset(), l.end_offset());
                let ternary = !s.starts_with("if") && !s.starts_with("elsif");
                self.push(PC::new(if ternary { PK::TernaryCondition } else { PK::Condition }));
                self.vn(&n.predicate()); self.pop();
                let bk = if ternary { PK::Ternary } else { PK::IfBody };
                if let Some(body) = n.statements() {
                    self.push(PC::new(bk));
                    for stmt in body.body().iter() { self.vn(&stmt); }
                    self.pop();
                }
                if let Some(sub) = n.subsequent() {
                    self.push(PC::new(bk)); self.vn(&sub); self.pop();
                }
            }
            Node::UnlessNode{..} => {
                let n = node.as_unless_node().unwrap();
                self.push(PC::new(PK::Condition)); self.vn(&n.predicate()); self.pop();
                if let Some(body) = n.statements() {
                    self.push(PC::new(PK::IfBody));
                    for stmt in body.body().iter() { self.vn(&stmt); }
                    self.pop();
                }
                if let Some(ec) = n.else_clause() {
                    self.push(PC::new(PK::IfBody));
                    ruby_prism::visit_else_node(self, &ec);
                    self.pop();
                }
            }
            Node::WhileNode{..} | Node::UntilNode{..} => self.visit_loop_node(node),
            Node::CaseNode{..} | Node::CaseMatchNode{..} => self.visit_case_like_node(node),
            Node::DefNode{..} => {
                let n = node.as_def_node().unwrap();
                let endless = n.end_keyword_loc().is_none() && n.equal_loc().is_some();
                if let Some(params) = n.parameters() {
                    self.push(PC::new(PK::Other));
                    ruby_prism::visit_parameters_node(self, &params);
                    self.pop();
                }
                if let Some(body) = n.body() {
                    self.push(PC::new(if endless { PK::EndlessMethodBody } else { PK::DefBody }));
                    self.vn(&body); self.pop();
                }
            }
            Node::AndNode{..} | Node::OrNode{..} => self.visit_logical_node(node),
            Node::LocalVariableWriteNode{..} => self.with_ctx(PK::Assignment, &node.as_local_variable_write_node().unwrap().value()),
            Node::InstanceVariableWriteNode{..} => self.with_ctx(PK::Assignment, &node.as_instance_variable_write_node().unwrap().value()),
            Node::ClassVariableWriteNode{..} => self.with_ctx(PK::Assignment, &node.as_class_variable_write_node().unwrap().value()),
            Node::GlobalVariableWriteNode{..} => self.with_ctx(PK::Assignment, &node.as_global_variable_write_node().unwrap().value()),
            Node::ConstantWriteNode{..} => self.with_ctx(PK::Assignment, &node.as_constant_write_node().unwrap().value()),
            Node::LocalVariableOperatorWriteNode{..} => self.with_ctx(PK::OpAssignment, &node.as_local_variable_operator_write_node().unwrap().value()),
            Node::LocalVariableOrWriteNode{..} => self.with_ctx(PK::OpAssignment, &node.as_local_variable_or_write_node().unwrap().value()),
            Node::LocalVariableAndWriteNode{..} => self.with_ctx(PK::OpAssignment, &node.as_local_variable_and_write_node().unwrap().value()),
            Node::ReturnNode{..} | Node::BreakNode{..} | Node::NextNode{..} => self.visit_flow_control_node(node),
            Node::SuperNode{..} | Node::YieldNode{..} => self.visit_keyword_call_node(node),
            Node::ForwardingSuperNode{..} => {
                if let Some(block) = node.as_forwarding_super_node().unwrap().block() {
                    let bn = block;
                    if let Some(params) = bn.parameters() { self.with_ctx(PK::Other, &params); }
                    self.visit_block_body_(bn.body());
                }
            }
            Node::BlockNode{..} => {
                let bn = node.as_block_node().unwrap();
                if let Some(params) = bn.parameters() { self.with_ctx(PK::Other, &params); }
                self.visit_block_body_(bn.body());
            }
            Node::LambdaNode{..} => self.visit_block_body_(node.as_lambda_node().unwrap().body()),
            Node::EmbeddedStatementsNode{..} => self.visit_embedded_stmts(&node.as_embedded_statements_node().unwrap()),
            Node::SplatNode{..} => {
                if let Some(expr) = node.as_splat_node().unwrap().expression() {
                    self.with_ctx(PK::Splat, &expr);
                }
            }
            Node::KeywordHashNode{..} => {
                for elem in node.as_keyword_hash_node().unwrap().elements().iter() {
                    self.with_ctx(PK::Hash, &elem);
                }
            }
            Node::RangeNode{..} => {
                let rn = node.as_range_node().unwrap();
                if let Some(l) = rn.left() { self.with_ctx(PK::RangeOperand, &l); }
                if let Some(r) = rn.right() { self.with_ctx(PK::RangeOperand, &r); }
            }
            Node::ArrayNode{..} => {
                for elem in node.as_array_node().unwrap().elements().iter() {
                    self.with_ctx(PK::Array, &elem);
                }
            }
            Node::HashNode{..} => {
                for elem in node.as_hash_node().unwrap().elements().iter() {
                    self.with_ctx(PK::Hash, &elem);
                }
            }
            Node::StatementsNode{..} => {
                for stmt in node.as_statements_node().unwrap().body().iter() { self.vn(&stmt); }
            }
            Node::PinnedExpressionNode{..} => self.visit_pinned_expr(&node.as_pinned_expression_node().unwrap()),
            Node::AssocNode{..} => {
                let an = node.as_assoc_node().unwrap();
                self.with_ctx(PK::Hash, &an.value());
                self.with_ctx(PK::Other, &an.key());
            }
            Node::AssocSplatNode{..} => {
                if let Some(v) = node.as_assoc_splat_node().unwrap().value() {
                    self.with_ctx(PK::Splat, &v);
                }
            }
            Node::InterpolatedStringNode{..} | Node::InterpolatedSymbolNode{..}
            | Node::InterpolatedRegularExpressionNode{..} => self.visit_interpolated_parts(node),
            Node::ElseNode{..} => ruby_prism::visit_else_node(self, &node.as_else_node().unwrap()),
            _ => {
                self.push(PC::new(PK::Other));
                match node {
                    Node::ClassNode{..} => ruby_prism::visit_class_node(self, &node.as_class_node().unwrap()),
                    Node::ModuleNode{..} => ruby_prism::visit_module_node(self, &node.as_module_node().unwrap()),
                    Node::SingletonClassNode{..} => ruby_prism::visit_singleton_class_node(self, &node.as_singleton_class_node().unwrap()),
                    Node::BeginNode{..} => ruby_prism::visit_begin_node(self, &node.as_begin_node().unwrap()),
                    Node::RescueNode{..} => ruby_prism::visit_rescue_node(self, &node.as_rescue_node().unwrap()),
                    Node::EnsureNode{..} => ruby_prism::visit_ensure_node(self, &node.as_ensure_node().unwrap()),
                    Node::ForNode{..} => ruby_prism::visit_for_node(self, &node.as_for_node().unwrap()),
                    Node::WhenNode{..} => ruby_prism::visit_when_node(self, &node.as_when_node().unwrap()),
                    Node::MultiWriteNode{..} => ruby_prism::visit_multi_write_node(self, &node.as_multi_write_node().unwrap()),
                    Node::ConstantPathWriteNode{..} => ruby_prism::visit_constant_path_write_node(self, &node.as_constant_path_write_node().unwrap()),
                    Node::MatchPredicateNode{..} => ruby_prism::visit_match_predicate_node(self, &node.as_match_predicate_node().unwrap()),
                    Node::MatchRequiredNode{..} => ruby_prism::visit_match_required_node(self, &node.as_match_required_node().unwrap()),
                    Node::HashPatternNode{..} => ruby_prism::visit_hash_pattern_node(self, &node.as_hash_pattern_node().unwrap()),
                    Node::ArrayPatternNode{..} => ruby_prism::visit_array_pattern_node(self, &node.as_array_pattern_node().unwrap()),
                    Node::FindPatternNode{..} => ruby_prism::visit_find_pattern_node(self, &node.as_find_pattern_node().unwrap()),
                    Node::CapturePatternNode{..} => ruby_prism::visit_capture_pattern_node(self, &node.as_capture_pattern_node().unwrap()),
                    Node::InNode{..} => ruby_prism::visit_in_node(self, &node.as_in_node().unwrap()),
                    _ => {}
                }
                self.pop();
            }
        }
    }

    fn visit_flow_control_node(&mut self, node: &Node) {
        let args = match node {
            Node::ReturnNode{..} => node.as_return_node().unwrap().arguments(),
            Node::BreakNode{..} => node.as_break_node().unwrap().arguments(),
            Node::NextNode{..} => node.as_next_node().unwrap().arguments(),
            _ => return,
        };
        if let Some(args) = args {
            for arg in args.arguments().iter() {
                self.with_ctx(PK::FlowControl, &arg);
            }
        }
    }

    fn visit_keyword_call_node(&mut self, node: &Node) {
        let (hp, args, block) = match node {
            Node::SuperNode{..} => {
                let s = node.as_super_node().unwrap();
                (s.lparen_loc().is_some(), s.arguments(), s.block())
            }
            Node::YieldNode{..} => {
                let y = node.as_yield_node().unwrap();
                (y.lparen_loc().is_some(), y.arguments(), None)
            }
            _ => return,
        };
        if let Some(args) = args {
            let ac = args.arguments().iter().count();
            for arg in args.arguments().iter() {
                let mut c = PC::new(if hp { PK::CallArgs } else { PK::KeywordCall });
                c.call_has_parens = hp; c.arg_count = ac;
                self.push(c); self.vn(&arg); self.pop();
            }
        }
        if let Some(block) = block { self.vn(&block); }
    }

    fn visit_block_body_(&mut self, body: Option<Node>) {
        if let Some(body) = body {
            let pk = if is_single_statement(&body) { PK::BlockBody } else { PK::Statements };
            self.push(PC::new(pk)); self.vn(&body); self.pop();
        }
    }

    fn visit_logical_node(&mut self, node: &Node) {
        let (ol, left, right) = match node {
            Node::AndNode{..} => {
                let n = node.as_and_node().unwrap();
                (n.operator_loc(), n.left(), n.right())
            }
            Node::OrNode{..} => {
                let n = node.as_or_node().unwrap();
                (n.operator_loc(), n.left(), n.right())
            }
            _ => return,
        };
        let op = self.src(ol.start_offset(), ol.end_offset()).to_string();
        let mut c = PC::new(PK::BinaryOp); c.operator = Some(op.clone());
        self.push(c); self.vn(&left); self.pop();
        let mut c = PC::new(PK::BinaryOp); c.operator = Some(op);
        self.push(c); self.vn(&right); self.pop();
    }

    fn visit_case_like_node(&mut self, node: &Node) {
        let (predicate, conditions, else_clause): (Option<Node>, Box<dyn Iterator<Item = Node>>, _) = match node {
            Node::CaseNode{..} => {
                let c = node.as_case_node().unwrap();
                (c.predicate(), Box::new(c.conditions().iter()), c.else_clause())
            }
            Node::CaseMatchNode{..} => {
                let c = node.as_case_match_node().unwrap();
                (c.predicate(), Box::new(c.conditions().iter()), c.else_clause())
            }
            _ => return,
        };
        if let Some(pred) = predicate {
            self.with_ctx(PK::CaseCondition, &pred);
        }
        for c in conditions {
            self.with_ctx(PK::Other, &c);
        }
        if let Some(ec) = else_clause {
            self.push(PC::new(PK::IfBody));
            ruby_prism::visit_else_node(self, &ec);
            self.pop();
        }
    }

    fn visit_loop_node(&mut self, node: &Node) {
        let (predicate, statements) = match node {
            Node::WhileNode{..} => {
                let w = node.as_while_node().unwrap();
                (w.predicate(), w.statements())
            }
            Node::UntilNode{..} => {
                let u = node.as_until_node().unwrap();
                (u.predicate(), u.statements())
            }
            _ => return,
        };
        self.push(PC::new(PK::Condition));
        self.vn(&predicate);
        self.pop();
        if let Some(body) = statements {
            self.push(PC::new(PK::Statements));
            for stmt in body.body().iter() { self.vn(&stmt); }
            self.pop();
        }
    }

    fn visit_interpolated_parts(&mut self, node: &Node) {
        let parts: Box<dyn Iterator<Item = _>> = match node {
            Node::InterpolatedStringNode{..} => Box::new(node.as_interpolated_string_node().unwrap().parts().iter()),
            Node::InterpolatedSymbolNode{..} => Box::new(node.as_interpolated_symbol_node().unwrap().parts().iter()),
            Node::InterpolatedRegularExpressionNode{..} => Box::new(node.as_interpolated_regular_expression_node().unwrap().parts().iter()),
            _ => return,
        };
        for part in parts {
            if let Node::EmbeddedStatementsNode{..} = part {
                self.visit_embedded_stmts(&part.as_embedded_statements_node().unwrap());
            }
        }
    }

    fn visit_embedded_stmts(&mut self, node: &ruby_prism::EmbeddedStatementsNode) {
        if let Some(stmts) = node.statements() {
            self.push(PC::new(PK::Interpolation));
            for stmt in stmts.body().iter() { self.vn(&stmt); }
            self.pop();
        }
    }

    fn visit_pinned_expr(&mut self, node: &ruby_prism::PinnedExpressionNode) {
        let expr = node.expression();
        if matches!(expr, Node::LocalVariableReadNode{..} | Node::InstanceVariableReadNode{..}
            | Node::ClassVariableReadNode{..} | Node::GlobalVariableReadNode{..}) {
            let lp = node.lparen_loc();
            let rp = node.rparen_loc();
            let (ps, pe) = (lp.start_offset(), rp.end_offset());
            let inner_src = self.src(lp.end_offset(), rp.start_offset());
            let loc = crate::offense::Location::from_offsets(self.ctx.source, ps, pe);
            let offense = Offense::new(COP_NAME, "Don't use parentheses around a variable.", Severity::Convention, loc, self.ctx.filename)
                .with_correction(Correction::replace(ps, pe, inner_src));
            self.offenses.push(offense);
        }
        self.with_ctx(PK::PinExpression, &expr);
    }
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        self.push(PC::new(PK::Program));
        ruby_prism::visit_program_node(self, node);
        self.pop();
    }
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode) {
        for stmt in node.body().iter() { self.vn(&stmt); }
    }
    fn visit_pinned_expression_node(&mut self, node: &ruby_prism::PinnedExpressionNode) {
        self.visit_pinned_expr(node);
    }
}
