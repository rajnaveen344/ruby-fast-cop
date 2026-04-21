//! Style/GuardClause - Use a guard clause instead of wrapping code inside a conditional.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/style/guard_clause.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::guard_clause::{is_guard_clause, match_terminator};
use crate::offense::{Offense, Severity};
use crate::node_name;
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/GuardClause";

pub struct GuardClause {
    min_body_length: i64,
    allow_consecutive_conditionals: bool,
    max_line_length: Option<usize>,
}

impl Default for GuardClause {
    fn default() -> Self {
        Self {
            min_body_length: 1,
            allow_consecutive_conditionals: false,
            // RuboCop only enforces too-long-for-single-line when Layout/LineLength
            // is explicitly enabled. Default to None so the modifier-form example
            // is always emitted in tests with no Layout/LineLength config.
            max_line_length: None,
        }
    }
}

impl GuardClause {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        min_body_length: i64,
        allow_consecutive_conditionals: bool,
        max_line_length: Option<usize>,
    ) -> Self {
        Self {
            min_body_length,
            allow_consecutive_conditionals,
            max_line_length,
        }
    }
}

impl Cop for GuardClause {
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
            cop: self,
            offenses: Vec::new(),
            in_assignment_rhs: false,
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a GuardClause,
    offenses: Vec<Offense>,
    /// Whether the current node is the RHS of an assignment (skip if-based offenses here).
    in_assignment_rhs: bool,
}

/// Information about an if/unless node: its keyword, branches, condition.
struct IfInfo<'a> {
    keyword: String,             // "if" or "unless"
    keyword_start: usize,
    keyword_end: usize,
    is_elsif: bool,
    is_modifier: bool,
    is_ternary: bool,
    has_else: bool,              // has an else clause (could be elsif chain for if)
    /// The else node is an `elsif` chain (only applicable to IfNode)
    has_elsif_conditional: bool,
    condition: Node<'a>,
    /// The truthy branch (then) — contents of the `statements` field
    then_statements: Option<ruby_prism::StatementsNode<'a>>,
    /// The falsy branch (else body) — only if has_else and not elsif
    else_statements: Option<ruby_prism::StatementsNode<'a>>,
    node_start: usize,
    node_end: usize,
}

fn inverse_keyword(kw: &str) -> &'static str {
    match kw {
        "if" => "unless",
        "unless" => "if",
        _ => "",
    }
}

impl<'a> Visitor<'a> {
    // ── Public entry points ──

    fn check_def_body(&mut self, body: Option<Node<'a>>) {
        let body = match body {
            Some(b) => b,
            None => return,
        };
        self.check_ending_body(&body);
    }

    /// Recursively check the "ending" body for a trailing if/unless without else.
    fn check_ending_body(&mut self, body: &Node<'a>) {
        match body {
            Node::IfNode { .. } => {
                if let Some(info) = self.if_node_info(&body.as_if_node().unwrap()) {
                    self.check_ending_if(&info, body);
                }
            }
            Node::UnlessNode { .. } => {
                if let Some(info) = self.unless_node_info(&body.as_unless_node().unwrap()) {
                    self.check_ending_if(&info, body);
                }
            }
            Node::BeginNode { .. } => {
                // A method body with multiple statements is a StatementsNode, not BeginNode
                // (BeginNode is explicit begin/rescue). But handle it for completeness.
                let begin = body.as_begin_node().unwrap();
                if let Some(stmts) = begin.statements() {
                    let list: Vec<Node> = stmts.body().iter().collect();
                    if let Some(last) = list.last() {
                        self.check_ending_body(last);
                    }
                }
            }
            Node::StatementsNode { .. } => {
                // Multi-statement body
                let stmts = body.as_statements_node().unwrap();
                let list: Vec<Node> = stmts.body().iter().collect();
                if let Some(last) = list.last() {
                    self.check_ending_body(last);
                }
            }
            _ => {}
        }
    }

    fn check_ending_if(&mut self, info: &IfInfo<'a>, node_ref: &Node<'a>) {
        // accepted_form?(node, ending: true)
        if self.accepted_form(info, true) {
            return;
        }

        // min_body_length check
        if !self.min_body_length_met(info) {
            return;
        }

        // AllowConsecutiveConditionals
        if self.cop.allow_consecutive_conditionals
            && self.is_consecutive_conditional(info.node_start)
        {
            return;
        }

        // Register the offense: "return" + inverse_keyword
        let cond_src = self.src(&info.condition);
        let single_line = format!("return {} {}", inverse_keyword(&info.keyword), cond_src);

        // Then-branch last statement (used for trivial-body check below)
        let then_last = info.then_statements.as_ref().and_then(|s| last_stmt(s));

        let example = if self.too_long_for_single_line(info.keyword_start, &single_line) {
            // Trivial body → skip offense entirely (matches RuboCop's trivial? rule).
            if self.is_trivial(info, &then_last) {
                return;
            }
            // Multi-statement form: `kw cond; return; end`
            format!("{} {}; return; end", inverse_keyword(&info.keyword), cond_src)
        } else {
            single_line
        };
        self.register_offense_keyword(info, &example);

        // Recurse on then branch
        if let Some(then_stmts) = &info.then_statements {
            let list: Vec<Node> = then_stmts.body().iter().collect();
            if let Some(last) = list.last() {
                self.check_ending_body(last);
            }
        }
        let _ = node_ref;
    }

    fn check_on_if(&mut self, info: &IfInfo<'a>) {
        if self.accepted_form(info, false) {
            return;
        }

        // Find guard clause in either branch.
        // Guard clause source is the whole branch expression (e.g. `work and return`).
        let then_last = info.then_statements.as_ref().and_then(|s| last_stmt(s));
        let else_last = info.else_statements.as_ref().and_then(|s| last_stmt(s));

        // Borrow (don't move) so `then_last` stays usable for the trivial check below.
        let (guard_source_node, kw) = if then_last.as_ref().map_or(false, |n| is_guard_clause(n, self.ctx.source)) {
            (then_last.as_ref().unwrap(), info.keyword.clone())
        } else if else_last.as_ref().map_or(false, |n| is_guard_clause(n, self.ctx.source)) {
            (else_last.as_ref().unwrap(), inverse_keyword(&info.keyword).to_string())
        } else {
            return;
        };

        // Build example: e.g., "return if foo", "raise 'x' unless bar"
        let guard_src = self.src(guard_source_node);
        let cond_src = self.src(&info.condition);
        let single_line = format!("{} {} {}", guard_src, kw, cond_src);

        let example = if self.too_long_for_single_line(info.keyword_start, &single_line) {
            // Trivial body check: inner guard clause is the only branch expression and
            // the branch isn't nested if/begin — skip offense per trivial? rule.
            if self.is_trivial(info, &then_last) {
                return;
            }
            // Use the (possibly inverted) `kw` rather than info.keyword: when the
            // guard clause is in the else branch we invert if↔unless.
            format!(
                "{} {}; {}; end",
                kw, cond_src, guard_src
            )
        } else {
            single_line
        };

        self.register_offense_keyword(info, &example);
    }

    fn register_offense_keyword(&mut self, info: &IfInfo<'a>, example: &str) {
        let msg = format!(
            "Use a guard clause (`{}`) instead of wrapping the code inside a conditional expression.",
            example
        );
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            &msg,
            Severity::Convention,
            info.keyword_start,
            info.keyword_end,
        ));
    }

    // ── accepted_form ──

    fn accepted_form(&self, info: &IfInfo, ending: bool) -> bool {
        if self.accepted_if(info, ending) {
            return true;
        }
        // condition.multiline?
        if self.src(&info.condition).contains('\n') {
            return true;
        }
        // parent.assignment?
        if self.in_assignment_rhs {
            return true;
        }
        false
    }

    fn accepted_if(&self, info: &IfInfo, ending: bool) -> bool {
        if info.is_modifier || info.is_ternary || info.is_elsif || info.has_elsif_conditional {
            return true;
        }
        if self.assigned_lvar_used_in_if_branch(info) {
            return true;
        }
        if ending {
            // ending path: accept if has else
            info.has_else
        } else {
            // non-ending: accept if !else or elsif (but elsif already handled above)
            !info.has_else
        }
    }

    fn assigned_lvar_used_in_if_branch(&self, info: &IfInfo) -> bool {
        let assigned = collect_lvasgn_names(&info.condition);
        if assigned.is_empty() {
            return false;
        }
        let stmts = match &info.then_statements {
            Some(s) => s,
            None => return false,
        };
        // Iterate lvar reads in then branch
        let mut used = Vec::new();
        for node in stmts.body().iter() {
            collect_lvar_reads(&node, &mut used);
        }
        assigned.iter().any(|n| used.contains(n))
    }

    // ── min_body_length ──

    fn min_body_length_met(&self, info: &IfInfo) -> bool {
        if info.is_modifier {
            return false;
        }
        // Ruby: (node.loc.end.line - node.loc.keyword.line) > min_body_length
        let end_offset = info.node_end.saturating_sub(3); // "end" keyword starts 3 bytes before
        let end_line = self.ctx.line_of(end_offset);
        let kw_line = self.ctx.line_of(info.keyword_start);
        let diff = end_line as i64 - kw_line as i64;
        let min = if self.cop.min_body_length <= 0 {
            // RuboCop raises in this case; we silently accept as no offense.
            return false;
        } else {
            self.cop.min_body_length
        };
        diff > min
    }

    // ── AllowConsecutiveConditionals ──

    /// Check if the previous sibling statement is also an if/unless.
    /// Since Prism doesn't provide parent/siblings, we scan the source before
    /// the if's line for the previous non-blank code line.
    fn is_consecutive_conditional(&self, node_start: usize) -> bool {
        // Walk backwards looking for the previous non-blank, non-comment line.
        let line_start = self.ctx.line_start(node_start);
        if line_start == 0 {
            return false;
        }
        let before = &self.ctx.source[..line_start];
        // Walk lines backwards
        for line in before.lines().rev() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // This is the previous non-blank line
            return trimmed.starts_with("end")
                || trimmed == "end"
                || trimmed.starts_with("if ")
                || trimmed.starts_with("unless ");
        }
        false
    }

    // ── Helpers ──

    fn src(&self, node: &Node) -> String {
        let loc = node.location();
        self.ctx.source[loc.start_offset()..loc.end_offset()].to_string()
    }

    fn too_long_for_single_line(&self, keyword_start: usize, example: &str) -> bool {
        match self.cop.max_line_length {
            Some(max) => self.ctx.col_of(keyword_start) + example.len() > max,
            None => false,
        }
    }

    /// RuboCop: trivial? means the if_branch is a single "non-if, non-begin" expression.
    fn is_trivial(&self, info: &IfInfo, then_last: &Option<Node<'a>>) -> bool {
        // branches.one? && !if_branch.if_type? && !if_branch.begin_type?
        if info.has_else {
            return false;
        }
        let then_last = match then_last {
            Some(n) => n,
            None => return false,
        };
        // Check that it's not an if or begin inside
        if matches!(then_last, Node::IfNode { .. } | Node::UnlessNode { .. } | Node::BeginNode { .. }) {
            return false;
        }
        // Also check that there's only one statement in the body
        let count = info
            .then_statements
            .as_ref()
            .map(|s| s.body().iter().count())
            .unwrap_or(0);
        count == 1
    }

    // ── IfInfo builders ──

    fn if_node_info(&self, node: &ruby_prism::IfNode<'a>) -> Option<IfInfo<'a>> {
        let kw_loc = node.if_keyword_loc();
        let is_ternary = kw_loc
            .as_ref()
            .map(|loc| self.ctx.src(loc.start_offset(), loc.end_offset()) == "?")
            .unwrap_or(true);
        if is_ternary {
            return Some(IfInfo {
                keyword: String::new(),
                keyword_start: 0,
                keyword_end: 0,
                is_elsif: false,
                is_modifier: false,
                is_ternary: true,
                has_else: node.subsequent().is_some(),
                has_elsif_conditional: false,
                condition: node.predicate(),
                then_statements: node.statements(),
                else_statements: None,
                node_start: node.location().start_offset(),
                node_end: node.location().end_offset(),
            });
        }
        let kw_loc = kw_loc.unwrap();
        let kw_src = self.ctx.src(kw_loc.start_offset(), kw_loc.end_offset());
        let is_elsif = kw_src == "elsif";

        // Detect else chain
        let (has_else, has_elsif_conditional, else_stmts) = match node.subsequent() {
            Some(Node::ElseNode { .. }) => {
                let else_node = node.subsequent().unwrap();
                let en = else_node.as_else_node().unwrap();
                (true, false, en.statements())
            }
            Some(Node::IfNode { .. }) => (true, true, None),
            _ => (false, false, None),
        };

        Some(IfInfo {
            keyword: kw_src.to_string(),
            keyword_start: kw_loc.start_offset(),
            keyword_end: kw_loc.end_offset(),
            is_elsif,
            is_modifier: node.end_keyword_loc().is_none(),
            is_ternary: false,
            has_else,
            has_elsif_conditional,
            condition: node.predicate(),
            then_statements: node.statements(),
            else_statements: else_stmts,
            node_start: node.location().start_offset(),
            node_end: node.location().end_offset(),
        })
    }

    fn unless_node_info(&self, node: &ruby_prism::UnlessNode<'a>) -> Option<IfInfo<'a>> {
        let kw_loc = node.keyword_loc();
        let (has_else, else_stmts) = match node.else_clause() {
            Some(ec) => (true, ec.statements()),
            None => (false, None),
        };
        Some(IfInfo {
            keyword: "unless".to_string(),
            keyword_start: kw_loc.start_offset(),
            keyword_end: kw_loc.end_offset(),
            is_elsif: false,
            is_modifier: node.end_keyword_loc().is_none(),
            is_ternary: false,
            has_else,
            has_elsif_conditional: false,
            condition: node.predicate(),
            then_statements: node.statements(),
            else_statements: else_stmts,
            node_start: node.location().start_offset(),
            node_end: node.location().end_offset(),
        })
    }

    // ── Walkers ──

    fn walk_def(&mut self, def_stmts: Option<ruby_prism::StatementsNode<'a>>) {
        if let Some(stmts) = def_stmts {
            // check_ending_body takes either the single body or the last statement
            let list: Vec<Node> = stmts.body().iter().collect();
            if let Some(last) = list.last() {
                self.check_ending_body(last);
            }
        }
    }
}

fn last_stmt<'a>(stmts: &ruby_prism::StatementsNode<'a>) -> Option<Node<'a>> {
    stmts.body().iter().last()
}

fn collect_lvasgn_names<'pr>(node: &Node<'pr>) -> Vec<String> {
    struct C { names: Vec<String> }
    impl<'v> Visit<'v> for C {
        fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'v>) {
            self.names.push(String::from_utf8_lossy(n.name().as_slice()).to_string());
            ruby_prism::visit_local_variable_write_node(self, n);
        }
    }
    let mut c = C { names: Vec::new() };
    c.visit(node);
    c.names
}

fn collect_lvar_reads<'pr>(node: &Node<'pr>, out: &mut Vec<String>) {
    struct C<'b> { out: &'b mut Vec<String> }
    impl<'v, 'b> Visit<'v> for C<'b> {
        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'v>) {
            self.out.push(String::from_utf8_lossy(n.name().as_slice()).to_string());
        }
    }
    let mut c = C { out };
    c.visit(node);
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'a>) {
        self.walk_def(node.body().and_then(|b| {
            match b {
                Node::StatementsNode { .. } => Some(b.as_statements_node().unwrap()),
                Node::BeginNode { .. } => b.as_begin_node().unwrap().statements(),
                _ => None,
            }
        }));
        // Also check the body as a single expression if not statements
        if let Some(body) = node.body() {
            match body {
                Node::StatementsNode { .. } | Node::BeginNode { .. } => {}
                _ => self.check_ending_body(&body),
            }
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        // Only for define_method / define_singleton_method blocks (handled in call_node).
        // Here we just recurse.
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        let name = node_name!(node);
        if matches!(name.as_ref(), "define_method" | "define_singleton_method") {
            if let Some(block) = node.block() {
                // Extract the block's body
                let block_stmts = match &block {
                    Node::BlockNode { .. } => block.as_block_node().unwrap().body(),
                    _ => None,
                };
                if let Some(body) = block_stmts {
                    match body {
                        Node::StatementsNode { .. } => {
                            let stmts = body.as_statements_node().unwrap();
                            let list: Vec<Node> = stmts.body().iter().collect();
                            if let Some(last) = list.last() {
                                self.check_ending_body(last);
                            }
                        }
                        Node::BeginNode { .. } => {
                            if let Some(stmts) = body.as_begin_node().unwrap().statements() {
                                let list: Vec<Node> = stmts.body().iter().collect();
                                if let Some(last) = list.last() {
                                    self.check_ending_body(last);
                                }
                            }
                        }
                        _ => self.check_ending_body(&body),
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'a>) {
        if let Some(info) = self.if_node_info(node) {
            if !info.is_elsif && !info.is_ternary {
                self.check_on_if(&info);
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'a>) {
        if let Some(info) = self.unless_node_info(node) {
            self.check_on_if(&info);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    // Track assignment RHS state so on_if can check parent.assignment?
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'a>) {
        let was = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        ruby_prism::visit_local_variable_write_node(self, node);
        self.in_assignment_rhs = was;
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'a>) {
        let was = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        ruby_prism::visit_instance_variable_write_node(self, node);
        self.in_assignment_rhs = was;
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'a>) {
        let was = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        ruby_prism::visit_class_variable_write_node(self, node);
        self.in_assignment_rhs = was;
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'a>) {
        let was = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        ruby_prism::visit_global_variable_write_node(self, node);
        self.in_assignment_rhs = was;
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'a>) {
        let was = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        ruby_prism::visit_constant_write_node(self, node);
        self.in_assignment_rhs = was;
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { min_body_length: i64, allow_consecutive_conditionals: bool }
impl Default for Cfg {
    fn default() -> Self { Self { min_body_length: 1, allow_consecutive_conditionals: false } }
}

crate::register_cop!("Style/GuardClause", |cfg| {
    let c: Cfg = cfg.typed("Style/GuardClause");
    let max_line_length = if cfg.is_cop_enabled("Layout/LineLength") {
        cfg.get_cop_config("Layout/LineLength").and_then(|c| c.max).map(|m| m as usize)
    } else {
        None
    };
    Some(Box::new(GuardClause::with_config(
        c.min_body_length, c.allow_consecutive_conditionals, max_line_length,
    )))
});
