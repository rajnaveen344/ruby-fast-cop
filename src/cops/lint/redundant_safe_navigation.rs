//! Lint/RedundantSafeNavigation - Checks for redundant safe navigation calls.
//!
//! Detects unnecessary `&.` on non-nil receivers (constants, self, literals) or
//! on guaranteed instance methods (to_s, to_i, to_f, to_a, to_h), and conversion
//! patterns like `foo&.to_h || {}`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/redundant_safe_navigation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const COP_NAME: &str = "Lint/RedundantSafeNavigation";
const MSG: &str = "Redundant safe navigation detected, use `.` instead.";
const MSG_LITERAL: &str = "Redundant safe navigation with default literal detected.";
const MSG_NON_NIL: &str = "Redundant safe navigation on non-nil receiver (detected by analyzing previous code/method invocations).";

/// Methods that always return an instance (nil.to_s => "", nil.to_i => 0, etc.)
const GUARANTEED_INSTANCE_METHODS: &[&str] = &["to_s", "to_i", "to_f", "to_a", "to_h"];

/// Regex: SNAKE_CASE = /\A[[:digit:][:upper:]_]+\z/
fn is_snake_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Methods defined on NilClass that respond_to? would return true for.
/// This is a subset - we check respond_to?(:method) where method is one of these.
fn is_nil_method(name: &str) -> bool {
    matches!(
        name,
        "to_s" | "to_i" | "to_f" | "to_a" | "to_h" | "to_r" | "to_c"
            | "inspect" | "nil?" | "!" | "&" | "|" | "^"
            | "==" | "===" | "!=" | "hash" | "dup" | "freeze"
            | "frozen?" | "to_enum" | "enum_for"
            | "respond_to?" | "respond_to_missing?"
            | "is_a?" | "kind_of?" | "instance_of?"
            | "class" | "object_id" | "send" | "public_send"
            | "method" | "equal?" | "eql?" | "tap" | "then" | "yield_self"
    )
}

/// Methods defined on Object that respond_to? with those as arg means &. is not redundant
fn is_object_method(name: &str) -> bool {
    matches!(
        name,
        "class" | "object_id" | "send" | "public_send" | "respond_to?"
            | "respond_to_missing?" | "method" | "is_a?" | "kind_of?"
            | "instance_of?" | "equal?" | "eql?" | "hash" | "freeze"
            | "frozen?" | "dup" | "clone" | "tap" | "then" | "yield_self"
            | "itself" | "display" | "inspect" | "to_s"
    )
}

pub struct RedundantSafeNavigation {
    allowed_methods: Vec<String>,
    infer_non_nil_receiver: bool,
    additional_nil_methods: Vec<String>,
}

impl RedundantSafeNavigation {
    pub fn new() -> Self {
        Self {
            allowed_methods: vec!["respond_to?".to_string()],
            infer_non_nil_receiver: false,
            additional_nil_methods: Vec::new(),
        }
    }

    pub fn with_config(
        allowed_methods: Vec<String>,
        infer_non_nil_receiver: bool,
        additional_nil_methods: Vec<String>,
    ) -> Self {
        Self {
            allowed_methods,
            infer_non_nil_receiver,
            additional_nil_methods,
        }
    }
}

impl Default for RedundantSafeNavigation {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for RedundantSafeNavigation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if self.infer_non_nil_receiver {
            // Two-pass approach for InferNonNilReceiver:
            // Pass 1: Collect non-nil evidence (regular dot calls and conditions)
            let mut collector = NonNilCollector {
                ctx,
                additional_nil_methods: &self.additional_nil_methods,
                evidence: Vec::new(),
                scope_depth: 0,
                branch_depth: 0,
            };
            collector.visit_program_node(node);
            let evidence = collector.evidence;

            // Pass 2: Normal visitor with evidence
            let mut visitor = RedundantSafeNavVisitor {
                ctx,
                allowed_methods: &self.allowed_methods,
                infer_non_nil_receiver: true,
                additional_nil_methods: &self.additional_nil_methods,
                offenses: Vec::new(),
                non_nil_evidence: evidence,
            };
            visitor.visit_program_node(node);
            visitor.offenses
        } else {
            let mut visitor = RedundantSafeNavVisitor {
                ctx,
                allowed_methods: &self.allowed_methods,
                infer_non_nil_receiver: false,
                additional_nil_methods: &self.additional_nil_methods,
                offenses: Vec::new(),
                non_nil_evidence: Vec::new(),
            };
            visitor.visit_program_node(node);
            visitor.offenses
        }
    }
}

/// Evidence that a receiver is non-nil
#[derive(Debug, Clone)]
struct NonNilEvidence {
    /// Source text of the receiver (e.g., "foo")
    receiver_src: String,
    /// Byte offset of the evidence (where the regular call or condition is)
    offset: usize,
    /// The scope this evidence is in (def start offset, or 0 for top-level)
    scope_start: usize,
    /// Kind of evidence
    kind: EvidenceKind,
}

#[derive(Debug, Clone)]
enum EvidenceKind {
    /// receiver.method (regular dot call) - guaranteed execution
    RegularCall {
        /// Whether this call is in a condition (if/while/case etc.)
        in_condition: bool,
        /// If in a condition, the range of the body where receiver is guaranteed non-nil
        /// (start_offset, end_offset)
        guaranteed_range: Option<(usize, usize)>,
    },
    /// receiver is a sole condition in if/elsif/while etc.
    SoleCondition {
        /// The range of the body where receiver is guaranteed non-nil
        guaranteed_range: (usize, usize),
    },
}

/// Pass 1 collector: finds all evidence of non-nil receivers
struct NonNilCollector<'a> {
    ctx: &'a CheckContext<'a>,
    additional_nil_methods: &'a [String],
    evidence: Vec<NonNilEvidence>,
    scope_depth: usize,
    /// Track if we're inside a branch body (if/else/when/elsif body)
    /// Calls inside branches are not guaranteed to execute
    branch_depth: usize,
}

impl<'a> NonNilCollector<'a> {
    fn current_scope_start(&self) -> usize {
        // This is approximate - we track scope depth but not exact start
        0 // Will be refined
    }

    fn is_nil_method(&self, name: &str) -> bool {
        name == "nil?" || self.additional_nil_methods.iter().any(|m| m == name)
    }

    /// Check if a call at the given offset is on the RHS of `&&` or `and`
    fn is_on_rhs_of_and(&self, offset: usize) -> bool {
        // Scan backwards from the offset to find `&&` or ` and `
        // This is a heuristic - we look at the source before the receiver
        let before = &self.ctx.source[..offset];
        let trimmed = before.trim_end();
        trimmed.ends_with("&&") || trimmed.ends_with("and")
            || trimmed.ends_with("||") || trimmed.ends_with("or")
    }

    /// Collect evidence from a regular (non-safe) call node.
    /// Only collects when NOT inside a branch body (guaranteed sequential).
    fn collect_from_call(&mut self, node: &ruby_prism::CallNode, scope_start: usize) {
        // Only collect from guaranteed execution positions (not in branch bodies)
        if self.branch_depth > 0 {
            return;
        }

        // Also check if this call is on the RHS of && (not guaranteed to execute)
        // Look backwards in source for `&&` before this call on the same line or nearby
        let call_start = node.location().start_offset();
        if self.is_on_rhs_of_and(call_start) {
            return;
        }

        // Only interested in regular dot calls (not &., not ::)
        let op_loc = match node.call_operator_loc() {
            Some(loc) => loc,
            None => return,
        };
        let op = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
        if op != "." {
            return;
        }

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        // Don't count nil methods
        let method = String::from_utf8_lossy(node.name().as_slice());
        if self.is_nil_method(&method) {
            return;
        }

        let recv_src = &self.ctx.source
            [receiver.location().start_offset()..receiver.location().end_offset()];

        self.evidence.push(NonNilEvidence {
            receiver_src: recv_src.to_string(),
            offset: receiver.location().start_offset(),
            scope_start,
            kind: EvidenceKind::RegularCall {
                in_condition: false,
                guaranteed_range: None,
            },
        });
    }

    fn collect_from_condition_if(&mut self, node: &ruby_prism::IfNode, scope_start: usize) {
        let pred = node.predicate();

        // Check if condition is a sole receiver (e.g., `if foo`)
        // Only a bare variable or constant counts - not a method call
        // Actually, RuboCop checks if the condition is `foo` (identifier) as sole condition
        // and if so, foo&.bar in the if-branch is redundant

        // Check if condition starts with a method call on the receiver
        // e.g., `if foo.bar?` -> foo is non-nil in if-branch AND else-branch
        self.collect_condition_evidence(&pred, node, scope_start);
    }

    fn collect_condition_evidence(
        &mut self,
        condition: &Node,
        if_node: &ruby_prism::IfNode,
        scope_start: usize,
    ) {
        // For `if foo.method` or `if foo.method?` - foo is non-nil in the entire if statement
        // For `if foo` (bare identifier) - foo is non-nil only in the if-branch (not else)
        // For `if foo.method && other` - foo is non-nil (LHS of && is evaluated first)
        // For `if other && foo.method` - NOT guaranteed (short-circuit)

        match condition {
            Node::CallNode { .. } => {
                let call = condition.as_call_node().unwrap();
                if let Some(op_loc) = call.call_operator_loc() {
                    // receiver.method call in condition
                    let op = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
                    if op == "." {
                        if let Some(receiver) = call.receiver() {
                            let method = String::from_utf8_lossy(call.name().as_slice());
                            if !self.is_nil_method(&method) {
                                let recv_src = &self.ctx.source[receiver.location().start_offset()
                                    ..receiver.location().end_offset()];

                                // The entire if/elsif body is guaranteed non-nil
                                let body_range = self.get_if_body_range(if_node);
                                self.evidence.push(NonNilEvidence {
                                    receiver_src: recv_src.to_string(),
                                    offset: receiver.location().start_offset(),
                                    scope_start,
                                    kind: EvidenceKind::RegularCall {
                                        in_condition: true,
                                        guaranteed_range: Some(body_range),
                                    },
                                });
                            }
                        }
                    }
                } else if call.receiver().is_none() {
                    // Bare method call like `if foo` - sole condition
                    // foo is non-nil in the if-branch only
                    let cond_src = &self.ctx.source
                        [condition.location().start_offset()..condition.location().end_offset()];
                    if is_simple_identifier(cond_src) {
                        if let Some(stmts) = if_node.statements() {
                            let body_start = stmts.location().start_offset();
                            let body_end = stmts.location().end_offset();
                            self.evidence.push(NonNilEvidence {
                                receiver_src: cond_src.to_string(),
                                offset: condition.location().start_offset(),
                                scope_start,
                                kind: EvidenceKind::SoleCondition {
                                    guaranteed_range: (body_start, body_end),
                                },
                            });
                        }
                    }
                }
            }
            Node::AndNode { .. } => {
                let and = condition.as_and_node().unwrap();
                // Only LHS of && is guaranteed to execute
                self.collect_condition_evidence(&and.left(), if_node, scope_start);
            }
            Node::ParenthesesNode { .. } => {
                let paren = condition.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if let Node::StatementsNode { .. } = &body {
                        let stmts: Vec<_> = body.as_statements_node().unwrap().body().iter().collect();
                        if stmts.len() == 1 {
                            self.collect_condition_evidence(&stmts[0], if_node, scope_start);
                            return;
                        }
                    } else {
                        self.collect_condition_evidence(&body, if_node, scope_start);
                        return;
                    }
                }
            }
            Node::LocalVariableReadNode { .. } => {
                // Local variable as sole condition - non-nil in if-branch
                let cond_src = &self.ctx.source
                    [condition.location().start_offset()..condition.location().end_offset()];
                if let Some(stmts) = if_node.statements() {
                    let body_start = stmts.location().start_offset();
                    let body_end = stmts.location().end_offset();
                    self.evidence.push(NonNilEvidence {
                        receiver_src: cond_src.to_string(),
                        offset: condition.location().start_offset(),
                        scope_start,
                        kind: EvidenceKind::SoleCondition {
                            guaranteed_range: (body_start, body_end),
                        },
                    });
                }
            }
            _ => {
                // Other identifiers as sole condition
                let cond_src = &self.ctx.source
                    [condition.location().start_offset()..condition.location().end_offset()];
                if is_simple_identifier(cond_src) {
                    if let Some(stmts) = if_node.statements() {
                        let body_start = stmts.location().start_offset();
                        let body_end = stmts.location().end_offset();
                        self.evidence.push(NonNilEvidence {
                            receiver_src: cond_src.to_string(),
                            offset: condition.location().start_offset(),
                            scope_start,
                            kind: EvidenceKind::SoleCondition {
                                guaranteed_range: (body_start, body_end),
                            },
                        });
                    }
                }
            }
        }
    }

    fn collect_evidence_from_when_condition(&mut self, condition: &Node, case_node: &ruby_prism::CaseNode) {
        if let Node::CallNode { .. } = condition {
            let call = condition.as_call_node().unwrap();
            if let Some(op_loc) = call.call_operator_loc() {
                let op = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
                if op == "." {
                    if let Some(receiver) = call.receiver() {
                        let method = String::from_utf8_lossy(call.name().as_slice());
                        if !self.is_nil_method(&method) {
                            let recv_src = &self.ctx.source[receiver.location().start_offset()
                                ..receiver.location().end_offset()];
                            // From this when condition to end of case
                            let body_range = (
                                condition.location().end_offset(),
                                case_node.location().end_offset(),
                            );
                            self.evidence.push(NonNilEvidence {
                                receiver_src: recv_src.to_string(),
                                offset: receiver.location().start_offset(),
                                scope_start: 0,
                                kind: EvidenceKind::RegularCall {
                                    in_condition: true,
                                    guaranteed_range: Some(body_range),
                                },
                            });
                        }
                    }
                }
            }
        }
    }

    fn get_if_body_range(&self, node: &ruby_prism::IfNode) -> (usize, usize) {
        // The body range covers from after the condition to before the end keyword
        let start = node.predicate().location().end_offset();
        let end = node.location().end_offset();
        (start, end)
    }
}

fn is_simple_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
        && s.chars().next().map_or(false, |c| c.is_lowercase() || c == '_')
}

impl Visit<'_> for NonNilCollector<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.collect_from_call(node, 0);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.collect_from_condition_if(node, 0);
        // Visit everything inside with increased branch depth
        self.branch_depth += 1;
        ruby_prism::visit_if_node(self, node);
        self.branch_depth -= 1;
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.branch_depth += 1;
        ruby_prism::visit_unless_node(self, node);
        self.branch_depth -= 1;
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let scope_start = node.location().start_offset();
        let mut inner = NonNilCollector {
            ctx: self.ctx,
            additional_nil_methods: self.additional_nil_methods,
            evidence: Vec::new(),
            scope_depth: self.scope_depth + 1,
            branch_depth: 0,
        };
        ruby_prism::visit_def_node(&mut inner, node);
        for mut ev in inner.evidence {
            ev.scope_start = scope_start;
            self.evidence.push(ev);
        }
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        // For `case foo.method` - foo is non-nil in all branches
        if let Some(pred) = node.predicate() {
            if let Node::CallNode { .. } = &pred {
                let call = pred.as_call_node().unwrap();
                if let Some(op_loc) = call.call_operator_loc() {
                    let op = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
                    if op == "." {
                        if let Some(receiver) = call.receiver() {
                            let method = String::from_utf8_lossy(call.name().as_slice());
                            if !self.is_nil_method(&method) {
                                let recv_src = &self.ctx.source[receiver.location().start_offset()
                                    ..receiver.location().end_offset()];
                                let body_range = (
                                    pred.location().end_offset(),
                                    node.location().end_offset(),
                                );
                                self.evidence.push(NonNilEvidence {
                                    receiver_src: recv_src.to_string(),
                                    offset: receiver.location().start_offset(),
                                    scope_start: 0,
                                    kind: EvidenceKind::RegularCall {
                                        in_condition: true,
                                        guaranteed_range: Some(body_range),
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }

        // Also collect evidence from when conditions
        for cond in node.conditions().iter() {
            if let Node::WhenNode { .. } = &cond {
                let when = cond.as_when_node().unwrap();
                for wc in when.conditions().iter() {
                    self.collect_evidence_from_when_condition(&wc, node);
                }
            }
        }

        // Visit everything inside with increased branch depth
        self.branch_depth += 1;
        ruby_prism::visit_case_node(self, node);
        self.branch_depth -= 1;
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        self.branch_depth += 1;
        ruby_prism::visit_while_node(self, node);
        self.branch_depth -= 1;
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.branch_depth += 1;
        ruby_prism::visit_until_node(self, node);
        self.branch_depth -= 1;
    }

}

struct RedundantSafeNavVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    allowed_methods: &'a [String],
    infer_non_nil_receiver: bool,
    additional_nil_methods: &'a [String],
    offenses: Vec<Offense>,
    non_nil_evidence: Vec<NonNilEvidence>,
}

impl<'a> RedundantSafeNavVisitor<'a> {
    fn is_safe_navigation(&self, node: &ruby_prism::CallNode) -> bool {
        if let Some(op_loc) = node.call_operator_loc() {
            let op = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
            op == "&."
        } else {
            false
        }
    }

    fn check_csend(&mut self, node: &ruby_prism::CallNode) {
        if !self.is_safe_navigation(node) {
            return;
        }

        let dot_loc = node.call_operator_loc().unwrap();

        // Check InferNonNilReceiver first
        if self.infer_non_nil_receiver {
            if let Some(receiver) = node.receiver() {
                if self.cant_be_nil(&receiver, node) {
                    self.offenses.push(self.ctx.offense_with_range(
                        COP_NAME,
                        MSG_NON_NIL,
                        Severity::Warning,
                        dot_loc.start_offset(),
                        dot_loc.end_offset(),
                    ));
                    return;
                }
            }
        }

        // Check receiver type - const, self, literal
        if let Some(receiver) = node.receiver() {
            if self.assume_receiver_instance_exists(&receiver) {
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Warning,
                    dot_loc.start_offset(),
                    dot_loc.end_offset(),
                ));
                return;
            }

            // Check guaranteed instance methods (to_s, to_i, etc.)
            if self.guaranteed_instance(&receiver, node) {
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Warning,
                    dot_loc.start_offset(),
                    dot_loc.end_offset(),
                ));
                return;
            }

            // Check AllowedMethods in condition context
            if self.check_allowed_in_condition(node) {
                // Check respond_to? with nil method arg
                if self.respond_to_nil_method(node) {
                    return;
                }

                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Warning,
                    dot_loc.start_offset(),
                    dot_loc.end_offset(),
                ));
            }
        }
    }

    /// Check `foo&.to_h || {}` patterns in OrNode
    fn check_or_conversion(&mut self, node: &ruby_prism::OrNode) {
        let lhs = node.left();
        let rhs = node.right();

        // Check direct csend patterns: foo&.to_X || default
        if self.is_csend_without_block(&lhs) {
            let send_node = lhs.as_call_node().unwrap();
            let method = String::from_utf8_lossy(send_node.name().as_slice());
            if let Some(default_type) = self.conversion_default_type(&method) {
                if self.matches_default(&rhs, default_type) {
                    let dot_loc = send_node.call_operator_loc().unwrap();
                    let end_offset = node.location().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        COP_NAME,
                        MSG_LITERAL,
                        Severity::Warning,
                        dot_loc.start_offset(),
                        end_offset,
                    ));
                    return;
                }
            }
        }

        // Check block pattern: foo&.to_h { |k, v| ... } || {}
        if let Node::BlockNode { .. } = &lhs {
            let block = lhs.as_block_node().unwrap();
            // block's parent call is accessed by looking at the block itself
            // In Prism, BlockNode doesn't have a .call() - the call is the receiver
            // Actually, blocks wrap CallNodes. We need to check block differently.
            // The block structure in Prism: a BlockNode is a child of a CallNode
            // But here lhs IS the block node. Let me check the Prism API...
            // Actually in `x&.to_h { ... } || {}`, the LHS of || is a CallNode with block.
            // Let me reconsider: the LHS might be a CallNode (which has block).
            let _ = block;
        }

        // Check CallNode with block: foo&.to_h { |k, v| ... } || {}
        if let Node::CallNode { .. } = &lhs {
            let call = lhs.as_call_node().unwrap();
            if call.block().is_some() && self.is_safe_navigation(&call) {
                let method = String::from_utf8_lossy(call.name().as_slice());
                if let Some(default_type) = self.conversion_default_type(&method) {
                    if self.matches_default(&rhs, default_type) {
                        let dot_loc = call.call_operator_loc().unwrap();
                        let end_offset = node.location().end_offset();
                        self.offenses.push(self.ctx.offense_with_range(
                            COP_NAME,
                            MSG_LITERAL,
                            Severity::Warning,
                            dot_loc.start_offset(),
                            end_offset,
                        ));
                    }
                }
            }
        }
    }

    fn is_csend_without_block(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            self.is_safe_navigation(&call) && call.block().is_none()
        } else {
            false
        }
    }

    fn conversion_default_type(&self, method: &str) -> Option<DefaultType> {
        match method {
            "to_h" => Some(DefaultType::EmptyHash),
            "to_a" => Some(DefaultType::EmptyArray),
            "to_i" => Some(DefaultType::ZeroInt),
            "to_f" => Some(DefaultType::ZeroFloat),
            "to_s" => Some(DefaultType::EmptyString),
            _ => None,
        }
    }

    fn matches_default(&self, node: &Node, default_type: DefaultType) -> bool {
        match default_type {
            DefaultType::EmptyHash => {
                if let Node::HashNode { .. } = node {
                    let hash = node.as_hash_node().unwrap();
                    hash.elements().iter().count() == 0
                } else {
                    false
                }
            }
            DefaultType::EmptyArray => {
                if let Node::ArrayNode { .. } = node {
                    let arr = node.as_array_node().unwrap();
                    arr.elements().iter().count() == 0
                } else {
                    false
                }
            }
            DefaultType::ZeroInt => {
                if let Node::IntegerNode { .. } = node {
                    let src = &self.ctx.source
                        [node.location().start_offset()..node.location().end_offset()];
                    src == "0"
                } else {
                    false
                }
            }
            DefaultType::ZeroFloat => {
                if let Node::FloatNode { .. } = node {
                    let src = &self.ctx.source
                        [node.location().start_offset()..node.location().end_offset()];
                    src == "0.0"
                } else {
                    false
                }
            }
            DefaultType::EmptyString => {
                if let Node::StringNode { .. } = node {
                    let s = node.as_string_node().unwrap();
                    s.unescaped().is_empty()
                } else {
                    false
                }
            }
        }
    }

    /// Check if receiver is a constant (CamelCase, not SNAKE_CASE), self, or literal (not nil)
    fn assume_receiver_instance_exists(&self, receiver: &Node) -> bool {
        match receiver {
            Node::ConstantReadNode { .. } => {
                let name = String::from_utf8_lossy(
                    receiver.as_constant_read_node().unwrap().name().as_slice(),
                );
                !is_snake_case(&name)
            }
            Node::ConstantPathNode { .. } => {
                // For namespaced constants like FOO::Bar, check the rightmost part
                let path = receiver.as_constant_path_node().unwrap();
                let name = String::from_utf8_lossy(path.name().unwrap().as_slice());
                !is_snake_case(&name)
            }
            Node::SelfNode { .. } => true,
            Node::NilNode { .. } => false,
            // Literals (not nil) - string, integer, float, array, hash, symbol, regex, true, false
            Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::SymbolNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. } => true,
            _ => false,
        }
    }

    /// Check if receiver is a call to a guaranteed instance method (to_s, to_i, etc.)
    /// where the receiver of THAT call uses `.` (not `&.`)
    fn guaranteed_instance(&self, receiver: &Node, _node: &ruby_prism::CallNode) -> bool {
        // Check if receiver is a regular call (not &.) to a GUARANTEED_INSTANCE_METHOD
        let recv_call = match receiver {
            Node::CallNode { .. } => receiver.as_call_node().unwrap(),
            Node::BlockNode { .. } => {
                // Block wrapping a call - but in Prism blocks don't have .send_node()
                // Instead check the call_node that is the "parent" of the block
                // Actually in Prism, if `foo.to_h { ... }&.keys`, the receiver of `&.keys`
                // is a CallNode (to_h) with a block. Let me check:
                // Actually in Prism's tree, `foo.to_h { ... }&.keys` has:
                // CallNode(name=keys, receiver=CallNode(name=to_h, block=BlockNode(...)))
                // So receiver is CallNode, not BlockNode.
                return false;
            }
            _ => return false,
        };

        // The receiver call must use `.` (not `&.`)
        if let Some(op_loc) = recv_call.call_operator_loc() {
            let op = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
            if op == "&." {
                return false;
            }
        }

        let method = String::from_utf8_lossy(recv_call.name().as_slice());
        GUARANTEED_INSTANCE_METHODS.contains(&method.as_ref())
    }

    /// Check if this csend uses an AllowedMethod in a condition/operator/negation context
    fn check_allowed_in_condition(&self, node: &ruby_prism::CallNode) -> bool {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if !self.allowed_methods.iter().any(|m| m == method.as_ref()) {
            return false;
        }

        // Now check context: must be in a condition, operator keyword, or negation
        // We can't easily check parent in Prism visitor, so we use source heuristic
        // Check if this node is in a condition position by looking at surrounding source
        // This is approximated by checking if we're after `if `, `unless `, `while `, `until `,
        // or after `&&`, `||`, `and`, `or`, or after `!`

        // For accurate detection, let's check by offset analysis
        let start = node.location().start_offset();
        let before = &self.ctx.source[..start];
        let trimmed = before.trim_end();

        // Check condition keywords
        if trimmed.ends_with("if")
            || trimmed.ends_with("unless")
            || trimmed.ends_with("while")
            || trimmed.ends_with("until")
            || trimmed.ends_with("elsif")
        {
            return true;
        }

        // Check logical operators
        if trimmed.ends_with("&&")
            || trimmed.ends_with("||")
            || trimmed.ends_with("and")
            || trimmed.ends_with("or")
        {
            return true;
        }

        // Check negation
        if trimmed.ends_with('!') {
            return true;
        }

        false
    }

    /// Check if this is respond_to? with a nil-safe method argument
    fn respond_to_nil_method(&self, node: &ruby_prism::CallNode) -> bool {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method != "respond_to?" {
            return false;
        }

        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let Some(first_arg) = arg_list.first() {
                if let Node::SymbolNode { .. } = first_arg {
                    let sym = first_arg.as_symbol_node().unwrap();
                    let sym_name = String::from_utf8_lossy(sym.unescaped());
                    // If the symbol refers to a method that nil also responds to,
                    // then &. is NOT redundant (removing it changes behavior)
                    if is_nil_method(&sym_name) || is_object_method(&sym_name) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// InferNonNilReceiver: check if the receiver can't be nil based on evidence
    fn cant_be_nil(&self, receiver: &Node, csend_node: &ruby_prism::CallNode) -> bool {
        let recv_src = &self.ctx.source
            [receiver.location().start_offset()..receiver.location().end_offset()];
        let csend_offset = csend_node.location().start_offset();

        // Find the scope this csend is in
        let csend_scope = self.find_scope_for_offset(csend_offset);

        for ev in &self.non_nil_evidence {
            if ev.receiver_src != recv_src {
                continue;
            }
            // Must be in the same scope
            if ev.scope_start != csend_scope {
                continue;
            }

            match &ev.kind {
                EvidenceKind::RegularCall {
                    in_condition,
                    guaranteed_range,
                } => {
                    if *in_condition {
                        // Evidence is in a condition - csend must be in the guaranteed range
                        if let Some((start, end)) = guaranteed_range {
                            if csend_offset >= *start && csend_offset < *end {
                                return true;
                            }
                        }
                    } else {
                        // Evidence is a regular sequential call - must be before csend
                        if ev.offset < csend_offset {
                            return true;
                        }
                    }
                }
                EvidenceKind::SoleCondition { guaranteed_range } => {
                    let (start, end) = *guaranteed_range;
                    if csend_offset >= start && csend_offset < end {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn find_scope_for_offset(&self, offset: usize) -> usize {
        // Find the innermost scope (def) that contains this offset
        let mut best = 0;
        for ev in &self.non_nil_evidence {
            if ev.scope_start > best && ev.scope_start <= offset {
                best = ev.scope_start;
            }
        }
        // Also check if there are scope boundaries in the source
        let bytes = self.ctx.source.as_bytes();
        let mut i = 0;
        while i < offset {
            if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"def " {
                if i == 0 || bytes[i.saturating_sub(1)] == b'\n' || bytes[i.saturating_sub(1)] == b' ' || bytes[i.saturating_sub(1)] == b'\t' {
                    if i > best {
                        best = i;
                    }
                }
            }
            i += 1;
        }
        best
    }
}

#[derive(Debug, Clone, Copy)]
enum DefaultType {
    EmptyHash,
    EmptyArray,
    ZeroInt,
    ZeroFloat,
    EmptyString,
}

impl Visit<'_> for RedundantSafeNavVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_csend(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        self.check_or_conversion(node);
        ruby_prism::visit_or_node(self, node);
    }
}
