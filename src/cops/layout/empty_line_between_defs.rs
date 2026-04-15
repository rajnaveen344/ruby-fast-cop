//! Layout/EmptyLineBetweenDefs - class/module/method defs separated by empty lines.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_line_between_defs.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct EmptyLineBetweenDefs {
    pub allow_adjacent_one_line_defs: bool,
    pub empty_line_between_method_defs: bool,
    pub empty_line_between_class_defs: bool,
    pub empty_line_between_module_defs: bool,
    pub def_like_macros: Vec<String>,
    pub number_of_empty_lines_min: u32,
    pub number_of_empty_lines_max: u32,
}

impl Default for EmptyLineBetweenDefs {
    fn default() -> Self {
        Self {
            allow_adjacent_one_line_defs: true,
            empty_line_between_method_defs: true,
            empty_line_between_class_defs: true,
            empty_line_between_module_defs: true,
            def_like_macros: Vec::new(),
            number_of_empty_lines_min: 1,
            number_of_empty_lines_max: 1,
        }
    }
}

impl EmptyLineBetweenDefs {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Cop for EmptyLineBetweenDefs {
    fn name(&self) -> &'static str {
        "Layout/EmptyLineBetweenDefs"
    }
    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { cop: self, ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    cop: &'a EmptyLineBetweenDefs,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_statements(&mut self, stmts_body: Vec<Node>) {
        for pair in stmts_body.windows(2) {
            let prev = &pair[0];
            let cur = &pair[1];
            if self.candidate(prev).is_some() && self.candidate(cur).is_some() {
                self.check_pair(prev, cur);
            }
        }
    }

    /// Returns (type_label) if the node counts as a def/class/module/macro candidate.
    fn candidate(&self, node: &Node) -> Option<&'static str> {
        match node {
            Node::DefNode { .. } => {
                if self.cop.empty_line_between_method_defs {
                    return Some("method");
                }
            }
            Node::ClassNode { .. } => {
                if self.cop.empty_line_between_class_defs {
                    return Some("class");
                }
            }
            Node::ModuleNode { .. } => {
                if self.cop.empty_line_between_module_defs {
                    return Some("module");
                }
            }
            _ => {}
        }
        if self.is_macro_candidate(node) {
            return Some(macro_kind(node));
        }
        None
    }

    fn is_macro_candidate(&self, node: &Node) -> bool {
        if self.cop.def_like_macros.is_empty() {
            return false;
        }
        let call = match node {
            Node::CallNode { .. } => Some(node.as_call_node().unwrap()),
            Node::BlockNode { .. } => None, // blocks in prism are CallNode with block
            _ => None,
        };
        // In Prism, `foo 'x' do ... end` is a CallNode with `block`.
        if let Some(call) = call {
            if call.receiver().is_some() {
                return false;
            }
            let name = node_name!(call);
            return self.cop.def_like_macros.iter().any(|m| *m == name);
        }
        false
    }

    fn check_pair(&mut self, prev: &Node, cur: &Node) {
        let prev_end_line = self.node_end_line(prev);
        let cur_start_line = self.node_start_line(cur);

        // blank lines between = number of lines between defs that are blank.
        let count = self.blank_lines_between(prev_end_line, cur_start_line);

        if self.line_count_allowed(count) {
            return;
        }
        if self.multiple_blank_lines_groups(prev_end_line, cur_start_line) {
            return;
        }
        if self.is_single_line(prev)
            && self.is_single_line(cur)
            && self.cop.allow_adjacent_one_line_defs
        {
            return;
        }

        let (o_start, o_end) = self.def_location(cur);
        let type_label = self.candidate(cur).unwrap_or("method");
        let msg = self.message(type_label, count);
        self.offenses.push(self.ctx.offense_with_range(
            "Layout/EmptyLineBetweenDefs",
            &msg,
            Severity::Convention,
            o_start,
            o_end,
        ));
    }

    fn node_start_line(&self, node: &Node) -> usize {
        let off = match node {
            Node::DefNode { .. } => node.as_def_node().unwrap().def_keyword_loc().start_offset(),
            Node::ClassNode { .. } => {
                node.as_class_node().unwrap().class_keyword_loc().start_offset()
            }
            Node::ModuleNode { .. } => {
                node.as_module_node().unwrap().module_keyword_loc().start_offset()
            }
            _ => node.location().start_offset(),
        };
        self.ctx.line_of(off)
    }

    fn node_end_line(&self, node: &Node) -> usize {
        // Find the last character of node
        let end = node.location().end_offset();
        // end_offset is exclusive, so subtract 1 to get line of last char
        let off = if end > 0 { end - 1 } else { end };
        self.ctx.line_of(off)
    }

    fn is_single_line(&self, node: &Node) -> bool {
        self.node_start_line(node) == self.node_end_line(node)
    }

    /// The "def location": from keyword-start to name-end (or whole source range for macros).
    fn def_location(&self, node: &Node) -> (usize, usize) {
        match node {
            Node::DefNode { .. } => {
                let d = node.as_def_node().unwrap();
                let s = d.def_keyword_loc().start_offset();
                let e = d.name_loc().end_offset();
                (s, e)
            }
            Node::ClassNode { .. } => {
                let c = node.as_class_node().unwrap();
                let s = c.class_keyword_loc().start_offset();
                let e = c.constant_path().location().end_offset();
                (s, e)
            }
            Node::ModuleNode { .. } => {
                let m = node.as_module_node().unwrap();
                let s = m.module_keyword_loc().start_offset();
                let e = m.constant_path().location().end_offset();
                (s, e)
            }
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                // If call has a block, join start of call with end of block-opener (do/{)
                if let Some(block) = c.block() {
                    if let Node::BlockNode { .. } = &block {
                        let b = block.as_block_node().unwrap();
                        let s = c.location().start_offset();
                        let e = b.opening_loc().end_offset();
                        return (s, e);
                    }
                }
                let loc = c.location();
                (loc.start_offset(), loc.end_offset())
            }
            _ => {
                let loc = node.location();
                (loc.start_offset(), loc.end_offset())
            }
        }
    }

    fn blank_lines_between(&self, prev_end_line: usize, cur_start_line: usize) -> u32 {
        // lines between are (prev_end_line+1 .. cur_start_line-1), inclusive.
        // For single-line-on-same-line defs (def a; end; def b; end), prev_end_line == cur_start_line,
        // blank = 0.
        if cur_start_line <= prev_end_line + 1 {
            return 0;
        }
        let mut blanks = 0u32;
        for ln in (prev_end_line + 1)..cur_start_line {
            if self.is_blank_line(ln) {
                blanks += 1;
            }
        }
        blanks
    }

    fn is_blank_line(&self, line_number: usize) -> bool {
        let lines: Vec<&str> = self.ctx.source.lines().collect();
        if line_number == 0 || line_number > lines.len() {
            return false;
        }
        lines[line_number - 1].trim().is_empty()
    }

    fn multiple_blank_lines_groups(&self, prev_end_line: usize, cur_start_line: usize) -> bool {
        // Lines between defs: if last blank line index > first non-blank line index → multiple groups.
        if cur_start_line <= prev_end_line + 1 {
            return false;
        }
        let lines: Vec<&str> = self.ctx.source.lines().collect();
        let range: Vec<&str> = ((prev_end_line + 1)..cur_start_line)
            .filter_map(|i| lines.get(i - 1).copied())
            .collect();
        let mut blank_indices = Vec::new();
        let mut non_blank_indices = Vec::new();
        for (i, l) in range.iter().enumerate() {
            if l.trim().is_empty() {
                blank_indices.push(i);
            } else {
                non_blank_indices.push(i);
            }
        }
        let Some(&blank_max) = blank_indices.iter().max() else { return false };
        let Some(&nb_min) = non_blank_indices.iter().min() else { return false };
        blank_max > nb_min
    }

    fn line_count_allowed(&self, count: u32) -> bool {
        count >= self.cop.number_of_empty_lines_min && count <= self.cop.number_of_empty_lines_max
    }

    fn message(&self, type_label: &str, count: u32) -> String {
        let expected = if self.cop.number_of_empty_lines_min != self.cop.number_of_empty_lines_max {
            format!(
                "{}..{} empty lines",
                self.cop.number_of_empty_lines_min, self.cop.number_of_empty_lines_max
            )
        } else {
            let n = self.cop.number_of_empty_lines_max;
            let unit = if n == 1 { "line" } else { "lines" };
            format!("{} empty {}", n, unit)
        };
        format!(
            "Expected {} between {} definitions; found {}.",
            expected, type_label, count
        )
    }
}

fn macro_kind(node: &Node) -> &'static str {
    match node {
        Node::CallNode { .. } => {
            // Plain `foo :x` → "send"; `foo do ... end` → "block"
            let c = node.as_call_node().unwrap();
            if c.block().is_some() {
                "block"
            } else {
                "send"
            }
        }
        _ => "send",
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        let stmts = node.statements();
        let body: Vec<Node> = stmts.body().iter().collect();
        self.check_statements(body);
        ruby_prism::visit_program_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(body) = node.body() {
            if let Node::StatementsNode { .. } = &body {
                let stmts = body.as_statements_node().unwrap();
                let collected: Vec<Node> = stmts.body().iter().collect();
                self.check_statements(collected);
            }
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if let Some(body) = node.body() {
            if let Node::StatementsNode { .. } = &body {
                let stmts = body.as_statements_node().unwrap();
                let collected: Vec<Node> = stmts.body().iter().collect();
                self.check_statements(collected);
            }
        }
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(stmts) = node.statements() {
            let collected: Vec<Node> = stmts.body().iter().collect();
            self.check_statements(collected);
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(stmts) = node.statements() {
            let collected: Vec<Node> = stmts.body().iter().collect();
            self.check_statements(collected);
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_else_node(&mut self, node: &ruby_prism::ElseNode) {
        if let Some(stmts) = node.statements() {
            let collected: Vec<Node> = stmts.body().iter().collect();
            self.check_statements(collected);
        }
        ruby_prism::visit_else_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if let Some(stmts) = node.statements() {
            let collected: Vec<Node> = stmts.body().iter().collect();
            self.check_statements(collected);
        }
        ruby_prism::visit_unless_node(self, node);
    }
}
