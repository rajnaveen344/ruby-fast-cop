//! Layout/IndentationConsistency - ensures entities at the same logical depth
//! have the same indentation.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/indentation_consistency.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::access_modifier::is_bare_access_modifier;
use crate::helpers::source::col_at_offset;
use crate::offense::{Location, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Inconsistent indentation detected.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentationConsistencyStyle {
    Normal,
    IndentedInternalMethods,
}

pub struct IndentationConsistency {
    style: IndentationConsistencyStyle,
}

impl IndentationConsistency {
    pub fn new(style: IndentationConsistencyStyle) -> Self {
        Self { style }
    }
}

impl Default for IndentationConsistency {
    fn default() -> Self {
        Self::new(IndentationConsistencyStyle::Normal)
    }
}

impl Cop for IndentationConsistency {
    fn name(&self) -> &'static str {
        "Layout/IndentationConsistency"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: IndentationConsistencyStyle,
    offenses: Vec<Offense>,
}

fn begins_its_line(source: &str, off: usize) -> bool {
    let s = source.as_bytes();
    let mut i = off;
    while i > 0 {
        i -= 1;
        if s[i] == b'\n' {
            return true;
        }
        if s[i] != b' ' && s[i] != b'\t' {
            return false;
        }
    }
    true
}

fn is_bare_am(node: &Node<'_>) -> bool {
    node.as_call_node()
        .map(|c| is_bare_access_modifier(&c))
        .unwrap_or(false)
}

fn base_col_normal(source: &str, children: &[Node<'_>], parent_col: Option<usize>) -> Option<usize> {
    let first = children.first()?;
    if !is_bare_am(first) {
        return None;
    }
    let am_col = col_at_offset(source, first.location().start_offset()) as usize;
    match parent_col {
        None => Some(am_col),
        Some(pc) => if am_col > pc { Some(am_col) } else { None },
    }
}

impl<'a> Visitor<'a> {
    /// Check a list of sibling statements for consistent indentation.
    fn check_group(&mut self, children: &[Node<'_>], base_col: Option<usize>) {
        let mut first: Option<usize> = base_col;
        for child in children {
            let off = child.location().start_offset();
            if !begins_its_line(self.ctx.source, off) {
                continue;
            }
            let col = col_at_offset(self.ctx.source, off) as usize;
            match first {
                None => first = Some(col),
                Some(base) => {
                    if col != base {
                        let end = child.location().end_offset();
                        let loc = Location::from_offsets(self.ctx.source, off, end);
                        self.offenses.push(Offense::new(
                            "Layout/IndentationConsistency",
                            MSG,
                            Severity::Convention,
                            loc,
                            self.ctx.filename,
                        ));
                    }
                }
            }
        }
    }

    /// Check a flat list of children using the current style.
    fn check_statements(&mut self, children: Vec<Node<'_>>, parent_col: Option<usize>) {
        if children.len() < 2 {
            return;
        }

        match self.style {
            IndentationConsistencyStyle::Normal => {
                let base = base_col_normal(self.ctx.source, &children, parent_col);
                let filtered: Vec<Node<'_>> = children
                    .into_iter()
                    .filter(|c| !is_bare_am(c))
                    .collect();
                self.check_group(&filtered, base);
            }
            IndentationConsistencyStyle::IndentedInternalMethods => {
                let mut groups: Vec<Vec<Node<'_>>> = vec![Vec::new()];
                for c in children {
                    if is_bare_am(&c) {
                        groups.push(Vec::new());
                    } else {
                        groups.last_mut().unwrap().push(c);
                    }
                }
                for g in &groups {
                    self.check_group(g, None);
                }
            }
        }
    }
}

fn stmts_from_body<'b>(body: Option<Node<'b>>) -> Vec<Node<'b>> {
    match body {
        None => Vec::new(),
        Some(n) => {
            if let Some(stmts) = n.as_statements_node() {
                stmts.body().iter().collect()
            } else if let Some(begin) = n.as_begin_node() {
                begin.statements().map(|s| s.body().iter().collect()).unwrap_or_default()
            } else {
                vec![n]
            }
        }
    }
}

fn stmts_from_sts<'b>(sts: Option<ruby_prism::StatementsNode<'b>>) -> Vec<Node<'b>> {
    match sts {
        None => Vec::new(),
        Some(s) => s.body().iter().collect(),
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        // Top-level statements: parent_col = None (no outer container).
        let children: Vec<Node<'_>> = node.statements().body().iter().collect();
        self.check_statements(children, None);
        ruby_prism::visit_program_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_body(node.body());
        self.check_statements(children, parent_col);
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_body(node.body());
        self.check_statements(children, parent_col);
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_body(node.body());
        self.check_statements(children, parent_col);
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_body(node.body());
        self.check_statements(children, parent_col);
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_body(node.body());
        self.check_statements(children, parent_col);
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_sts(node.statements());
        self.check_statements(children, parent_col);
        if let Some(sub) = node.subsequent() {
            if let Some(else_n) = sub.as_else_node() {
                let children = stmts_from_sts(else_n.statements());
                self.check_statements(children, parent_col);
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_sts(node.statements());
        self.check_statements(children, parent_col);
        if let Some(else_n) = node.else_clause() {
            let children = stmts_from_sts(else_n.statements());
            self.check_statements(children, parent_col);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_sts(node.statements());
        self.check_statements(children, parent_col);
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_sts(node.statements());
        self.check_statements(children, parent_col);
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_sts(node.statements());
        self.check_statements(children, parent_col);
        ruby_prism::visit_for_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        for cond in node.conditions().iter() {
            if let Some(when_n) = cond.as_when_node() {
                let parent_col = Some(col_at_offset(self.ctx.source, when_n.location().start_offset()) as usize);
                let children = stmts_from_sts(when_n.statements());
                self.check_statements(children, parent_col);
            }
        }
        if let Some(else_n) = node.else_clause() {
            let parent_col = Some(col_at_offset(self.ctx.source, else_n.location().start_offset()) as usize);
            let children = stmts_from_sts(else_n.statements());
            self.check_statements(children, parent_col);
        }
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        let parent_col = Some(col_at_offset(self.ctx.source, node.location().start_offset()) as usize);
        let children = stmts_from_sts(node.statements());
        self.check_statements(children, parent_col);
        ruby_prism::visit_begin_node(self, node);
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}
impl Default for Cfg {
    fn default() -> Self { Self { enforced_style: "normal".into() } }
}

crate::register_cop!("Layout/IndentationConsistency", |cfg| {
    let c: Cfg = cfg.typed("Layout/IndentationConsistency");
    let style = match c.enforced_style.as_str() {
        "indented_internal_methods" => IndentationConsistencyStyle::IndentedInternalMethods,
        _ => IndentationConsistencyStyle::Normal,
    };
    Some(Box::new(IndentationConsistency::new(style)))
});
