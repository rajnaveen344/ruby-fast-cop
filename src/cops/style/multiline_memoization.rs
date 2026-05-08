//! Style/MultilineMemoization cop
//!
//! Checks wrapping styles for multiline memoization (`||=`).
//! keyword style (default): requires `begin...end`
//! braces style: requires `(...)`

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Keyword,
    Braces,
}

pub struct MultilineMemoization {
    style: Style,
}

impl Default for MultilineMemoization {
    fn default() -> Self {
        Self { style: Style::Keyword }
    }
}

impl MultilineMemoization {
    pub fn new(style: Style) -> Self {
        Self { style }
    }
}

struct MemoVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    style: Style,
}

impl MemoVisitor<'_> {
    fn is_multiline_source(start: usize, end: usize, source: &str) -> bool {
        source[start..end].contains('\n')
    }

    fn check_rhs(&mut self, rhs: Node, node_start: usize, node_end: usize) {
        if !Self::is_multiline_source(rhs.location().start_offset(), rhs.location().end_offset(), self.ctx.source) {
            return;
        }

        let is_bad = match self.style {
            Style::Keyword => {
                // bad: rhs is ParenthesesNode (parenthesized begin)
                matches!(rhs, Node::ParenthesesNode { .. })
            }
            Style::Braces => {
                // bad: rhs is BeginNode (begin...end keyword)
                matches!(rhs, Node::BeginNode { .. })
            }
        };

        if is_bad {
            let msg = match self.style {
                Style::Keyword => "Wrap multiline memoization blocks in `begin` and `end`.",
                Style::Braces => "Wrap multiline memoization blocks in `(` and `)`.",
            };

            let correction = match self.style {
                Style::Keyword => {
                    // rhs is ParenthesesNode: ( → begin, ) → end
                    if let Some(parens) = rhs.as_parentheses_node() {
                        let open = parens.opening_loc();
                        let close = parens.closing_loc();
                        // For multiline: check if content starts on next line (( followed by \n)
                        // or on same line ((bar ||...).
                        let is_multiline = self.ctx.line_of(open.start_offset()) != self.ctx.line_of(close.start_offset());
                        let (begin_repl, end_repl) = if is_multiline {
                            let col = self.ctx.col_of(open.start_offset());
                            let indent = " ".repeat(col);
                            // Check if `(` is immediately followed by `\n` (content on next line)
                            let open_end = open.end_offset();
                            let bytes = self.ctx.source.as_bytes();
                            let next_char_is_newline = bytes.get(open_end) == Some(&b'\n');
                            let begin_repl = if next_char_is_newline {
                                "begin".to_string() // \n already follows
                            } else {
                                "begin\n".to_string() // add newline
                            };
                            // Check if `)` is at start of a line (preceded only by whitespace)
                            let close_start = close.start_offset();
                            let line_start = self.ctx.line_start(close_start);
                            let before_close = &self.ctx.source[line_start..close_start];
                            let close_on_own_line = before_close.chars().all(|c| c == ' ' || c == '\t');
                            let end_repl = if close_on_own_line {
                                "end".to_string() // ) is already at the right column; just replace with end
                            } else {
                                format!("\n{}end", indent)
                            };
                            (begin_repl, end_repl)
                        } else {
                            ("begin".to_string(), "end".to_string())
                        };
                        Some(Correction {
                            edits: vec![
                                Edit { start_offset: close.start_offset(), end_offset: close.end_offset(), replacement: end_repl },
                                Edit { start_offset: open.start_offset(), end_offset: open.end_offset(), replacement: begin_repl },
                            ],
                        })
                    } else { None }
                }
                Style::Braces => {
                    // rhs is BeginNode: begin → (, end → )
                    if let Some(begin_node) = rhs.as_begin_node() {
                        if let (Some(begin_kw), Some(end_kw)) = (begin_node.begin_keyword_loc(), begin_node.end_keyword_loc()) {
                            Some(Correction {
                                edits: vec![
                                    Edit { start_offset: end_kw.start_offset(), end_offset: end_kw.end_offset(), replacement: ")".to_string() },
                                    Edit { start_offset: begin_kw.start_offset(), end_offset: begin_kw.end_offset(), replacement: "(".to_string() },
                                ],
                            })
                        } else { None }
                    } else { None }
                }
            };

            let offense = self.ctx.offense_with_range(
                "Style/MultilineMemoization", msg, Severity::Convention, node_start, node_end,
            );
            self.offenses.push(if let Some(c) = correction { offense.with_correction(c) } else { offense });
        }
    }
}

impl<'a> Visit<'_> for MemoVisitor<'a> {
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }

    fn visit_class_variable_or_write_node(&mut self, node: &ruby_prism::ClassVariableOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_class_variable_or_write_node(self, node);
    }

    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_constant_or_write_node(self, node);
    }

    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode) {
        let rhs = node.value();
        let ns = node.location().start_offset();
        let ne = node.location().end_offset();
        self.check_rhs(rhs, ns, ne);
        ruby_prism::visit_call_or_write_node(self, node);
    }
}

impl Cop for MultilineMemoization {
    fn name(&self) -> &'static str {
        "Style/MultilineMemoization"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MemoVisitor {
            ctx,
            offenses: vec![],
            style: self.style,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/MultilineMemoization", |cfg| {
    let c: Cfg = cfg.typed("Style/MultilineMemoization");
    let style = match c.enforced_style.as_deref() {
        Some("braces") => Style::Braces,
        _ => Style::Keyword,
    };
    Some(Box::new(MultilineMemoization::new(style)))
});
