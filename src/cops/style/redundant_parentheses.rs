//! Style/RedundantParentheses - Checks for redundant parentheses.
//!
//! Detects cases where parentheses wrap an expression but are not needed.
//! For example, `(x)` can just be `x`, `(1 + 2)` can be `1 + 2`, etc.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_parentheses.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantParentheses";

pub struct RedundantParentheses {
    /// If Style/TernaryParentheses is enabled with require_parentheses or
    /// require_parentheses_when_complex, allow parens around ternary conditions.
    ternary_parentheses_required: bool,
    /// If Style/ParenthesesAroundCondition AllowInMultilineConditions is true,
    /// allow parens around multiline logical expressions.
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
    Return,
    Break,
    Next,
    Super,
    Yield,
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

// Helper: get inner offsets from body (unwrap single StatementsNode)
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

// Helper: apply function to the unwrapped inner node
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

// Helper: check if body is compound (multiple statements)
fn is_compound(body: &Node) -> bool {
    if let Node::StatementsNode { .. } = body {
        let stmts = body.as_statements_node().unwrap();
        stmts.body().iter().count() > 1
    } else {
        false
    }
}

// Helper: check if body is a single statement (or not a StatementsNode)
fn is_single_statement(body: &Node) -> bool {
    if let Node::StatementsNode { .. } = body {
        let stmts = body.as_statements_node().unwrap();
        stmts.body().iter().count() <= 1
    } else {
        true
    }
}

impl<'a> Visitor<'a> {
    fn push(&mut self, ctx: PC) { self.parent_stack.push(ctx); }
    fn pop(&mut self) { self.parent_stack.pop(); }
    fn parent(&self) -> Option<&PC> { self.parent_stack.last() }
    fn has_ancestor(&self, kind: PK) -> bool {
        self.parent_stack.iter().any(|p| p.kind == kind)
    }

    fn src(&self, s: usize, e: usize) -> &str { &self.ctx.source[s..e] }

    fn line_of(&self, offset: usize) -> usize {
        let mut line = 1;
        for (i, ch) in self.ctx.source.char_indices() {
            if i >= offset { break; }
            if ch == '\n' { line += 1; }
        }
        line
    }

    /// Compute offense location for parentheses. For multi-line parens,
    /// last_column is the end of the first line (matching RuboCop's expect_offense format
    /// where ^ marks only span the first line of multi-line offenses).
    fn paren_offense_location(&self, open: usize, close: usize) -> crate::offense::Location {
        let loc = crate::offense::Location::from_offsets(self.ctx.source, open, close);
        if loc.line == loc.last_line {
            loc
        } else {
            // Find end of first line from the opening paren
            let bytes = self.ctx.source.as_bytes();
            let mut end_of_first_line = open;
            while end_of_first_line < self.ctx.source.len() && bytes[end_of_first_line] != b'\n' {
                end_of_first_line += 1;
            }
            // Compute column at end_of_first_line by scanning from line start
            let mut col = 0u32;
            for (idx, ch) in self.ctx.source.char_indices() {
                if idx >= end_of_first_line { break; }
                if ch == '\n' { col = 0; } else { col += 1; }
            }
            crate::offense::Location::new(loc.line, loc.column, loc.line, col)
        }
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
            | Node::SelfNode{..} | Node::RedoNode{..} | Node::RetryNode{..} => "a keyword",

            Node::ReturnNode{..} | Node::BreakNode{..} | Node::NextNode{..}
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
                // RuboCop uses begin_node.children.first for classification
                let mut iter = stmts.body().iter();
                if let Some(first) = iter.next() {
                    self.classify(&first)
                } else { "a method call" }
            }

            _ => "a method call",
        }
    }

    fn check_parens(&mut self, node: &ruby_prism::ParenthesesNode) {
        let body = match node.body() {
            Some(b) => b,
            None => return,
        };

        let paren_loc = node.location();
        let open = paren_loc.start_offset();
        let close = paren_loc.end_offset();

        let pk = self.parent().map(|p| p.kind).unwrap_or(PK::Program);
        let parent = self.parent().cloned();

        // Use with_inner to check conditions on the unwrapped inner node
        let should_skip = with_inner(&body, |inner| {
            // 1. Pin operator
            if pk == PK::PinExpression {
                match inner {
                    Node::LocalVariableReadNode{..} | Node::InstanceVariableReadNode{..}
                    | Node::ClassVariableReadNode{..} | Node::GlobalVariableReadNode{..} => {}
                    _ => return true,
                }
            }

            // 2-3. Rescue/When
            if pk == PK::Other {
                // handled by touching keyword check below
            }

            // 4. Multiline control flow (only return/break/next)
            if matches!(pk, PK::Return | PK::Break | PK::Next) {
                if self.line_of(open) != self.line_of(close - 1) { return true; }
            }

            // 5. Post-while/until touching keyword
            if pk == PK::Condition && open > 0 {
                let before = self.ctx.source.as_bytes()[open - 1];
                if before != b' ' && before != b'\t' && before != b'\n' { return true; }
            }

            // 7. Touching keyword
            if self.touches_keyword(open, close) { return true; }

            // 8. Range operand
            if pk == PK::RangeOperand { return true; }

            // 9. Standalone range
            if matches!(inner, Node::RangeNode{..}) && pk != PK::CallArgs { return true; }

            // 10. Negative numeric in exponent base ((-2)**n, not n**(-2))
            if pk == PK::ExponentBase && self.is_neg_num(inner) { return true; }

            // 11. Hash as first arg of unparen call
            if pk == PK::MethodCallUnparen {
                if let Some(ref p) = parent {
                    if !p.call_has_parens && p.is_first_arg && self.starts_with_hash(inner) { return true; }
                }
            }

            // 12. Splat
            if pk == PK::Splat { return true; }

            // 13. Single arg unparen method call
            if pk == PK::MethodCallUnparen {
                if let Some(ref p) = parent {
                    if !p.call_has_parens && p.arg_count == 1 { return true; }
                }
            }

            // 15. Conditional/unparen call/rescue/compound as arg in paren call
            if pk == PK::CallArgs {
                if let Some(ref p) = parent {
                    if p.call_has_parens {
                        if self.is_modifier_cond(inner) { return true; }
                        if self.is_unparen_call(inner) { return true; }
                        if matches!(inner, Node::RescueModifierNode{..}) { return true; }
                        if is_compound(&body) { return true; }
                    }
                }
            }

            // 16. Assignment condition
            if pk == PK::Condition && is_assignment(inner) { return true; }

            // 17. Keyword with unparen args
            if self.is_kw_with_bare_args(inner) {
                if let Some(ref p) = parent {
                    if p.kind == PK::BinaryOp || p.kind == PK::CallReceiver { return true; }
                }
                return true;
            }

            // 18. Unary as receiver: (!x).y
            if is_unary_op_node(inner) {
                if let Some(ref p) = parent {
                    if p.kind == PK::CallReceiver { return true; }
                }
            }

            // 19. Unary +/- of method call
            if pk == PK::UnaryOp {
                if let Some(ref p) = parent {
                    if p.operator.as_deref() == Some("-@") || p.operator.as_deref() == Some("+@") {
                        if let Node::CallNode{..} = inner {
                            let call = inner.as_call_node().unwrap();
                            if call.receiver().is_some() { return true; }
                        }
                    }
                }
            }

            // 20-21. Logical/comparison/arithmetic precedence
            if pk == PK::BinaryOp {
                if let Some(ref p) = parent {
                    if self.logical_parens_needed(inner, p) { return true; }
                    let op = p.operator.as_deref().unwrap_or("");
                    if self.is_arith_op(op) {
                        if matches!(inner, Node::AndNode{..} | Node::OrNode{..}) { return true; }
                    }
                    if self.is_logical_op(op) {
                        if let Node::CallNode{..} = inner {
                            let call = inner.as_call_node().unwrap();
                            let m = String::from_utf8_lossy(call.name().as_slice());
                            if is_comparison_op(&m) { return true; }
                        }
                    }
                    if op == "=~" {
                        if matches!(inner, Node::AndNode{..} | Node::OrNode{..}) { return true; }
                    }
                }
            }

            // 22. Logical/comparison as receiver, do-end block as receiver
            if pk == PK::CallReceiver {
                match inner {
                    Node::AndNode{..} | Node::OrNode{..} => return true,
                    Node::CallNode{..} => {
                        let call = inner.as_call_node().unwrap();
                        let m = String::from_utf8_lossy(call.name().as_slice());
                        if is_comparison_op(&m) || m == "&" { return true; }
                        // do-end block in receiver position (removing parens would change parse)
                        if self.has_do_end(inner) { return true; }
                    }
                    _ => {}
                }
                // Hash at start of receiver in unparen method call arg context
                if self.starts_with_hash(inner) && self.has_ancestor(PK::MethodCallUnparen) { return true; }
            }

            // 23. Ternary branches
            if pk == PK::Ternary {
                if matches!(inner, Node::AndNode{..} | Node::OrNode{..}
                    | Node::CaseNode{..} | Node::RescueModifierNode{..}) { return true; }
            }

            // 24. or/and in assignment
            if matches!(pk, PK::Assignment | PK::OpAssignment) {
                if self.is_kw_logical(inner) { return true; }
                if is_compound(&body) { return true; }
            }

            // 25. Keyword logical in binary op with keyword logical parent
            if pk == PK::BinaryOp && self.is_kw_logical(inner) {
                if let Some(ref p) = parent {
                    if is_kw_logical_op(p.operator.as_deref().unwrap_or("")) { return true; }
                }
            }

            // 29. Super/yield multiline
            if matches!(pk, PK::Super | PK::Yield) {
                if self.line_of(open) != self.line_of(close - 1) { return true; }
            }

            // 30-31. do-end block in chain / lambda/proc do-end
            if self.has_do_end_chain(inner) {
                if matches!(pk, PK::MethodCallUnparen | PK::Hash | PK::Assignment | PK::Other) { return true; }
            }
            if self.is_lambda_proc_do_end(inner) { return true; }

            // 32. do-end in unparen method arg
            if pk == PK::MethodCallUnparen && self.has_do_end(inner) { return true; }

            // 33-34. Unparen call / unary with unparen call in binary op
            if pk == PK::BinaryOp {
                if self.is_unparen_call(inner) { return true; }
                if self.is_unary_with_unparen(inner) { return true; }
                if is_compound(&body) { return true; }
            }

            // 36-38. Rescue modifier in various contexts
            if matches!(inner, Node::RescueModifierNode{..}) {
                if matches!(pk, PK::Condition | PK::Array | PK::Hash
                    | PK::TernaryCondition | PK::Ternary | PK::CaseCondition) { return true; }
            }

            // 39-42. Pattern matching in various contexts
            if matches!(inner, Node::MatchPredicateNode{..} | Node::MatchRequiredNode{..}) {
                if matches!(pk, PK::CallArgs | PK::BinaryOp | PK::Assignment | PK::OpAssignment
                    | PK::EndlessMethodBody) { return true; }
            }

            // 46. Arithmetic in comparison
            if pk == PK::BinaryOp {
                if let Some(ref p) = parent {
                    if is_comparison_op(p.operator.as_deref().unwrap_or("")) {
                        if let Node::CallNode{..} = inner {
                            let call = inner.as_call_node().unwrap();
                            let m = String::from_utf8_lossy(call.name().as_slice());
                            if self.is_arith_op(&m) { return true; }
                        }
                    }
                }
            }

            // Cross-cop: Style/TernaryParentheses
            // When TernaryParentheses requires parentheses, don't flag ternary conditions
            if self.ternary_parentheses_required && pk == PK::TernaryCondition {
                return true;
            }

            // Cross-cop: Style/ParenthesesAroundCondition AllowInMultilineConditions
            // When enabled, don't flag multiline logical expressions
            if self.allow_in_multiline_conditions {
                if matches!(inner, Node::AndNode{..} | Node::OrNode{..}) {
                    if self.line_of(open) != self.line_of(close - 1) {
                        return true;
                    }
                }
            }

            false
        });

        if should_skip { return; }

        // Skip when inner node is a ParenthesesNode wrapping a non-classifiable node.
        // RuboCop's find_offense_message doesn't classify begin nodes, so when the inner
        // is parens wrapping something like an assignment in condition, skip the outer.
        // Exception: ((range)) patterns - RuboCop shifts these offenses to the outer.
        let should_skip_nested_parens = with_inner(&body, |inner| {
            if let Node::ParenthesesNode{..} = inner {
                // Check what's inside the inner parens
                let inner_pn = inner.as_parentheses_node().unwrap();
                if let Some(inner_body) = inner_pn.body() {
                    let is_range_literal = with_inner(&inner_body, |deep_inner| {
                        matches!(deep_inner, Node::RangeNode{..})
                    });
                    // For ((range)), don't skip - RuboCop flags the outer
                    if is_range_literal { return false; }
                }
                // For other nested parens, skip the outer
                return true;
            }
            false
        });
        if should_skip_nested_parens { return; }

        // Additional check for method calls: RuboCop's check_send only flags regular method calls,
        // not binary operator expressions. Binary ops like `1 + 2` are only flagged in specific
        // contexts (e.g., as "a method call" at top level but not inside logical operators).
        let should_skip_call = with_inner(&body, |inner| {
            if let Node::CallNode{..} = inner {
                let call = inner.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                // Binary operators in their normal syntax are handled by the classify/check logic,
                // but should not be flagged in contexts where find_offense_message returns nil
                // in RuboCop (i.e., the check_send path requires method_call_with_redundant_parentheses?)
                if is_operator_method(&m) && !is_unary_op(&call) && call.call_operator_loc().is_none() {
                    // In binary op context, don't flag inner binary operators
                    if matches!(pk, PK::BinaryOp | PK::ExponentBase | PK::ExponentPower) { return true; }
                }
            }
            false
        });
        if should_skip_call { return; }

        // Determine type description
        // Determine message type following RuboCop's priority order:
        // 1. keyword, 2. literal, 3. variable, 4. constant, 5. block body,
        // 6. assignment, 7. lambda/proc, 8. pattern matching, 9. interpolation,
        // 10. method argument, 11. rescue, 12. logical/comparison expression, 13. method call
        let inner_type = with_inner(&body, |inner| {
            let base_type = self.classify(inner);
            // "a keyword" takes priority
            if base_type == "a keyword" { return "a keyword"; }
            // "a literal" takes priority, but RuboCop's disallowed_literal? returns false
            // for a range node that is the only child of the begin node
            if base_type == "a literal" {
                let is_single_range = matches!(inner, Node::RangeNode{..}) && !is_compound(&body);
                if !is_single_range { return "a literal"; }
            }
            // "a variable" takes priority
            if base_type == "a variable" { return "a variable"; }
            // "a constant" takes priority
            if base_type == "a constant" { return "a constant"; }
            // block body - RuboCop returns "block body" when parens are direct body of a block
            if pk == PK::BlockBody { return "block body"; }
            if pk == PK::DefBody { return base_type; }
            // "an assignment" takes priority
            if base_type == "an assignment" { return "an assignment"; }
            // lambda/proc expression
            if base_type == "an expression" { return "an expression"; }
            // pattern matching
            if base_type == "a one-line pattern matching" { return "a one-line pattern matching"; }
            // interpolation
            if pk == PK::Interpolation { return "an interpolated expression"; }
            // method argument (after the above checks)
            if pk == PK::CallArgs {
                if let Some(ref p) = parent {
                    if p.call_has_parens { return "a method argument"; }
                }
            }
            // rescue
            if base_type == "a one-line rescue" { return "a one-line rescue"; }
            // Otherwise use base type
            base_type
        });

        let msg = format!("Don't use parentheses around {}.", inner_type);
        let loc = self.paren_offense_location(open, close);
        let mut offense = Offense::new(COP_NAME, &msg, Severity::Convention, loc, self.ctx.filename);

        let (inner_start, inner_end) = get_inner_offsets(&body);
        let inner_text = self.src(inner_start, inner_end);
        // If the char after closing paren is '?' (ternary), add trailing space
        // to prevent identifier? method call interpretation
        let replacement = if close < self.ctx.source.len()
            && self.ctx.source.as_bytes()[close] == b'?'
        {
            format!("{} ", inner_text)
        } else {
            inner_text.to_string()
        };
        offense = offense.with_correction(Correction::replace(open, close, &replacement));

        self.offenses.push(offense);
    }

    fn is_neg_num(&self, node: &Node) -> bool {
        match node {
            Node::IntegerNode{..} => {
                let loc = node.location();
                let s = self.src(loc.start_offset(), loc.end_offset());
                s.starts_with('-')
            }
            Node::FloatNode{..} => {
                let loc = node.location();
                let s = self.src(loc.start_offset(), loc.end_offset());
                s.starts_with('-')
            }
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                if m == "-@" {
                    if let Some(r) = call.receiver() {
                        return matches!(r, Node::IntegerNode{..} | Node::FloatNode{..});
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn starts_with_hash(&self, node: &Node) -> bool {
        match node {
            Node::HashNode{..} => true,
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                call.receiver().map(|r| self.starts_with_hash(&r)).unwrap_or(false)
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
        match node {
            Node::IfNode{..} => { let n = node.as_if_node().unwrap(); let l = n.location(); !self.src(l.start_offset(), l.end_offset()).starts_with("if") }
            Node::UnlessNode{..} => { let n = node.as_unless_node().unwrap(); let l = n.location(); !self.src(l.start_offset(), l.end_offset()).starts_with("unless") }
            Node::WhileNode{..} => { let n = node.as_while_node().unwrap(); let l = n.location(); !self.src(l.start_offset(), l.end_offset()).starts_with("while") }
            Node::UntilNode{..} => { let n = node.as_until_node().unwrap(); let l = n.location(); !self.src(l.start_offset(), l.end_offset()).starts_with("until") }
            _ => false,
        }
    }

    fn is_unparen_call(&self, node: &Node) -> bool {
        match node {
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                if is_unary_op(&call) { return false; }
                // Operator methods in normal syntax (a + b) are not "unparen calls",
                // but with dot syntax (a.+ b) they are
                if is_operator_method(&m) && call.call_operator_loc().is_none() { return false; }
                if let Some(args) = call.arguments() {
                    if args.arguments().iter().count() == 0 { return false; }
                } else { return false; }
                call.opening_loc().is_none()
            }
            Node::SuperNode{..} => {
                let s = node.as_super_node().unwrap();
                s.arguments().is_some() && s.lparen_loc().is_none()
            }
            Node::YieldNode{..} => {
                let y = node.as_yield_node().unwrap();
                y.arguments().is_some() && y.lparen_loc().is_none()
            }
            _ => false,
        }
    }

    fn is_kw_with_bare_args(&self, node: &Node) -> bool {
        match node {
            Node::DefinedNode{..} => node.as_defined_node().unwrap().lparen_loc().is_none(),
            Node::CallNode{..} => {
                let call = node.as_call_node().unwrap();
                let m = String::from_utf8_lossy(call.name().as_slice());
                if m == "!" {
                    let l = call.location();
                    return self.src(l.start_offset(), l.end_offset()).starts_with("not ");
                }
                false
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
            Node::ReturnNode{..} => {
                let r = node.as_return_node().unwrap();
                r.arguments().is_some() && self.ctx.source.as_bytes().get(r.keyword_loc().end_offset()).map(|&b| b == b' ').unwrap_or(false)
            }
            Node::BreakNode{..} => {
                let b = node.as_break_node().unwrap();
                b.arguments().is_some() && self.ctx.source.as_bytes().get(b.keyword_loc().end_offset()).map(|&b| b == b' ').unwrap_or(false)
            }
            Node::NextNode{..} => {
                let n = node.as_next_node().unwrap();
                n.arguments().is_some() && self.ctx.source.as_bytes().get(n.keyword_loc().end_offset()).map(|&b| b == b' ').unwrap_or(false)
            }
            Node::SuperNode{..} => {
                let s = node.as_super_node().unwrap();
                s.arguments().is_some() && s.lparen_loc().is_none()
            }
            Node::YieldNode{..} => {
                let y = node.as_yield_node().unwrap();
                y.arguments().is_some() && y.lparen_loc().is_none()
            }
            _ => false,
        }
    }

    fn is_kw_logical(&self, node: &Node) -> bool {
        match node {
            Node::AndNode{..} => {
                let a = node.as_and_node().unwrap();
                let ol = a.operator_loc();
                self.src(ol.start_offset(), ol.end_offset()) == "and"
            }
            Node::OrNode{..} => {
                let o = node.as_or_node().unwrap();
                let ol = o.operator_loc();
                self.src(ol.start_offset(), ol.end_offset()) == "or"
            }
            _ => false,
        }
    }

    fn logical_parens_needed(&self, inner: &Node, parent: &PC) -> bool {
        let pop = parent.operator.as_deref().unwrap_or("");
        if !self.is_logical_op(pop) { return false; }
        let (iop, inner_is_and) = match inner {
            Node::AndNode{..} => {
                let a = inner.as_and_node().unwrap();
                let ol = a.operator_loc();
                (self.src(ol.start_offset(), ol.end_offset()).to_string(), true)
            }
            Node::OrNode{..} => {
                let o = inner.as_or_node().unwrap();
                let ol = o.operator_loc();
                (self.src(ol.start_offset(), ol.end_offset()).to_string(), false)
            }
            _ => return false,
        };
        // keyword/symbol mismatch
        if is_kw_logical_op(&iop) != is_kw_logical_op(pop) { return true; }
        let parent_is_and = pop == "&&" || pop == "and";
        // Different precedence
        if inner_is_and && !parent_is_and { return true; }
        if !inner_is_and && parent_is_and { return true; }
        false
    }

    fn is_logical_op(&self, op: &str) -> bool {
        matches!(op, "&&" | "||" | "and" | "or")
    }

    fn is_arith_op(&self, op: &str) -> bool {
        matches!(op, "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^")
    }

    fn has_do_end_chain(&self, node: &Node) -> bool {
        if let Node::CallNode{..} = node {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                return self.has_do_end_in(&recv);
            }
        }
        false
    }

    fn has_do_end_in(&self, node: &Node) -> bool {
        if let Node::CallNode{..} = node {
            let call = node.as_call_node().unwrap();
            if let Some(block) = call.block() {
                if let Node::BlockNode{..} = block {
                    let bn = block.as_block_node().unwrap();
                    let ol = bn.opening_loc();
                    if self.src(ol.start_offset(), ol.end_offset()) == "do" { return true; }
                }
            }
            if let Some(recv) = call.receiver() {
                return self.has_do_end_in(&recv);
            }
        }
        false
    }

    fn is_lambda_proc_do_end(&self, node: &Node) -> bool {
        // Only `lambda do..end` and `proc do..end` method calls with do-end blocks
        // NOT `-> do..end` lambda literals (those are safe to unwrap)
        if let Node::CallNode{..} = node {
            let call = node.as_call_node().unwrap();
            let m = String::from_utf8_lossy(call.name().as_slice());
            if (m == "lambda" || m == "proc") && call.receiver().is_none() {
                if let Some(block) = call.block() {
                    if let Node::BlockNode{..} = block {
                        let bn = block.as_block_node().unwrap();
                        let ol = bn.opening_loc();
                        return self.src(ol.start_offset(), ol.end_offset()) == "do";
                    }
                }
            }
        }
        false
    }

    fn has_do_end(&self, node: &Node) -> bool {
        if let Node::CallNode{..} = node {
            let call = node.as_call_node().unwrap();
            if let Some(block) = call.block() {
                if let Node::BlockNode{..} = block {
                    let bn = block.as_block_node().unwrap();
                    let ol = bn.opening_loc();
                    if self.src(ol.start_offset(), ol.end_offset()) == "do" { return true; }
                }
            }
            if let Some(recv) = call.receiver() {
                if self.has_do_end(&recv) { return true; }
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
                if let Some(args) = call.arguments() {
                    args.arguments().iter().count() > 0 && call.opening_loc().is_none()
                } else { false }
            }
            Node::SuperNode{..} => { let s = node.as_super_node().unwrap(); s.arguments().is_some() && s.lparen_loc().is_none() }
            Node::YieldNode{..} => { let y = node.as_yield_node().unwrap(); y.arguments().is_some() && y.lparen_loc().is_none() }
            Node::DefinedNode{..} => { node.as_defined_node().unwrap().lparen_loc().is_none() }
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
                    self.vn(&arg);
                    self.pop();
                } else if has_parens {
                    let mut ctx = PC::new(PK::CallArgs);
                    ctx.call_has_parens = true;
                    ctx.arg_count = arg_count;
                    ctx.is_first_arg = idx == 0;
                    self.push(ctx);
                    self.vn(&arg);
                    self.pop();
                } else {
                    let mut ctx = PC::new(PK::MethodCallUnparen);
                    ctx.call_has_parens = false;
                    ctx.arg_count = arg_count;
                    ctx.is_first_arg = idx == 0;
                    self.push(ctx);
                    self.vn(&arg);
                    self.pop();
                }
            }
        }

        if let Some(block) = node.block() {
            self.vn(&block);
        }
    }

    /// Visit node dispatching to the correct handler
    fn vn(&mut self, node: &Node) {
        match node {
            Node::ParenthesesNode{..} => self.visit_parentheses_node(&node.as_parentheses_node().unwrap()),
            Node::CallNode{..} => self.visit_call_node(&node.as_call_node().unwrap()),
            Node::IfNode{..} => self.visit_if_node(&node.as_if_node().unwrap()),
            Node::UnlessNode{..} => self.visit_unless_node(&node.as_unless_node().unwrap()),
            Node::WhileNode{..} => self.visit_while_node(&node.as_while_node().unwrap()),
            Node::UntilNode{..} => self.visit_until_node(&node.as_until_node().unwrap()),
            Node::CaseNode{..} => self.visit_case_node(&node.as_case_node().unwrap()),
            Node::CaseMatchNode{..} => self.visit_case_match_node(&node.as_case_match_node().unwrap()),
            Node::DefNode{..} => self.visit_def_node(&node.as_def_node().unwrap()),
            Node::AndNode{..} => self.visit_and_node(&node.as_and_node().unwrap()),
            Node::OrNode{..} => self.visit_or_node(&node.as_or_node().unwrap()),
            Node::LocalVariableWriteNode{..} => self.visit_local_variable_write_node(&node.as_local_variable_write_node().unwrap()),
            Node::InstanceVariableWriteNode{..} => self.visit_instance_variable_write_node(&node.as_instance_variable_write_node().unwrap()),
            Node::ClassVariableWriteNode{..} => self.visit_class_variable_write_node(&node.as_class_variable_write_node().unwrap()),
            Node::GlobalVariableWriteNode{..} => self.visit_global_variable_write_node(&node.as_global_variable_write_node().unwrap()),
            Node::ConstantWriteNode{..} => self.visit_constant_write_node(&node.as_constant_write_node().unwrap()),
            Node::LocalVariableOperatorWriteNode{..} => self.visit_local_variable_operator_write_node(&node.as_local_variable_operator_write_node().unwrap()),
            Node::LocalVariableOrWriteNode{..} => self.visit_local_variable_or_write_node(&node.as_local_variable_or_write_node().unwrap()),
            Node::LocalVariableAndWriteNode{..} => self.visit_local_variable_and_write_node(&node.as_local_variable_and_write_node().unwrap()),
            Node::ReturnNode{..} => self.visit_return_node(&node.as_return_node().unwrap()),
            Node::BreakNode{..} => self.visit_break_node(&node.as_break_node().unwrap()),
            Node::NextNode{..} => self.visit_next_node(&node.as_next_node().unwrap()),
            Node::SuperNode{..} => self.visit_super_node(&node.as_super_node().unwrap()),
            Node::ForwardingSuperNode{..} => self.visit_forwarding_super_node(&node.as_forwarding_super_node().unwrap()),
            Node::YieldNode{..} => self.visit_yield_node(&node.as_yield_node().unwrap()),
            Node::BlockNode{..} => self.visit_block_node(&node.as_block_node().unwrap()),
            Node::LambdaNode{..} => self.visit_lambda_node(&node.as_lambda_node().unwrap()),
            Node::EmbeddedStatementsNode{..} => self.visit_embedded_statements_node(&node.as_embedded_statements_node().unwrap()),
            Node::SplatNode{..} => self.visit_splat_node(&node.as_splat_node().unwrap()),
            Node::KeywordHashNode{..} => self.visit_keyword_hash_node(&node.as_keyword_hash_node().unwrap()),
            Node::RangeNode{..} => self.visit_range_node(&node.as_range_node().unwrap()),
            Node::ArrayNode{..} => self.visit_array_node(&node.as_array_node().unwrap()),
            Node::HashNode{..} => self.visit_hash_node(&node.as_hash_node().unwrap()),
            Node::StatementsNode{..} => self.visit_statements_node(&node.as_statements_node().unwrap()),
            Node::PinnedExpressionNode{..} => self.visit_pinned_expression_node(&node.as_pinned_expression_node().unwrap()),
            Node::AssocNode{..} => self.visit_assoc_node(&node.as_assoc_node().unwrap()),
            Node::AssocSplatNode{..} => self.visit_assoc_splat_node(&node.as_assoc_splat_node().unwrap()),
            Node::InterpolatedStringNode{..} => self.visit_interpolated_string_node(&node.as_interpolated_string_node().unwrap()),
            Node::InterpolatedSymbolNode{..} => self.visit_interpolated_symbol_node(&node.as_interpolated_symbol_node().unwrap()),
            Node::InterpolatedRegularExpressionNode{..} => self.visit_interpolated_regular_expression_node(&node.as_interpolated_regular_expression_node().unwrap()),
            Node::ElseNode{..} => ruby_prism::visit_else_node(self, &node.as_else_node().unwrap()),
            _ => {
                // For other nodes, recurse with Other context into common containers
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
                    // Pattern matching nodes
                    Node::MatchPredicateNode{..} => ruby_prism::visit_match_predicate_node(self, &node.as_match_predicate_node().unwrap()),
                    Node::MatchRequiredNode{..} => ruby_prism::visit_match_required_node(self, &node.as_match_required_node().unwrap()),
                    Node::HashPatternNode{..} => ruby_prism::visit_hash_pattern_node(self, &node.as_hash_pattern_node().unwrap()),
                    Node::ArrayPatternNode{..} => ruby_prism::visit_array_pattern_node(self, &node.as_array_pattern_node().unwrap()),
                    Node::FindPatternNode{..} => ruby_prism::visit_find_pattern_node(self, &node.as_find_pattern_node().unwrap()),
                    Node::CapturePatternNode{..} => ruby_prism::visit_capture_pattern_node(self, &node.as_capture_pattern_node().unwrap()),
                    Node::InNode{..} => ruby_prism::visit_in_node(self, &node.as_in_node().unwrap()),
                    Node::CaseMatchNode{..} => self.visit_case_match_node(&node.as_case_match_node().unwrap()),
                    _ => {} // Leaf nodes
                }
                self.pop();
            }
        }
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
    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        self.check_parens(node);
        if let Some(body) = node.body() {
            self.push(PC::new(PK::Statements));
            self.vn(&body);
            self.pop();
        }
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let m = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if is_unary_op(node) {
            if let Some(recv) = node.receiver() {
                let mut ctx = PC::new(PK::UnaryOp);
                ctx.operator = Some(m);
                self.push(ctx);
                self.vn(&recv);
                self.pop();
            }
            return;
        }
        self.visit_call_children(node);
    }
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let l = node.location();
        let s = self.src(l.start_offset(), l.end_offset());
        let ternary = !s.starts_with("if") && !s.starts_with("elsif");
        let ck = if ternary { PK::TernaryCondition } else { PK::Condition };
        self.push(PC::new(ck));
        self.vn(&node.predicate());
        self.pop();
        if let Some(body) = node.statements() {
            self.push(PC::new(if ternary { PK::Ternary } else { PK::IfBody }));
            self.visit_statements_node(&body);
            self.pop();
        }
        if let Some(sub) = node.subsequent() {
            self.push(PC::new(if ternary { PK::Ternary } else { PK::IfBody }));
            self.vn(&sub);
            self.pop();
        }
    }
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.push(PC::new(PK::Condition));
        self.vn(&node.predicate());
        self.pop();
        if let Some(body) = node.statements() {
            self.push(PC::new(PK::IfBody));
            self.visit_statements_node(&body);
            self.pop();
        }
        if let Some(ec) = node.else_clause() {
            self.push(PC::new(PK::IfBody));
            ruby_prism::visit_else_node(self, &ec);
            self.pop();
        }
    }
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        self.push(PC::new(PK::Condition));
        self.vn(&node.predicate());
        self.pop();
        if let Some(body) = node.statements() {
            self.push(PC::new(PK::Statements));
            self.visit_statements_node(&body);
            self.pop();
        }
    }
    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.push(PC::new(PK::Condition));
        self.vn(&node.predicate());
        self.pop();
        if let Some(body) = node.statements() {
            self.push(PC::new(PK::Statements));
            self.visit_statements_node(&body);
            self.pop();
        }
    }
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if let Some(pred) = node.predicate() {
            self.push(PC::new(PK::CaseCondition));
            self.vn(&pred);
            self.pop();
        }
        for c in node.conditions().iter() {
            self.push(PC::new(PK::Other));
            self.vn(&c);
            self.pop();
        }
        if let Some(ec) = node.else_clause() {
            self.push(PC::new(PK::IfBody));
            ruby_prism::visit_else_node(self, &ec);
            self.pop();
        }
    }
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        if let Some(pred) = node.predicate() {
            self.push(PC::new(PK::CaseCondition));
            self.vn(&pred);
            self.pop();
        }
        for c in node.conditions().iter() {
            self.push(PC::new(PK::Other));
            self.vn(&c);
            self.pop();
        }
        if let Some(ec) = node.else_clause() {
            self.push(PC::new(PK::IfBody));
            ruby_prism::visit_else_node(self, &ec);
            self.pop();
        }
    }
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let endless = node.end_keyword_loc().is_none() && node.equal_loc().is_some();
        if let Some(params) = node.parameters() {
            self.push(PC::new(PK::Other));
            ruby_prism::visit_parameters_node(self, &params);
            self.pop();
        }
        if let Some(body) = node.body() {
            self.push(PC::new(if endless { PK::EndlessMethodBody } else { PK::DefBody }));
            self.vn(&body);
            self.pop();
        }
    }
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let ol = node.operator_loc();
        let op = self.src(ol.start_offset(), ol.end_offset()).to_string();
        let mut c = PC::new(PK::BinaryOp); c.operator = Some(op.clone());
        self.push(c); self.vn(&node.left()); self.pop();
        let mut c = PC::new(PK::BinaryOp); c.operator = Some(op);
        self.push(c); self.vn(&node.right()); self.pop();
    }
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let ol = node.operator_loc();
        let op = self.src(ol.start_offset(), ol.end_offset()).to_string();
        let mut c = PC::new(PK::BinaryOp); c.operator = Some(op.clone());
        self.push(c); self.vn(&node.left()); self.pop();
        let mut c = PC::new(PK::BinaryOp); c.operator = Some(op);
        self.push(c); self.vn(&node.right()); self.pop();
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        self.push(PC::new(PK::Assignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        self.push(PC::new(PK::Assignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        self.push(PC::new(PK::Assignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.push(PC::new(PK::Assignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        self.push(PC::new(PK::Assignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        self.push(PC::new(PK::OpAssignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        self.push(PC::new(PK::OpAssignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        self.push(PC::new(PK::OpAssignment)); self.vn(&node.value()); self.pop();
    }
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() { self.push(PC::new(PK::Return)); self.vn(&arg); self.pop(); }
        }
    }
    fn visit_break_node(&mut self, node: &ruby_prism::BreakNode) {
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() { self.push(PC::new(PK::Break)); self.vn(&arg); self.pop(); }
        }
    }
    fn visit_next_node(&mut self, node: &ruby_prism::NextNode) {
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() { self.push(PC::new(PK::Next)); self.vn(&arg); self.pop(); }
        }
    }
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        let hp = node.lparen_loc().is_some();
        if let Some(args) = node.arguments() {
            let ac = args.arguments().iter().count();
            for arg in args.arguments().iter() {
                let mut c = PC::new(if hp { PK::CallArgs } else { PK::Super });
                c.call_has_parens = hp; c.arg_count = ac;
                self.push(c); self.vn(&arg); self.pop();
            }
        }
        if let Some(block) = node.block() { self.vn(&block); }
    }
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        if let Some(block) = node.block() { self.visit_block_node(&block); }
    }
    fn visit_yield_node(&mut self, node: &ruby_prism::YieldNode) {
        let hp = node.lparen_loc().is_some();
        if let Some(args) = node.arguments() {
            let ac = args.arguments().iter().count();
            for arg in args.arguments().iter() {
                let mut c = PC::new(if hp { PK::CallArgs } else { PK::Yield });
                c.call_has_parens = hp; c.arg_count = ac;
                self.push(c); self.vn(&arg); self.pop();
            }
        }
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if let Some(params) = node.parameters() {
            self.push(PC::new(PK::Other)); self.vn(&params); self.pop();
        }
        if let Some(body) = node.body() {
            // Only use BlockBody when the parens IS the direct/only body statement.
            // For multi-statement block bodies, use Statements for individual statements.
            let pk = if is_single_statement(&body) { PK::BlockBody } else { PK::Statements };
            self.push(PC::new(pk)); self.vn(&body); self.pop();
        }
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        if let Some(body) = node.body() {
            let pk = if is_single_statement(&body) { PK::BlockBody } else { PK::Statements };
            self.push(PC::new(pk)); self.vn(&body); self.pop();
        }
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        for part in node.parts().iter() {
            if let Node::EmbeddedStatementsNode{..} = part {
                self.visit_embedded_statements_node(&part.as_embedded_statements_node().unwrap());
            }
        }
    }
    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        for part in node.parts().iter() {
            if let Node::EmbeddedStatementsNode{..} = part {
                self.visit_embedded_statements_node(&part.as_embedded_statements_node().unwrap());
            }
        }
    }
    fn visit_interpolated_regular_expression_node(&mut self, node: &ruby_prism::InterpolatedRegularExpressionNode) {
        for part in node.parts().iter() {
            if let Node::EmbeddedStatementsNode{..} = part {
                self.visit_embedded_statements_node(&part.as_embedded_statements_node().unwrap());
            }
        }
    }
    fn visit_embedded_statements_node(&mut self, node: &ruby_prism::EmbeddedStatementsNode) {
        if let Some(stmts) = node.statements() {
            self.push(PC::new(PK::Interpolation));
            self.visit_statements_node(&stmts);
            self.pop();
        }
    }
    fn visit_splat_node(&mut self, node: &ruby_prism::SplatNode) {
        if let Some(expr) = node.expression() {
            self.push(PC::new(PK::Splat)); self.vn(&expr); self.pop();
        }
    }
    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        for elem in node.elements().iter() {
            self.push(PC::new(PK::Hash)); self.vn(&elem); self.pop();
        }
    }
    fn visit_range_node(&mut self, node: &ruby_prism::RangeNode) {
        if let Some(l) = node.left() { self.push(PC::new(PK::RangeOperand)); self.vn(&l); self.pop(); }
        if let Some(r) = node.right() { self.push(PC::new(PK::RangeOperand)); self.vn(&r); self.pop(); }
    }
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        for elem in node.elements().iter() {
            self.push(PC::new(PK::Array)); self.vn(&elem); self.pop();
        }
    }
    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        for elem in node.elements().iter() {
            self.push(PC::new(PK::Hash)); self.vn(&elem); self.pop();
        }
    }
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
        self.push(PC::new(PK::Hash)); self.vn(&node.value()); self.pop();
        self.push(PC::new(PK::Other)); self.vn(&node.key()); self.pop();
    }
    fn visit_assoc_splat_node(&mut self, node: &ruby_prism::AssocSplatNode) {
        if let Some(v) = node.value() {
            self.push(PC::new(PK::Splat)); self.vn(&v); self.pop();
        }
    }
    fn visit_pinned_expression_node(&mut self, node: &ruby_prism::PinnedExpressionNode) {
        // PinnedExpressionNode represents ^(expr) - the parens are part of the syntax.
        // When expr is a simple variable, parens are redundant (can use ^var instead of ^(var)).
        let expr = node.expression();
        if matches!(expr, Node::LocalVariableReadNode{..} | Node::InstanceVariableReadNode{..}
            | Node::ClassVariableReadNode{..} | Node::GlobalVariableReadNode{..}) {
            // The parens around the variable are redundant
            let lp = node.lparen_loc();
            let rp = node.rparen_loc();
            let paren_start = lp.start_offset();
            let paren_end = rp.end_offset();
            let msg = "Don't use parentheses around a variable.";
            let offense_loc = crate::offense::Location::from_offsets(self.ctx.source, paren_start, paren_end);
            let inner_src = self.src(lp.end_offset(), rp.start_offset());
            let offense = Offense::new(COP_NAME, msg, Severity::Convention, offense_loc, self.ctx.filename)
                .with_correction(Correction::replace(paren_start, paren_end, inner_src));
            self.offenses.push(offense);
        }
        self.push(PC::new(PK::PinExpression)); self.vn(&expr); self.pop();
    }
}

fn is_unary_op(call: &ruby_prism::CallNode) -> bool {
    let m = String::from_utf8_lossy(call.name().as_slice());
    call.receiver().is_some() && call.arguments().is_none() && matches!(m.as_ref(), "!" | "~" | "-@" | "+@")
}

fn is_unary_op_node(node: &Node) -> bool {
    if let Node::CallNode{..} = node { is_unary_op(&node.as_call_node().unwrap()) } else { false }
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
