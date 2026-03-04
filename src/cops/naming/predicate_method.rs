//! Naming/PredicateMethod - Checks that predicate methods end with `?` and non-predicate
//! methods don't end with `?`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/naming/predicate_method.rb
//!
//! Two modes:
//! - **Conservative**: Only flags non-`?` methods if ALL returns are boolean.
//!   Only flags `?` methods if ALL returns are non-boolean.
//!   Unknown returns (variables, method calls, super, conditionals with mixed branches) are
//!   treated as "acceptable" - they never trigger offenses.
//! - **Aggressive**: Flags `?` methods if ANY return is not boolean.
//!   Only flags non-`?` methods if ALL returns are boolean (same as conservative for this case).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use ruby_prism::Node;

const MSG_PREDICATE: &str = "Predicate method names should end with `?`.";
const MSG_NON_PREDICATE: &str = "Non-predicate method names should not end with `?`.";

/// Operator method names that should be excluded from this cop
const OPERATOR_METHODS: &[&str] = &[
    "==", "===", "!=", "<=>", "<", ">", "<=", ">=", "=~", "!~", "&", "|", "^", "~", "<<", ">>",
    "+", "-", "*", "/", "%", "**", "+@", "-@", "[]", "[]=", "`",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Conservative,
    Aggressive,
}

/// Classification of a return value
#[derive(Debug, Clone, Copy, PartialEq)]
enum ReturnKind {
    /// Definitely a boolean return (true, false, comparison, negation, predicate call)
    Boolean,
    /// Definitely not a boolean return (nil, string, integer, array, hash, etc.)
    NonBoolean,
    /// Unknown/indeterminate (variable, non-predicate method call, super, etc.)
    Unknown,
}

pub struct PredicateMethod {
    mode: Mode,
    allow_bang_methods: bool,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
    wayward_predicates: Vec<String>,
}

impl PredicateMethod {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            allow_bang_methods: false,
            allowed_methods: vec![],
            allowed_patterns: vec![],
            wayward_predicates: vec![],
        }
    }

    pub fn with_config(
        mode: Mode,
        allow_bang_methods: bool,
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
        wayward_predicates: Vec<String>,
    ) -> Self {
        Self {
            mode,
            allow_bang_methods,
            allowed_methods,
            allowed_patterns,
            wayward_predicates,
        }
    }

    fn is_allowed_method(&self, method_name: &str) -> bool {
        if self.allowed_methods.iter().any(|m| m == method_name) {
            return true;
        }
        for pattern in &self.allowed_patterns {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(method_name) {
                    return true;
                }
            }
        }
        false
    }

    fn is_operator_method(method_name: &str) -> bool {
        OPERATOR_METHODS.contains(&method_name)
    }

    /// Classify a node's return value as boolean, non-boolean, or unknown
    fn classify_return(&self, node: &Node, source: &str) -> ReturnKind {
        match node {
            // Return nodes - classify based on the returned value
            Node::ReturnNode { .. } => {
                let ret = node.as_return_node().unwrap();
                if let Some(args) = ret.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if arg_list.len() > 1 {
                        ReturnKind::NonBoolean
                    } else if arg_list.len() == 1 {
                        self.classify_return(&arg_list[0], source)
                    } else {
                        ReturnKind::NonBoolean // return with no args = nil
                    }
                } else {
                    ReturnKind::NonBoolean // Bare return = nil
                }
            }

            // Boolean literals
            Node::TrueNode { .. } | Node::FalseNode { .. } => ReturnKind::Boolean,

            // Non-boolean literals
            Node::NilNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::InterpolatedRegularExpressionNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::RangeNode { .. }
            | Node::SelfNode { .. }
            | Node::LambdaNode { .. }
            | Node::XStringNode { .. }
            | Node::InterpolatedXStringNode { .. } => ReturnKind::NonBoolean,

            // Call nodes - check the method being called
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let name = String::from_utf8_lossy(call.name().as_slice()).to_string();

                // Negation (!) - always boolean
                if name == "!" {
                    return ReturnKind::Boolean;
                }

                // Comparison operators
                if is_comparison_method(&name) {
                    return ReturnKind::Boolean;
                }

                // Predicate method calls (name ends with ?)
                if name.ends_with('?') {
                    // Check if it's a wayward predicate
                    if self.wayward_predicates.iter().any(|w| w == &name) {
                        return ReturnKind::Unknown;
                    }
                    return ReturnKind::Boolean;
                }

                ReturnKind::Unknown
            }

            // and/or chains - need to examine both sides
            Node::AndNode { .. } => {
                let and_node = node.as_and_node().unwrap();
                let left = self.classify_return(&and_node.left(), source);
                let right = self.classify_return(&and_node.right(), source);
                combine_and_or(left, right)
            }

            Node::OrNode { .. } => {
                let or_node = node.as_or_node().unwrap();
                let left = self.classify_return(&or_node.left(), source);
                let right = self.classify_return(&or_node.right(), source);
                combine_and_or(left, right)
            }

            // If - classify based on branches
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                self.classify_if_node(if_node, source)
            }

            // Unless - same branch structure as if (condition inversion irrelevant for return classification)
            Node::UnlessNode { .. } => {
                let unless_node = node.as_unless_node().unwrap();
                self.classify_unless_node(&unless_node, source)
            }

            // While/Until loops - classify based on body's last expression (semantic return intent)
            Node::WhileNode { .. } => {
                let while_node = node.as_while_node().unwrap();
                self.classify_loop_body(while_node.statements(), source)
            }

            Node::UntilNode { .. } => {
                let until_node = node.as_until_node().unwrap();
                self.classify_loop_body(until_node.statements(), source)
            }

            // Case/when
            Node::CaseNode { .. } => {
                let case_node = node.as_case_node().unwrap();
                self.classify_case_node(&case_node, source)
            }

            // Case/in (pattern matching)
            Node::CaseMatchNode { .. } => {
                let case_match = node.as_case_match_node().unwrap();
                self.classify_case_match_node(&case_match, source)
            }

            // Begin block - use last statement
            Node::BeginNode { .. } => {
                let begin_node = node.as_begin_node().unwrap();
                if let Some(stmts) = begin_node.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    if let Some(last) = body.last() {
                        return self.classify_return(last, source);
                    }
                }
                ReturnKind::Unknown
            }

            // Parentheses node - classify inner
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    self.classify_return(&body, source)
                } else {
                    // Empty parens ()
                    ReturnKind::NonBoolean
                }
            }

            // StatementsNode - use last statement
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    self.classify_return(last, source)
                } else {
                    ReturnKind::Unknown
                }
            }

            // DefinedNode (`defined?`) - returns string or nil
            Node::DefinedNode { .. } => ReturnKind::NonBoolean,

            // Variables - unknown
            Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::ConstantReadNode { .. }
            | Node::ConstantPathNode { .. } => ReturnKind::Unknown,

            // Super calls - unknown
            Node::SuperNode { .. } | Node::ForwardingSuperNode { .. } => ReturnKind::Unknown,

            // Yield - unknown
            Node::YieldNode { .. } => ReturnKind::Unknown,

            // Everything else is unknown
            _ => ReturnKind::Unknown,
        }
    }

    fn classify_if_node(
        &self,
        if_node: ruby_prism::IfNode,
        source: &str,
    ) -> ReturnKind {
        let then_kind = match if_node.statements() {
            Some(stmts) => {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    self.classify_return(last, source)
                } else {
                    // Empty then branch = nil
                    ReturnKind::NonBoolean
                }
            }
            None => ReturnKind::NonBoolean, // Missing then = nil
        };

        let else_kind = match if_node.subsequent() {
            Some(else_node) => match &else_node {
                Node::ElseNode { .. } => {
                    let else_n = else_node.as_else_node().unwrap();
                    if let Some(stmts) = else_n.statements() {
                        let body: Vec<_> = stmts.body().iter().collect();
                        if let Some(last) = body.last() {
                            self.classify_return(last, source)
                        } else {
                            ReturnKind::NonBoolean
                        }
                    } else {
                        ReturnKind::NonBoolean
                    }
                }
                // elsif is another IfNode
                Node::IfNode { .. } => {
                    let nested_if = else_node.as_if_node().unwrap();
                    self.classify_if_node(nested_if, source)
                }
                _ => self.classify_return(&else_node, source),
            },
            None => ReturnKind::NonBoolean, // No else = implicit nil
        };

        combine_branches(then_kind, else_kind, self.mode)
    }

    fn classify_unless_node(
        &self,
        unless_node: &ruby_prism::UnlessNode,
        source: &str,
    ) -> ReturnKind {
        let then_kind = match unless_node.statements() {
            Some(stmts) => {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    self.classify_return(last, source)
                } else {
                    ReturnKind::NonBoolean
                }
            }
            None => ReturnKind::NonBoolean,
        };

        let else_kind = match unless_node.else_clause() {
            Some(else_node) => {
                if let Some(stmts) = else_node.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    if let Some(last) = body.last() {
                        self.classify_return(last, source)
                    } else {
                        ReturnKind::NonBoolean
                    }
                } else {
                    ReturnKind::NonBoolean
                }
            }
            None => ReturnKind::NonBoolean,
        };

        combine_branches(then_kind, else_kind, self.mode)
    }

    fn classify_loop_body(
        &self,
        statements: Option<ruby_prism::StatementsNode>,
        source: &str,
    ) -> ReturnKind {
        match statements {
            Some(stmts) => {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    self.classify_return(last, source)
                } else {
                    ReturnKind::NonBoolean // Empty loop body = nil
                }
            }
            None => ReturnKind::NonBoolean, // No body = nil
        }
    }

    fn classify_case_node(
        &self,
        case_node: &ruby_prism::CaseNode,
        source: &str,
    ) -> ReturnKind {
        let mut result = None;

        for condition in case_node.conditions().iter() {
            if let Node::WhenNode { .. } = &condition {
                let when = condition.as_when_node().unwrap();
                let kind = if let Some(stmts) = when.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    if let Some(last) = body.last() {
                        self.classify_return(last, source)
                    } else {
                        ReturnKind::NonBoolean
                    }
                } else {
                    ReturnKind::NonBoolean
                };
                result = Some(match result {
                    None => kind,
                    Some(prev) => combine_branches(prev, kind, self.mode),
                });
            }
        }

        // Handle else branch
        let else_kind = if let Some(else_node) = case_node.else_clause() {
            if let Some(stmts) = else_node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    self.classify_return(last, source)
                } else {
                    ReturnKind::NonBoolean
                }
            } else {
                ReturnKind::NonBoolean
            }
        } else {
            ReturnKind::NonBoolean // No else = implicit nil
        };

        match result {
            None => else_kind,
            Some(prev) => combine_branches(prev, else_kind, self.mode),
        }
    }

    fn classify_case_match_node(
        &self,
        case_match: &ruby_prism::CaseMatchNode,
        source: &str,
    ) -> ReturnKind {
        let mut result = None;

        for condition in case_match.conditions().iter() {
            if let Node::InNode { .. } = &condition {
                let in_node = condition.as_in_node().unwrap();
                let kind = if let Some(stmts) = in_node.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    if let Some(last) = body.last() {
                        self.classify_return(last, source)
                    } else {
                        ReturnKind::NonBoolean
                    }
                } else {
                    ReturnKind::NonBoolean
                };
                result = Some(match result {
                    None => kind,
                    Some(prev) => combine_branches(prev, kind, self.mode),
                });
            }
        }

        // Handle else branch
        let else_kind = if let Some(else_node) = case_match.else_clause() {
            if let Some(stmts) = else_node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = body.last() {
                    self.classify_return(last, source)
                } else {
                    ReturnKind::NonBoolean
                }
            } else {
                ReturnKind::NonBoolean
            }
        } else {
            ReturnKind::NonBoolean // No else = implicit nil
        };

        match result {
            None => else_kind,
            Some(prev) => combine_branches(prev, else_kind, self.mode),
        }
    }

    /// Collect all return value classifications from a method body.
    /// Returns (explicit_returns, implicit_return_kind).
    fn collect_returns(
        &self,
        body_node: &Node,
        source: &str,
    ) -> (Vec<ReturnKind>, Option<ReturnKind>) {
        let mut explicit_returns = Vec::new();

        // For StatementsNode bodies (normal methods)
        if let Some(stmts) = body_node.as_statements_node() {
            let body: Vec<_> = stmts.body().iter().collect();

            // Collect explicit returns from anywhere in the body
            for stmt in &body {
                self.collect_explicit_returns(stmt, source, &mut explicit_returns);
            }

            // Get implicit return (last expression)
            let implicit = if let Some(last) = body.last() {
                if matches!(last, Node::ReturnNode { .. }) {
                    None
                } else {
                    Some(self.classify_return(last, source))
                }
            } else {
                None
            };

            return (explicit_returns, implicit);
        }

        // For non-StatementsNode bodies (like endless methods)
        self.collect_explicit_returns(body_node, source, &mut explicit_returns);
        let implicit = if matches!(body_node, Node::ReturnNode { .. }) {
            None
        } else {
            Some(self.classify_return(body_node, source))
        };

        (explicit_returns, implicit)
    }

    /// Recursively collect explicit return classifications
    fn collect_explicit_returns(
        &self,
        node: &Node,
        source: &str,
        returns: &mut Vec<ReturnKind>,
    ) {
        match node {
            Node::ReturnNode { .. } => {
                let ret = node.as_return_node().unwrap();
                if let Some(args) = ret.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if arg_list.len() > 1 {
                        // Multiple return values = array, non-boolean
                        returns.push(ReturnKind::NonBoolean);
                    } else if arg_list.len() == 1 {
                        returns.push(self.classify_return(&arg_list[0], source));
                    } else {
                        // return with no args = nil
                        returns.push(ReturnKind::NonBoolean);
                    }
                } else {
                    // Bare return = nil
                    returns.push(ReturnKind::NonBoolean);
                }
            }
            // Recurse into control flow constructs to find nested returns
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                if let Some(stmts) = if_node.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
                if let Some(subsequent) = if_node.subsequent() {
                    self.collect_explicit_returns(&subsequent, source, returns);
                }
            }
            Node::ElseNode { .. } => {
                let else_node = node.as_else_node().unwrap();
                if let Some(stmts) = else_node.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
            }
            Node::UnlessNode { .. } => {
                let unless_node = node.as_unless_node().unwrap();
                if let Some(stmts) = unless_node.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
                if let Some(else_clause) = unless_node.else_clause() {
                    if let Some(stmts) = else_clause.statements() {
                        for stmt in stmts.body().iter() {
                            self.collect_explicit_returns(&stmt, source, returns);
                        }
                    }
                }
            }
            Node::WhileNode { .. } => {
                let while_node = node.as_while_node().unwrap();
                if let Some(stmts) = while_node.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
            }
            Node::UntilNode { .. } => {
                let until_node = node.as_until_node().unwrap();
                if let Some(stmts) = until_node.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
            }
            Node::CaseNode { .. } => {
                let case_node = node.as_case_node().unwrap();
                for condition in case_node.conditions().iter() {
                    self.collect_explicit_returns(&condition, source, returns);
                }
                if let Some(else_clause) = case_node.else_clause() {
                    if let Some(stmts) = else_clause.statements() {
                        for stmt in stmts.body().iter() {
                            self.collect_explicit_returns(&stmt, source, returns);
                        }
                    }
                }
            }
            Node::WhenNode { .. } => {
                let when = node.as_when_node().unwrap();
                if let Some(stmts) = when.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
            }
            Node::BeginNode { .. } => {
                let begin = node.as_begin_node().unwrap();
                if let Some(stmts) = begin.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
            }
            Node::RescueNode { .. } => {
                let rescue = node.as_rescue_node().unwrap();
                if let Some(stmts) = rescue.statements() {
                    for stmt in stmts.body().iter() {
                        self.collect_explicit_returns(&stmt, source, returns);
                    }
                }
                for exception in rescue.exceptions().iter() {
                    self.collect_explicit_returns(&exception, source, returns);
                }
                if let Some(subsequent) = rescue.subsequent() {
                    // subsequent is a RescueNode - need to recurse
                    let subseq_rescue = subsequent;
                    if let Some(stmts) = subseq_rescue.statements() {
                        for stmt in stmts.body().iter() {
                            self.collect_explicit_returns(&stmt, source, returns);
                        }
                    }
                }
            }
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for stmt in stmts.body().iter() {
                    self.collect_explicit_returns(&stmt, source, returns);
                }
            }
            _ => {
                // Don't recurse into nested def nodes or other constructs
            }
        }
    }

    /// Check a method definition and return an offense if applicable
    fn check_method(
        &self,
        method_name: &str,
        name_start_offset: usize,
        name_end_offset: usize,
        body: Option<Node>,
        source: &str,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        // Skip initialize, operator methods
        if method_name == "initialize" {
            return vec![];
        }
        if Self::is_operator_method(method_name) {
            return vec![];
        }

        // Skip allowed methods
        if self.is_allowed_method(method_name) {
            return vec![];
        }

        // Skip bang methods if configured
        if self.allow_bang_methods && method_name.ends_with('!') {
            return vec![];
        }

        let has_question_mark = method_name.ends_with('?');

        // Check if body is empty/nil
        let body_node = match body {
            Some(b) => b,
            None => return vec![], // No body = no offense
        };

        // Check if body is empty
        if self.is_body_empty(&body_node) {
            return vec![];
        }

        // Determine the predicate classification
        let (explicit_returns, implicit_return) = self.collect_returns(&body_node, source);
        let classification =
            self.determine_classification(&explicit_returns, implicit_return);

        match classification {
            MethodClassification::Predicate => {
                if !has_question_mark {
                    return vec![ctx.offense_with_range(
                        self.name(),
                        MSG_PREDICATE,
                        self.severity(),
                        name_start_offset,
                        name_end_offset,
                    )];
                }
            }
            MethodClassification::NonPredicate => {
                if has_question_mark {
                    return vec![ctx.offense_with_range(
                        self.name(),
                        MSG_NON_PREDICATE,
                        self.severity(),
                        name_start_offset,
                        name_end_offset,
                    )];
                }
            }
            MethodClassification::Acceptable => {
                // No offense either way
            }
        }

        vec![]
    }

    fn is_body_empty(&self, body: &Node) -> bool {
        match body {
            Node::StatementsNode { .. } => {
                let stmts = body.as_statements_node().unwrap();
                let body_stmts: Vec<_> = stmts.body().iter().collect();
                if body_stmts.is_empty() {
                    return true;
                }
                // Check if all statements are empty parentheses
                if body_stmts.len() == 1 {
                    return self.is_empty_parens_tree(&body_stmts[0]);
                }
                false
            }
            _ => false,
        }
    }

    fn is_empty_parens_tree(&self, node: &Node) -> bool {
        match node {
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                paren.body().is_none()
            }
            Node::CaseMatchNode { .. } => {
                // Check if all in-patterns have only empty parens
                let case_match = node.as_case_match_node().unwrap();
                for condition in case_match.conditions().iter() {
                    if let Node::InNode { .. } = &condition {
                        let in_node = condition.as_in_node().unwrap();
                        if let Some(stmts) = in_node.statements() {
                            let body: Vec<_> = stmts.body().iter().collect();
                            if body.len() == 1 && self.is_empty_parens_tree(&body[0]) {
                                continue;
                            }
                        }
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn determine_classification(
        &self,
        explicit_returns: &[ReturnKind],
        implicit_return: Option<ReturnKind>,
    ) -> MethodClassification {
        let mut all_returns: Vec<ReturnKind> = explicit_returns.to_vec();
        if let Some(implicit) = implicit_return {
            all_returns.push(implicit);
        }

        if all_returns.is_empty() {
            return MethodClassification::Acceptable;
        }

        let all_boolean = all_returns.iter().all(|k| *k == ReturnKind::Boolean);
        let all_non_boolean = all_returns.iter().all(|k| *k == ReturnKind::NonBoolean);

        match self.mode {
            Mode::Conservative => {
                if all_boolean {
                    MethodClassification::Predicate
                } else if all_non_boolean {
                    MethodClassification::NonPredicate
                } else {
                    MethodClassification::Acceptable
                }
            }
            Mode::Aggressive => {
                if all_boolean {
                    MethodClassification::Predicate
                } else if all_returns.iter().any(|k| *k == ReturnKind::NonBoolean) {
                    // In aggressive mode, any definite non-boolean → non-predicate
                    MethodClassification::NonPredicate
                } else {
                    // Mix of boolean + unknown with no definite non-boolean → acceptable
                    MethodClassification::Acceptable
                }
            }
        }
    }
}

fn is_comparison_method(name: &str) -> bool {
    matches!(
        name,
        "==" | "===" | "!=" | "<" | ">" | "<=" | ">=" | "=~" | "!~" | "match?"
    )
}

fn combine_and_or(left: ReturnKind, right: ReturnKind) -> ReturnKind {
    match (left, right) {
        (ReturnKind::Boolean, ReturnKind::Boolean) => ReturnKind::Boolean,
        (ReturnKind::NonBoolean, _) | (_, ReturnKind::NonBoolean) => ReturnKind::NonBoolean,
        _ => ReturnKind::Unknown,
    }
}

fn combine_branches(a: ReturnKind, b: ReturnKind, mode: Mode) -> ReturnKind {
    match (a, b) {
        (ReturnKind::Boolean, ReturnKind::Boolean) => ReturnKind::Boolean,
        (ReturnKind::NonBoolean, ReturnKind::NonBoolean) => ReturnKind::NonBoolean,
        _ => match mode {
            // In aggressive mode, NonBoolean contaminates (any non-boolean branch = non-predicate)
            Mode::Aggressive => {
                if a == ReturnKind::NonBoolean || b == ReturnKind::NonBoolean {
                    ReturnKind::NonBoolean
                } else {
                    ReturnKind::Unknown
                }
            }
            // In conservative mode, mixed = unknown
            Mode::Conservative => ReturnKind::Unknown,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MethodClassification {
    /// All returns are boolean - method should end with ?
    Predicate,
    /// Returns are definitely non-boolean - method should NOT end with ?
    NonPredicate,
    /// Can't determine / mixed / acceptable - no offense
    Acceptable,
}

impl Cop for PredicateMethod {
    fn name(&self) -> &'static str {
        "Naming/PredicateMethod"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let name_loc = node.name_loc();
        let name_start = name_loc.start_offset();
        let name_end = name_loc.end_offset();

        self.check_method(
            &method_name,
            name_start,
            name_end,
            node.body(),
            ctx.source,
            ctx,
        )
    }
}
