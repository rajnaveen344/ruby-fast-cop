//! Layout/EmptyLineAfterGuardClause - Enforces an empty line after guard clauses.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/empty_line_after_guard_clause.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::guard_clause::is_guard_clause;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Layout/EmptyLineAfterGuardClause";
const MSG: &str = "Add empty line after guard clause.";

#[derive(Default)]
pub struct EmptyLineAfterGuardClause;

impl EmptyLineAfterGuardClause {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EmptyLineAfterGuardClause {
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
        // Build right-sibling map by walking parents and recording siblings.
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
            parent_stack: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParentKind {
    Statements,
    Rescue,
    Ensure,
    Else, // inside `if/else` or rescue else
    Other,
}

struct ParentFrame<'pr> {
    kind: ParentKind,
    /// For Statements parents: the sibling list.
    siblings: Vec<Node<'pr>>,
    /// Index of the current child we're visiting (so right_sibling = siblings[index+1]).
    index: usize,
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    parent_stack: Vec<ParentFrame<'a>>,
}

impl<'a> Visitor<'a> {
    fn check_if_or_unless_modifier(&mut self, info: &IfInfo<'a>) {
        // Must have a guard clause: `node.if_branch&.guard_clause?`
        // For modifier form, the if_branch is the `then` body — a single expression.
        if !info.has_guard_clause {
            return;
        }

        // next_line_rescue_or_ensure?  parent is nil/rescue/ensure
        if self.next_line_rescue_or_ensure() {
            return;
        }

        // next_sibling_parent_empty_or_else? (next sibling's parent is if/else → accepted)
        if self.next_sibling_parent_empty_or_else() {
            return;
        }

        // next_sibling_empty_or_guard_clause?
        if self.next_sibling_empty_or_guard_clause(info) {
            return;
        }

        // multiple_statements_on_line?
        if self.multiple_statements_on_line(info) {
            return;
        }

        // Handle heredoc case: find last heredoc argument in the branch
        if info.is_modifier {
            if let Some((heredoc_last_line, heredoc_end_loc)) = self.last_heredoc_info(info) {
                // Check if the line after the heredoc end is blank/allowed directive
                if self.next_line_empty_or_allowed_directive(heredoc_last_line) {
                    return;
                }
                // Offense at heredoc_end location
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Convention,
                    heredoc_end_loc.0,
                    heredoc_end_loc.1,
                ));
                return;
            }
        }

        // Compute `node.last_line` — the last line of the if/unless (for modifier, it's the
        // line of the modifier itself, unless a heredoc extends it, which is handled above)
        let last_line = self.ctx.line_of(info.node_end.saturating_sub(1));
        if self.next_line_empty_or_allowed_directive(last_line) {
            return;
        }

        // Offense location: for modifier form (no `end`), use the whole node range.
        // For non-modifier form with end keyword, use the `end` location.
        let (off_start, off_end) = if info.is_modifier {
            (info.node_start, info.node_end)
        } else {
            // Use end keyword location if present (always for non-modifier)
            (info.end_keyword_start.unwrap(), info.end_keyword_end.unwrap())
        };
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG,
            Severity::Convention,
            off_start,
            off_end,
        ));
    }

    // ── correct_style? helpers ──

    /// next_line_rescue_or_ensure: the if is not in a Statements parent at all, or
    /// its parent is a rescue/ensure clause (meaning it's at the bottom of a begin body).
    fn next_line_rescue_or_ensure(&self) -> bool {
        let parent = match self.parent_stack.last() {
            Some(p) => p,
            None => return true,
        };
        matches!(parent.kind, ParentKind::Rescue | ParentKind::Ensure)
    }

    fn next_sibling_parent_empty_or_else(&self) -> bool {
        // Look at our right sibling; if its "parent" is an if/else, accept.
        // In our structure, the if/unless is the last statement of a branch (then/else),
        // and we check: is it actually the last one?
        // RuboCop's check: `next_sibling = node.right_sibling; return true unless Node`
        //                  `parent = next_sibling.parent; parent&.if_type? && parent.else?`
        // We approximate: if we have no right sibling and our parent is an if/else
        // branch's Statements, accept.
        let parent = match self.parent_stack.last() {
            Some(p) => p,
            None => return false,
        };
        if parent.kind != ParentKind::Statements {
            return false;
        }
        // Right sibling exists?
        if parent.index + 1 < parent.siblings.len() {
            return false; // has a right sibling
        }
        // No right sibling: check if this Statements is inside an if/else branch.
        // Walk one more up: if the Statements parent is part of an if (as else), accept.
        if self.parent_stack.len() >= 2 {
            let grand = &self.parent_stack[self.parent_stack.len() - 2];
            if grand.kind == ParentKind::Else {
                return true;
            }
        }
        false
    }

    fn next_sibling_empty_or_guard_clause(&self, info: &IfInfo<'a>) -> bool {
        let parent = match self.parent_stack.last() {
            Some(p) => p,
            None => return true,
        };
        if parent.kind != ParentKind::Statements {
            return true;
        }
        // Right sibling
        let next_idx = parent.index + 1;
        if next_idx >= parent.siblings.len() {
            return true;
        }
        let next = &parent.siblings[next_idx];
        // If the next sibling is also a guard-clause if/unless, accept.
        match next {
            Node::IfNode { .. } => {
                let n = next.as_if_node().unwrap();
                if let Some(stmts) = n.statements() {
                    if let Some(first) = stmts.body().iter().next() {
                        return is_guard_clause(&first, self.ctx.source);
                    }
                }
                false
            }
            Node::UnlessNode { .. } => {
                let n = next.as_unless_node().unwrap();
                if let Some(stmts) = n.statements() {
                    if let Some(first) = stmts.body().iter().next() {
                        return is_guard_clause(&first, self.ctx.source);
                    }
                }
                false
            }
            _ => {
                let _ = info;
                false
            }
        }
    }

    fn multiple_statements_on_line(&self, info: &IfInfo<'a>) -> bool {
        // Check if the if and its right sibling are on the same line.
        let parent = match self.parent_stack.last() {
            Some(p) => p,
            None => return false,
        };
        if parent.kind != ParentKind::Statements {
            return false;
        }
        let next_idx = parent.index + 1;
        if next_idx >= parent.siblings.len() {
            return false;
        }
        let next = &parent.siblings[next_idx];
        let if_last_line = self.ctx.line_of(info.node_end.saturating_sub(1));
        let next_start_line = self.ctx.line_of(next.location().start_offset());
        if_last_line == next_start_line
    }

    // ── next_line_empty_or_allowed_directive? ──

    fn next_line_empty_or_allowed_directive(&self, line_num: usize) -> bool {
        if self.next_line_empty(line_num) {
            return true;
        }
        // Check if the next line is an allowed directive (rubocop enable/disable, :nocov:)
        let next_line = line_num + 1;
        if self.next_line_is_allowed_directive(next_line)
            && self.next_line_empty(next_line)
        {
            return true;
        }
        false
    }

    fn next_line_empty(&self, line_num: usize) -> bool {
        // Check if line `line_num + 1` is blank.
        // 1-indexed line numbers. Line `line_num + 1` is the one after `line_num`.
        // If no such line exists, consider it "empty" (acceptable) since it's end-of-file.
        let next = line_num + 1;
        match get_line_at(self.ctx.source, next) {
            Some(text) => text.trim().is_empty(),
            None => true,
        }
    }

    fn next_line_is_allowed_directive(&self, line_num: usize) -> bool {
        let text = match get_line_at(self.ctx.source, line_num) {
            Some(t) => t,
            None => return false,
        };
        let trimmed = text.trim();
        if !trimmed.starts_with('#') {
            return false;
        }
        // rubocop directives enabled (:enable or :disable)
        if trimmed.contains("rubocop:enable") || trimmed.contains("rubocop:disable") {
            return true;
        }
        // :nocov:
        let body = trimmed.trim_start_matches('#').trim();
        if body == ":nocov:" {
            return true;
        }
        false
    }

    // ── last_heredoc_info ──

    /// If the if/unless modifier has a heredoc anywhere in its body or condition,
    /// returns (last_line_after_heredoc, (heredoc_end_start, heredoc_end_end)).
    fn last_heredoc_info(&self, info: &IfInfo<'a>) -> Option<(usize, (usize, usize))> {
        // Heredocs can live in either the body OR the condition (e.g.
        // `return true if <<~TEXT.length > bar`). Scan both, take the latest.
        let mut best: Option<(usize, usize)> = None;
        if let Some(stmts) = info.then_statements.as_ref() {
            if let Some(first) = stmts.body().iter().next() {
                if let Some(end) = find_last_heredoc_end(&first, self.ctx.source) {
                    best = Some(end);
                }
            }
        }
        if let Some(cond) = info.condition.as_ref() {
            if let Some(end) = find_last_heredoc_end(cond, self.ctx.source) {
                if best.map_or(true, |(s, _)| end.0 > s) {
                    best = Some(end);
                }
            }
        }
        let heredoc_end = best?;
        let hd_line = self.ctx.line_of(heredoc_end.0);
        Some((hd_line, heredoc_end))
    }

    // ── Visitor helpers for managing parent stack ──

    fn walk_statements(&mut self, stmts: &ruby_prism::StatementsNode<'a>) {
        // Collect siblings into the frame (moved). Re-iterate the linked list
        // separately for the actual walk — Node has no Clone so we can't share.
        let siblings: Vec<Node<'a>> = stmts.body().iter().collect();
        self.parent_stack.push(ParentFrame {
            kind: ParentKind::Statements,
            siblings,
            index: 0,
        });
        for (i, child) in stmts.body().iter().enumerate() {
            self.parent_stack.last_mut().unwrap().index = i;
            self.visit_any(&child);
        }
        self.parent_stack.pop();
    }

    fn visit_any(&mut self, node: &Node<'a>) {
        match node {
            Node::IfNode { .. } => self.visit_if_node(&node.as_if_node().unwrap()),
            Node::UnlessNode { .. } => self.visit_unless_node(&node.as_unless_node().unwrap()),
            Node::DefNode { .. } => self.visit_def_node(&node.as_def_node().unwrap()),
            _ => {
                // Recursively walk via Visit trait dispatch
                self.visit(node);
            }
        }
    }
}

// ── IfInfo struct + builders ──

struct IfInfo<'a> {
    keyword: String,
    is_modifier: bool,
    has_guard_clause: bool,
    node_start: usize,
    node_end: usize,
    end_keyword_start: Option<usize>,
    end_keyword_end: Option<usize>,
    then_statements: Option<ruby_prism::StatementsNode<'a>>,
    /// For modifier form: the condition node (so we can scan it for heredocs).
    condition: Option<Node<'a>>,
}

fn build_if_info<'a>(
    node: &ruby_prism::IfNode<'a>,
    source: &str,
) -> IfInfo<'a> {
    let kw = node
        .if_keyword_loc()
        .map(|loc| String::from_utf8_lossy(&source.as_bytes()[loc.start_offset()..loc.end_offset()]).to_string())
        .unwrap_or_default();
    let is_modifier = node.end_keyword_loc().is_none();
    let stmts = node.statements();
    let has_guard_clause = stmts
        .as_ref()
        .and_then(|s| s.body().iter().next())
        .map_or(false, |n| is_guard_clause(&n, source));
    IfInfo {
        keyword: kw,
        is_modifier,
        has_guard_clause,
        node_start: node.location().start_offset(),
        node_end: node.location().end_offset(),
        end_keyword_start: node.end_keyword_loc().map(|l| l.start_offset()),
        end_keyword_end: node.end_keyword_loc().map(|l| l.end_offset()),
        then_statements: stmts,
        condition: Some(node.predicate()),
    }
}

fn build_unless_info<'a>(
    node: &ruby_prism::UnlessNode<'a>,
    source: &str,
) -> IfInfo<'a> {
    let is_modifier = node.end_keyword_loc().is_none();
    let stmts = node.statements();
    let has_guard_clause = stmts
        .as_ref()
        .and_then(|s| s.body().iter().next())
        .map_or(false, |n| is_guard_clause(&n, source));
    IfInfo {
        keyword: "unless".to_string(),
        is_modifier,
        has_guard_clause,
        node_start: node.location().start_offset(),
        node_end: node.location().end_offset(),
        end_keyword_start: node.end_keyword_loc().map(|l| l.start_offset()),
        end_keyword_end: node.end_keyword_loc().map(|l| l.end_offset()),
        then_statements: stmts,
        condition: Some(node.predicate()),
    }
}

// ── Helpers ──

/// Get the text of a 1-indexed line (without trailing newline). None if line doesn't exist.
fn get_line_at(source: &str, line: usize) -> Option<&str> {
    if line == 0 {
        return None;
    }
    let mut current = 1;
    let mut start = 0;
    for (i, &b) in source.as_bytes().iter().enumerate() {
        if current == line {
            // Find the end of this line
            let end = source[start..].find('\n').map_or(source.len(), |p| start + p);
            return Some(&source[start..end]);
        }
        if b == b'\n' {
            current += 1;
            start = i + 1;
        }
    }
    if current == line && start <= source.len() {
        return Some(&source[start..]);
    }
    None
}

/// Find the last heredoc in a node; returns (start_of_heredoc_end_marker, end_of_heredoc_end_marker).
/// This walks the node's call/argument chain to find a heredoc in a raise/call.
fn find_last_heredoc_end<'pr>(
    node: &Node<'pr>,
    source: &str,
) -> Option<(usize, usize)> {
    // Look for interpolated/string nodes that are heredocs (opening starts with `<<`).
    // Since we can't easily detect heredoc via Prism without `.opening_loc()` inspection,
    // we walk the tree and check any StringNode/InterpolatedStringNode/XStringNode/
    // InterpolatedXStringNode.
    struct F<'s> {
        source: &'s str,
        best: Option<(usize, usize)>,
    }
    impl<'v, 's> Visit<'v> for F<'s> {
        fn visit_string_node(&mut self, n: &ruby_prism::StringNode<'v>) {
            if let Some(open) = n.opening_loc() {
                let o_src = &self.source.as_bytes()[open.start_offset()..open.end_offset()];
                if o_src.starts_with(b"<<") {
                    if let Some(close) = n.closing_loc() {
                        let entry = (close.start_offset(), close.end_offset());
                        if self.best.map_or(true, |(s, _)| entry.0 >= s) {
                            self.best = Some(entry);
                        }
                    }
                }
            }
            ruby_prism::visit_string_node(self, n);
        }
        fn visit_interpolated_string_node(&mut self, n: &ruby_prism::InterpolatedStringNode<'v>) {
            if let Some(open) = n.opening_loc() {
                let o_src = &self.source.as_bytes()[open.start_offset()..open.end_offset()];
                if o_src.starts_with(b"<<") {
                    if let Some(close) = n.closing_loc() {
                        let entry = (close.start_offset(), close.end_offset());
                        if self.best.map_or(true, |(s, _)| entry.0 >= s) {
                            self.best = Some(entry);
                        }
                    }
                }
            }
            ruby_prism::visit_interpolated_string_node(self, n);
        }
        fn visit_x_string_node(&mut self, n: &ruby_prism::XStringNode<'v>) {
            // XStringNode opening_loc returns Location directly (not Option).
            let open = n.opening_loc();
            let o_src = &self.source.as_bytes()[open.start_offset()..open.end_offset()];
            if o_src.starts_with(b"<<") {
                let close = n.closing_loc();
                let entry = (close.start_offset(), close.end_offset());
                if self.best.map_or(true, |(s, _)| entry.0 >= s) {
                    self.best = Some(entry);
                }
            }
            ruby_prism::visit_x_string_node(self, n);
        }
        fn visit_interpolated_x_string_node(&mut self, n: &ruby_prism::InterpolatedXStringNode<'v>) {
            let open = n.opening_loc();
            let o_src = &self.source.as_bytes()[open.start_offset()..open.end_offset()];
            if o_src.starts_with(b"<<") {
                let close = n.closing_loc();
                let entry = (close.start_offset(), close.end_offset());
                if self.best.map_or(true, |(s, _)| entry.0 >= s) {
                    self.best = Some(entry);
                }
            }
            ruby_prism::visit_interpolated_x_string_node(self, n);
        }
    }
    let mut f = F { source, best: None };
    f.visit(node);
    f.best
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode<'a>) {
        // Treat the program's top-level statements as a Statements parent
        self.walk_statements(&node.statements());
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'a>) {
        self.walk_statements(node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'a>) {
        // DefNode body is either Statements or BeginNode or a single expression.
        if let Some(body) = node.body() {
            match body {
                Node::StatementsNode { .. } => {
                    self.walk_statements(&body.as_statements_node().unwrap());
                }
                Node::BeginNode { .. } => {
                    self.visit_begin_node(&body.as_begin_node().unwrap());
                }
                _ => {
                    self.visit_any(&body);
                }
            }
        }
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'a>) {
        // Walk the main body as statements
        if let Some(stmts) = node.statements() {
            self.walk_statements(&stmts);
        }
        // Walk rescue clauses: if/guards inside rescue → parent kind is Rescue
        if let Some(rescue) = node.rescue_clause() {
            // Rescue has statements as its body
            // Push Rescue frame
            self.parent_stack.push(ParentFrame {
                kind: ParentKind::Rescue,
                siblings: Vec::new(),
                index: 0,
            });
            if let Some(stmts) = rescue.statements() {
                self.walk_statements(&stmts);
            }
            self.parent_stack.pop();

            // Walk any chained rescues
            let mut next = rescue.subsequent();
            while let Some(r) = next {
                self.parent_stack.push(ParentFrame {
                    kind: ParentKind::Rescue,
                    siblings: Vec::new(),
                    index: 0,
                });
                if let Some(stmts) = r.statements() {
                    self.walk_statements(&stmts);
                }
                self.parent_stack.pop();
                next = r.subsequent();
            }
        }
        // Walk else clause (rescue else)
        if let Some(el) = node.else_clause() {
            if let Some(stmts) = el.statements() {
                self.walk_statements(&stmts);
            }
        }
        // Walk ensure clause
        if let Some(en) = node.ensure_clause() {
            self.parent_stack.push(ParentFrame {
                kind: ParentKind::Ensure,
                siblings: Vec::new(),
                index: 0,
            });
            if let Some(stmts) = en.statements() {
                self.walk_statements(&stmts);
            }
            self.parent_stack.pop();
        }
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'a>) {
        let info = build_if_info(node, self.ctx.source);
        // RuboCop: on_if checks `if_branch&.guard_clause?`
        self.check_if_or_unless_modifier(&info);

        // Recurse into then body
        if let Some(stmts) = node.statements() {
            self.walk_statements(&stmts);
        }
        // Recurse into else/elsif
        if let Some(sub) = node.subsequent() {
            match sub {
                Node::ElseNode { .. } => {
                    self.parent_stack.push(ParentFrame {
                        kind: ParentKind::Else,
                        siblings: Vec::new(),
                        index: 0,
                    });
                    if let Some(stmts) = sub.as_else_node().unwrap().statements() {
                        self.walk_statements(&stmts);
                    }
                    self.parent_stack.pop();
                }
                Node::IfNode { .. } => {
                    // elsif — recurse
                    self.visit_if_node(&sub.as_if_node().unwrap());
                }
                _ => {}
            }
        }
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'a>) {
        let info = build_unless_info(node, self.ctx.source);
        self.check_if_or_unless_modifier(&info);

        if let Some(stmts) = node.statements() {
            self.walk_statements(&stmts);
        }
        if let Some(ec) = node.else_clause() {
            self.parent_stack.push(ParentFrame {
                kind: ParentKind::Else,
                siblings: Vec::new(),
                index: 0,
            });
            if let Some(stmts) = ec.statements() {
                self.walk_statements(&stmts);
            }
            self.parent_stack.pop();
        }
    }
}

crate::register_cop!("Layout/EmptyLineAfterGuardClause", |_cfg| {
    Some(Box::new(EmptyLineAfterGuardClause::new()))
});
