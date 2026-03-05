//! Naming/PredicateMethod - Checks that predicate methods end with `?` and non-predicate
//! methods don't end with `?`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use ruby_prism::Node;

const MSG_PREDICATE: &str = "Predicate method names should end with `?`.";
const MSG_NON_PREDICATE: &str = "Non-predicate method names should not end with `?`.";

const OPERATOR_METHODS: &[&str] = &[
    "==", "===", "!=", "<=>", "<", ">", "<=", ">=", "=~", "!~", "&", "|", "^", "~", "<<", ">>",
    "+", "-", "*", "/", "%", "**", "+@", "-@", "[]", "[]=", "`",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Conservative,
    Aggressive,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ReturnKind {
    Boolean,
    NonBoolean,
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
        Self { mode, allow_bang_methods, allowed_methods, allowed_patterns, wayward_predicates }
    }

    fn is_allowed_method(&self, method_name: &str) -> bool {
        if self.allowed_methods.iter().any(|m| m == method_name) {
            return true;
        }
        self.allowed_patterns.iter().any(|pattern| {
            Regex::new(pattern).map_or(false, |re| re.is_match(method_name))
        })
    }

    /// Classify the last statement of an optional StatementsNode, defaulting to NonBoolean (nil).
    fn classify_stmts_last(&self, stmts: Option<ruby_prism::StatementsNode>, source: &str) -> ReturnKind {
        stmts
            .and_then(|s| { let b: Vec<_> = s.body().iter().collect(); b.into_iter().last() })
            .map_or(ReturnKind::NonBoolean, |last| self.classify_return(&last, source))
    }

    fn classify_return(&self, node: &Node, source: &str) -> ReturnKind {
        match node {
            Node::ReturnNode { .. } => {
                let ret = node.as_return_node().unwrap();
                match ret.arguments() {
                    Some(args) => {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if arg_list.len() == 1 {
                            self.classify_return(&arg_list[0], source)
                        } else {
                            ReturnKind::NonBoolean
                        }
                    }
                    None => ReturnKind::NonBoolean,
                }
            }

            Node::TrueNode { .. } | Node::FalseNode { .. } => ReturnKind::Boolean,

            Node::NilNode { .. } | Node::IntegerNode { .. } | Node::FloatNode { .. }
            | Node::RationalNode { .. } | Node::ImaginaryNode { .. }
            | Node::StringNode { .. } | Node::InterpolatedStringNode { .. }
            | Node::SymbolNode { .. } | Node::InterpolatedSymbolNode { .. }
            | Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
            | Node::ArrayNode { .. } | Node::HashNode { .. } | Node::RangeNode { .. }
            | Node::SelfNode { .. } | Node::LambdaNode { .. }
            | Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. }
            | Node::DefinedNode { .. } => ReturnKind::NonBoolean,

            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let name = String::from_utf8_lossy(call.name().as_slice()).to_string();
                if name == "!" || is_comparison_method(&name) {
                    return ReturnKind::Boolean;
                }
                if name.ends_with('?') {
                    if self.wayward_predicates.iter().any(|w| w == &name) {
                        return ReturnKind::Unknown;
                    }
                    return ReturnKind::Boolean;
                }
                ReturnKind::Unknown
            }

            Node::AndNode { .. } => {
                let n = node.as_and_node().unwrap();
                combine_and_or(self.classify_return(&n.left(), source), self.classify_return(&n.right(), source))
            }
            Node::OrNode { .. } => {
                let n = node.as_or_node().unwrap();
                combine_and_or(self.classify_return(&n.left(), source), self.classify_return(&n.right(), source))
            }

            Node::IfNode { .. } => self.classify_if_chain(node, source),
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                let then_kind = self.classify_stmts_last(n.statements(), source);
                let else_kind = n.else_clause()
                    .and_then(|ec| ec.statements())
                    .map_or(ReturnKind::NonBoolean, |stmts| self.classify_stmts_last(Some(stmts), source));
                combine_branches(then_kind, else_kind, self.mode)
            }

            Node::WhileNode { .. } => self.classify_stmts_last(node.as_while_node().unwrap().statements(), source),
            Node::UntilNode { .. } => self.classify_stmts_last(node.as_until_node().unwrap().statements(), source),

            Node::CaseNode { .. } => self.classify_case_branches(node, source),
            Node::CaseMatchNode { .. } => self.classify_case_match_branches(node, source),

            Node::BeginNode { .. } => self.classify_stmts_last(node.as_begin_node().unwrap().statements(), source),

            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                paren.body().map_or(ReturnKind::NonBoolean, |b| self.classify_return(&b, source))
            }

            Node::StatementsNode { .. } => self.classify_stmts_last(Some(node.as_statements_node().unwrap()), source),

            Node::LocalVariableReadNode { .. } | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. } | Node::GlobalVariableReadNode { .. }
            | Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }
            | Node::SuperNode { .. } | Node::ForwardingSuperNode { .. }
            | Node::YieldNode { .. } => ReturnKind::Unknown,

            _ => ReturnKind::Unknown,
        }
    }

    /// Classify an if/elsif/else chain by walking the subsequent chain.
    fn classify_if_chain(&self, node: &Node, source: &str) -> ReturnKind {
        let if_node = node.as_if_node().unwrap();
        let then_kind = self.classify_stmts_last(if_node.statements(), source);

        let else_kind = match if_node.subsequent() {
            Some(sub) => match &sub {
                Node::ElseNode { .. } => {
                    let ec = sub.as_else_node().unwrap();
                    self.classify_stmts_last(ec.statements(), source)
                }
                Node::IfNode { .. } => self.classify_if_chain(&sub, source),
                _ => self.classify_return(&sub, source),
            },
            None => ReturnKind::NonBoolean,
        };

        combine_branches(then_kind, else_kind, self.mode)
    }

    /// Classify case/when branches, folding results.
    fn classify_case_branches(&self, node: &Node, source: &str) -> ReturnKind {
        let case_node = node.as_case_node().unwrap();
        let mut result: Option<ReturnKind> = None;

        for condition in case_node.conditions().iter() {
            if let Node::WhenNode { .. } = &condition {
                let kind = self.classify_stmts_last(condition.as_when_node().unwrap().statements(), source);
                result = Some(result.map_or(kind, |prev| combine_branches(prev, kind, self.mode)));
            }
        }

        let else_kind = case_node.else_clause()
            .map_or(ReturnKind::NonBoolean, |ec| self.classify_stmts_last(ec.statements(), source));

        result.map_or(else_kind, |prev| combine_branches(prev, else_kind, self.mode))
    }

    /// Classify case/in (pattern matching) branches, folding results.
    fn classify_case_match_branches(&self, node: &Node, source: &str) -> ReturnKind {
        let case_match = node.as_case_match_node().unwrap();
        let mut result: Option<ReturnKind> = None;

        for condition in case_match.conditions().iter() {
            if let Node::InNode { .. } = &condition {
                let kind = self.classify_stmts_last(condition.as_in_node().unwrap().statements(), source);
                result = Some(result.map_or(kind, |prev| combine_branches(prev, kind, self.mode)));
            }
        }

        let else_kind = case_match.else_clause()
            .map_or(ReturnKind::NonBoolean, |ec| self.classify_stmts_last(ec.statements(), source));

        result.map_or(else_kind, |prev| combine_branches(prev, else_kind, self.mode))
    }

    fn collect_returns(&self, body_node: &Node, source: &str) -> (Vec<ReturnKind>, Option<ReturnKind>) {
        let mut explicit_returns = Vec::new();

        if let Some(stmts) = body_node.as_statements_node() {
            let body: Vec<_> = stmts.body().iter().collect();
            for stmt in &body {
                self.collect_explicit_returns(stmt, source, &mut explicit_returns);
            }
            let implicit = body.last()
                .filter(|last| !matches!(last, Node::ReturnNode { .. }))
                .map(|last| self.classify_return(last, source));
            return (explicit_returns, implicit);
        }

        self.collect_explicit_returns(body_node, source, &mut explicit_returns);
        let implicit = if matches!(body_node, Node::ReturnNode { .. }) {
            None
        } else {
            Some(self.classify_return(body_node, source))
        };
        (explicit_returns, implicit)
    }

    /// Helper: recurse into statements of a node to collect explicit returns.
    fn recurse_stmts(&self, stmts: Option<ruby_prism::StatementsNode>, source: &str, returns: &mut Vec<ReturnKind>) {
        if let Some(s) = stmts {
            for stmt in s.body().iter() {
                self.collect_explicit_returns(&stmt, source, returns);
            }
        }
    }

    fn collect_explicit_returns(&self, node: &Node, source: &str, returns: &mut Vec<ReturnKind>) {
        match node {
            Node::ReturnNode { .. } => {
                let ret = node.as_return_node().unwrap();
                let kind = match ret.arguments() {
                    Some(args) => {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if arg_list.len() == 1 {
                            self.classify_return(&arg_list[0], source)
                        } else {
                            ReturnKind::NonBoolean
                        }
                    }
                    None => ReturnKind::NonBoolean,
                };
                returns.push(kind);
            }
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                self.recurse_stmts(if_node.statements(), source, returns);
                if let Some(sub) = if_node.subsequent() {
                    self.collect_explicit_returns(&sub, source, returns);
                }
            }
            Node::ElseNode { .. } => self.recurse_stmts(node.as_else_node().unwrap().statements(), source, returns),
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                self.recurse_stmts(n.statements(), source, returns);
                if let Some(ec) = n.else_clause() {
                    self.recurse_stmts(ec.statements(), source, returns);
                }
            }
            Node::WhileNode { .. } => self.recurse_stmts(node.as_while_node().unwrap().statements(), source, returns),
            Node::UntilNode { .. } => self.recurse_stmts(node.as_until_node().unwrap().statements(), source, returns),
            Node::CaseNode { .. } => {
                let case_node = node.as_case_node().unwrap();
                for condition in case_node.conditions().iter() {
                    self.collect_explicit_returns(&condition, source, returns);
                }
                if let Some(ec) = case_node.else_clause() {
                    self.recurse_stmts(ec.statements(), source, returns);
                }
            }
            Node::WhenNode { .. } => self.recurse_stmts(node.as_when_node().unwrap().statements(), source, returns),
            Node::BeginNode { .. } => self.recurse_stmts(node.as_begin_node().unwrap().statements(), source, returns),
            Node::RescueNode { .. } => {
                let rescue = node.as_rescue_node().unwrap();
                self.recurse_stmts(rescue.statements(), source, returns);
                for exception in rescue.exceptions().iter() {
                    self.collect_explicit_returns(&exception, source, returns);
                }
                if let Some(subsequent) = rescue.subsequent() {
                    self.recurse_stmts(subsequent.statements(), source, returns);
                }
            }
            Node::StatementsNode { .. } => self.recurse_stmts(Some(node.as_statements_node().unwrap()), source, returns),
            _ => {}
        }
    }

    fn check_method(
        &self,
        method_name: &str,
        name_start_offset: usize,
        name_end_offset: usize,
        body: Option<Node>,
        source: &str,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if method_name == "initialize" || OPERATOR_METHODS.contains(&method_name) {
            return vec![];
        }
        if self.is_allowed_method(method_name) {
            return vec![];
        }
        if self.allow_bang_methods && method_name.ends_with('!') {
            return vec![];
        }

        let body_node = match body {
            Some(b) => b,
            None => return vec![],
        };
        if self.is_body_empty(&body_node) {
            return vec![];
        }

        let has_question_mark = method_name.ends_with('?');
        let (explicit_returns, implicit_return) = self.collect_returns(&body_node, source);
        let classification = self.determine_classification(&explicit_returns, implicit_return);

        let msg = match classification {
            MethodClassification::Predicate if !has_question_mark => MSG_PREDICATE,
            MethodClassification::NonPredicate if has_question_mark => MSG_NON_PREDICATE,
            _ => return vec![],
        };

        vec![ctx.offense_with_range(self.name(), msg, self.severity(), name_start_offset, name_end_offset)]
    }

    fn is_body_empty(&self, body: &Node) -> bool {
        if let Some(stmts) = body.as_statements_node() {
            let body_stmts: Vec<_> = stmts.body().iter().collect();
            return body_stmts.is_empty() || (body_stmts.len() == 1 && self.is_empty_parens_tree(&body_stmts[0]));
        }
        false
    }

    fn is_empty_parens_tree(&self, node: &Node) -> bool {
        match node {
            Node::ParenthesesNode { .. } => node.as_parentheses_node().unwrap().body().is_none(),
            Node::CaseMatchNode { .. } => {
                let case_match = node.as_case_match_node().unwrap();
                case_match.conditions().iter().all(|condition| {
                    if let Node::InNode { .. } = &condition {
                        let in_node = condition.as_in_node().unwrap();
                        in_node.statements().map_or(false, |stmts| {
                            let body: Vec<_> = stmts.body().iter().collect();
                            body.len() == 1 && self.is_empty_parens_tree(&body[0])
                        })
                    } else {
                        false
                    }
                })
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
        if all_boolean {
            return MethodClassification::Predicate;
        }

        match self.mode {
            Mode::Conservative => {
                if all_returns.iter().all(|k| *k == ReturnKind::NonBoolean) {
                    MethodClassification::NonPredicate
                } else {
                    MethodClassification::Acceptable
                }
            }
            Mode::Aggressive => {
                if all_returns.iter().any(|k| *k == ReturnKind::NonBoolean) {
                    MethodClassification::NonPredicate
                } else {
                    MethodClassification::Acceptable
                }
            }
        }
    }
}

fn is_comparison_method(name: &str) -> bool {
    matches!(name, "==" | "===" | "!=" | "<" | ">" | "<=" | ">=" | "=~" | "!~" | "match?")
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
        _ if mode == Mode::Aggressive && (a == ReturnKind::NonBoolean || b == ReturnKind::NonBoolean) => {
            ReturnKind::NonBoolean
        }
        _ => ReturnKind::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MethodClassification {
    Predicate,
    NonPredicate,
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
        self.check_method(&method_name, name_loc.start_offset(), name_loc.end_offset(), node.body(), ctx.source, ctx)
    }
}
