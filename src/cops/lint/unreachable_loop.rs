//! Lint/UnreachableLoop — flags loops that always break/return on the first iteration.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/unreachable_loop.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "This loop will have at most one iteration.";

/// Methods that indicate a block is an iteration loop (union of RuboCop's
/// `enumerable_method?` + `enumerator_method?` + explicit `:loop`).
const LOOP_METHODS: &[&str] = &[
    // Enumerable instance methods
    "all?", "any?", "chunk", "chunk_while", "collect", "collect_concat", "count",
    "cycle", "detect", "drop", "drop_while", "each", "each_cons", "each_entry",
    "each_slice", "each_with_index", "each_with_object", "entries", "filter",
    "filter_map", "find", "find_all", "find_index", "first", "flat_map", "grep",
    "grep_v", "group_by", "include?", "inject", "lazy", "map", "max", "max_by",
    "member?", "min", "min_by", "minmax", "minmax_by", "none?", "one?", "partition",
    "reduce", "reject", "reverse_each", "select", "slice_after", "slice_before",
    "slice_when", "sort", "sort_by", "sum", "take", "take_while", "tally",
    "to_a", "to_h", "uniq", "zip",
    // Enumerator generators on core types
    "downto", "step", "times", "upto",
    // Hash-specific
    "each_key", "each_pair", "each_value",
    // Plain loop
    "loop",
];

fn is_loop_method(name: &str) -> bool {
    LOOP_METHODS.contains(&name)
}

const BREAK_SEND_METHODS: &[&str] = &["raise", "fail", "throw", "exit", "exit!", "abort"];

#[derive(Default)]
pub struct UnreachableLoop {
    allowed_patterns: Vec<String>,
}

impl UnreachableLoop {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(allowed_patterns: Vec<String>) -> Self {
        Self { allowed_patterns }
    }
}

impl Cop for UnreachableLoop {
    fn name(&self) -> &'static str {
        "Lint/UnreachableLoop"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    cop: &'a UnreachableLoop,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Return the list of top-level statements inside the loop's body.
    fn statements<'b>(body: Option<Node<'b>>) -> Vec<Node<'b>> {
        let Some(b) = body else { return vec![] };
        if let Some(stmts) = b.as_statements_node() {
            stmts.body().iter().collect()
        } else if let Some(begin) = b.as_begin_node() {
            begin
                .statements()
                .map(|s| s.body().iter().collect())
                .unwrap_or_default()
        } else {
            vec![b]
        }
    }

    /// Is the receiver source for this loop method call matched by AllowedPatterns?
    fn loop_receiver_allowed(&self, send: &ruby_prism::CallNode) -> bool {
        let loc = send.location();
        let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        is_method_allowed(&[], &self.cop.allowed_patterns, &node_name!(send), Some(src))
    }

    fn is_loop_block(&self, call: &ruby_prism::CallNode) -> bool {
        if call.block().is_none() {
            return false;
        }
        let name = node_name!(call);
        if !is_loop_method(&name) {
            return false;
        }
        !self.loop_receiver_allowed(call)
    }

    /// Is this a "break command" — return, break, or a raise-family send?
    fn is_break_command(node: &Node) -> bool {
        match node {
            Node::ReturnNode { .. } | Node::BreakNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let name = node_name!(call);
                if !BREAK_SEND_METHODS.contains(&name.as_ref()) {
                    return false;
                }
                match call.receiver() {
                    None => true,
                    Some(r) => match &r {
                        Node::ConstantReadNode { .. } => {
                            let c = r.as_constant_read_node().unwrap();
                            node_name!(c) == "Kernel"
                        }
                        Node::ConstantPathNode { .. } => {
                            let cp = r.as_constant_path_node().unwrap();
                            cp.parent().is_none()
                                && String::from_utf8_lossy(
                                    cp.name().map(|n| n.as_slice()).unwrap_or(&[]),
                                ) == "Kernel"
                        }
                        _ => false,
                    },
                }
            }
            _ => false,
        }
    }

    /// Does this expression unconditionally break out of the loop?
    fn is_break_statement(&self, node: &Node) -> bool {
        if Self::is_break_command(node) {
            return true;
        }
        match node {
            Node::BeginNode { .. } => {
                let begin = node.as_begin_node().unwrap();
                let stmts: Vec<Node> = begin
                    .statements()
                    .map(|s| s.body().iter().collect())
                    .unwrap_or_default();
                let Some(idx) = stmts.iter().position(|s| self.is_break_statement(s)) else {
                    return false;
                };
                !preceded_by_continue_in_list(&stmts, idx, &|n| self.is_loop_method_call(n))
            }
            Node::StatementsNode { .. } => {
                // Multi-statement branch body — treat like a begin block.
                let sn = node.as_statements_node().unwrap();
                let stmts: Vec<Node> = sn.body().iter().collect();
                let Some(idx) = stmts.iter().position(|s| self.is_break_statement(s)) else {
                    return false;
                };
                !preceded_by_continue_in_list(&stmts, idx, &|n| self.is_loop_method_call(n))
            }
            Node::IfNode { .. } => self.check_if(&node.as_if_node().unwrap()),
            Node::UnlessNode { .. } => self.check_unless(&node.as_unless_node().unwrap()),
            Node::CaseNode { .. } => self.check_case(&node.as_case_node().unwrap()),
            Node::CaseMatchNode { .. } => self.check_case_match(&node.as_case_match_node().unwrap()),
            _ => false,
        }
    }

    fn check_if(&self, node: &ruby_prism::IfNode) -> bool {
        let Some(if_body) = first_stmt_of(node.statements().map(|s| s.as_node())) else {
            return false;
        };
        let Some(else_clause) = node.subsequent() else {
            return false;
        };
        match &else_clause {
            Node::ElseNode { .. } => {
                let en = else_clause.as_else_node().unwrap();
                let Some(else_branch) = first_stmt_of(en.statements().map(|s| s.as_node())) else {
                    return false;
                };
                self.is_break_statement(&if_body) && self.is_break_statement(&else_branch)
            }
            Node::IfNode { .. } => {
                // elsif — all branches (including trailing else) must break
                self.is_break_statement(&if_body)
                    && self.check_if(&else_clause.as_if_node().unwrap())
            }
            _ => false,
        }
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode) -> bool {
        let Some(if_body) = first_stmt_of(node.statements().map(|s| s.as_node())) else {
            return false;
        };
        let Some(en) = node.else_clause() else {
            return false;
        };
        let Some(else_branch) = first_stmt_of(en.statements().map(|s| s.as_node())) else {
            return false;
        };
        self.is_break_statement(&if_body) && self.is_break_statement(&else_branch)
    }

    fn check_case(&self, node: &ruby_prism::CaseNode) -> bool {
        let Some(en) = node.else_clause() else {
            return false;
        };
        let Some(else_branch) = first_stmt_of(en.statements().map(|s| s.as_node())) else {
            return false;
        };
        if !self.is_break_statement(&else_branch) {
            return false;
        }
        node.conditions().iter().all(|cond| {
            if let Some(when_node) = cond.as_when_node() {
                match first_stmt_of(when_node.statements().map(|s| s.as_node())) {
                    Some(body) => self.is_break_statement(&body),
                    None => false,
                }
            } else {
                false
            }
        })
    }

    fn check_case_match(&self, node: &ruby_prism::CaseMatchNode) -> bool {
        let Some(en) = node.else_clause() else {
            return false;
        };
        let Some(else_branch) = first_stmt_of(en.statements().map(|s| s.as_node())) else {
            return false;
        };
        if !self.is_break_statement(&else_branch) {
            return false;
        }
        node.conditions().iter().all(|cond| {
            if let Some(in_node) = cond.as_in_node() {
                match first_stmt_of(in_node.statements().map(|s| s.as_node())) {
                    Some(body) => self.is_break_statement(&body),
                    None => false,
                }
            } else {
                false
            }
        })
    }

    fn is_loop_method_call(&self, node: &Node) -> bool {
        match node.as_call_node() {
            Some(c) => self.is_loop_block(&c),
            None => false,
        }
    }

    fn check_loop(
        &mut self,
        body: Option<Node>,
        offense_node_start: usize,
        offense_node_end: usize,
    ) {
        let stmts = Self::statements(body);
        let Some(idx) = stmts.iter().position(|s| self.is_break_statement(s)) else {
            return;
        };

        if preceded_by_continue_in_list(&stmts, idx, &|n| self.is_loop_method_call(n)) {
            return;
        }
        if conditional_continue_keyword(&stmts[idx]) {
            return;
        }

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/UnreachableLoop",
            MSG,
            Severity::Warning,
            offense_node_start,
            offense_node_end,
        ));
    }
}

/// Reduce a body expression to its single top-level expression; if the body is a
/// multi-statement StatementsNode, return it as-is (caller handles this via begin path).
fn first_stmt_of<'b>(stmts: Option<Node<'b>>) -> Option<Node<'b>> {
    let s = stmts?;
    if let Some(sn) = s.as_statements_node() {
        let body: Vec<Node> = sn.body().iter().collect();
        match body.len() {
            0 => None,
            1 => Some(body.into_iter().next().unwrap()),
            _ => Some(s),
        }
    } else {
        Some(s)
    }
}

fn preceded_by_continue_in_list(
    list: &[Node],
    target_idx: usize,
    is_loop_method_call: &dyn Fn(&Node) -> bool,
) -> bool {
    for sib in &list[..target_idx] {
        if is_loop_keyword(sib) || is_loop_method_call(sib) {
            continue;
        }
        if contains_continue_keyword(sib) {
            return true;
        }
    }
    false
}

fn is_loop_keyword(node: &Node) -> bool {
    matches!(
        node,
        Node::WhileNode { .. } | Node::UntilNode { .. } | Node::ForNode { .. }
    )
}

/// Deep scan: does `node` (or any descendant) contain `next` or `redo`?
fn contains_continue_keyword(node: &Node) -> bool {
    struct Scanner {
        found: bool,
    }
    impl<'a> Visit<'a> for Scanner {
        fn visit_next_node(&mut self, _n: &ruby_prism::NextNode<'a>) {
            self.found = true;
        }
        fn visit_redo_node(&mut self, _n: &ruby_prism::RedoNode<'a>) {
            self.found = true;
        }
    }
    let mut s = Scanner { found: false };
    s.visit(node);
    s.found
}

/// RuboCop's `conditional_continue_keyword?`: last OR descendant has `next`/`redo` as rhs.
fn conditional_continue_keyword(node: &Node) -> bool {
    struct OrScanner {
        last_rhs_is_continue: bool,
    }
    impl<'a> Visit<'a> for OrScanner {
        fn visit_or_node(&mut self, n: &ruby_prism::OrNode<'a>) {
            // Overwrite on each OR — the last visited wins (post-order traversal: children then self).
            let rhs = n.right();
            self.last_rhs_is_continue =
                matches!(&rhs, Node::NextNode { .. } | Node::RedoNode { .. });
            ruby_prism::visit_or_node(self, n);
        }
    }
    let mut s = OrScanner {
        last_rhs_is_continue: false,
    };
    s.visit(node);
    s.last_rhs_is_continue
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'a>) {
        let loc = node.location();
        let body = node.statements().map(|s| s.as_node());
        self.check_loop(body, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'a>) {
        let loc = node.location();
        let body = node.statements().map(|s| s.as_node());
        self.check_loop(body, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode<'a>) {
        let loc = node.location();
        let body = node.statements().map(|s| s.as_node());
        self.check_loop(body, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_for_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        if self.is_loop_block(node) {
            if let Some(block_node) = node.block().and_then(|b| b.as_block_node()) {
                let body = block_node.body();
                let loc = node.location();
                self.check_loop(body, loc.start_offset(), loc.end_offset());
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

// Merges AllowedPatterns and IgnoredPatterns (legacy alias) into one Vec.
#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_patterns: Vec<String>,
    ignored_patterns: Vec<String>,
}

crate::register_cop!("Lint/UnreachableLoop", |cfg| {
    let c: Cfg = cfg.typed("Lint/UnreachableLoop");
    let mut patterns = c.allowed_patterns;
    patterns.extend(c.ignored_patterns);
    Some(Box::new(UnreachableLoop::with_config(patterns)))
});
