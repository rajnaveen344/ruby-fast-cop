//! Style/EmptyElse - Checks for empty else-clauses.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/empty_else.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Redundant `else`-clause.";

/// EnforcedStyle for EmptyElse
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    /// Warn on both empty else and else with nil
    Both,
    /// Warn only on empty else
    Empty,
    /// Warn only on else with nil
    Nil,
}

/// Style/EmptyElse - Checks for empty else-clauses, possibly including
/// comments and/or an explicit `nil` depending on the EnforcedStyle.
pub struct EmptyElse {
    style: EnforcedStyle,
    allow_comments: bool,
}

impl EmptyElse {
    pub fn new(style: EnforcedStyle, allow_comments: bool) -> Self {
        Self {
            style,
            allow_comments,
        }
    }

    fn empty_style(&self) -> bool {
        matches!(self.style, EnforcedStyle::Empty | EnforcedStyle::Both)
    }

    fn nil_style(&self) -> bool {
        matches!(self.style, EnforcedStyle::Nil | EnforcedStyle::Both)
    }

    /// Check if there's a comment between start_offset and end_offset in source.
    fn has_comment_in_range(source: &str, start_offset: usize, end_offset: usize) -> bool {
        let range = &source[start_offset..end_offset];
        let mut in_string = false;
        let mut string_char = 0u8;
        let bytes = range.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if in_string {
                if b == b'\\' {
                    i += 1; // skip escaped char
                } else if b == string_char {
                    in_string = false;
                }
            } else {
                match b {
                    b'\'' | b'"' => {
                        in_string = true;
                        string_char = b;
                    }
                    b'#' => return true,
                    _ => {}
                }
            }
            i += 1;
        }
        false
    }
}

impl Default for EmptyElse {
    fn default() -> Self {
        Self::new(EnforcedStyle::Both, false)
    }
}

impl Cop for EmptyElse {
    fn name(&self) -> &'static str {
        "Style/EmptyElse"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = EmptyElseVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EmptyElseVisitor<'a> {
    cop: &'a EmptyElse,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> EmptyElseVisitor<'a> {
    /// Check an if/unless node's else clause
    fn check_if_unless(&mut self, subsequent: Option<Node>, node_end_offset: usize) {
        if let Some(subseq) = subsequent {
            if let Some(else_node) = subseq.as_else_node() {
                self.check_else_node(&else_node, node_end_offset);
            }
            // If it's another IfNode (elsif), the visitor will visit it naturally
        }
    }

    /// Check a case node's else clause
    fn check_case(&mut self, else_clause: Option<ruby_prism::ElseNode>, node_end_offset: usize) {
        if let Some(else_node) = else_clause {
            self.check_else_node(&else_node, node_end_offset);
        }
    }

    /// Core check on an ElseNode
    fn check_else_node(&mut self, else_node: &ruby_prism::ElseNode, _node_end_offset: usize) {
        let else_loc = else_node.else_keyword_loc();
        let else_start = else_loc.start_offset();
        let else_end = else_loc.end_offset();

        // Check AllowComments: if enabled, check for comments in the else region
        if self.cop.allow_comments {
            // Check from after 'else' keyword to end of the else node
            let else_node_end = else_node.location().end_offset();
            if EmptyElse::has_comment_in_range(self.ctx.source, else_end, else_node_end) {
                return;
            }
        }

        let stmts = else_node.statements();

        // empty_check: else clause with no statements
        if self.cop.empty_style() {
            if stmts.is_none() {
                let offense = self.ctx.offense_with_range(
                    "Style/EmptyElse",
                    MSG,
                    Severity::Convention,
                    else_start,
                    else_end,
                );
                // Correction: remove from else keyword to end keyword
                let correction = Correction::delete(else_start, _node_end_offset);
                self.offenses.push(offense.with_correction(correction));
                return;
            }
        }

        // nil_check: else clause with only `nil`
        if self.cop.nil_style() {
            if let Some(statements) = stmts {
                let body: Vec<_> = statements.body().iter().collect();
                if body.len() == 1 && matches!(body[0], Node::NilNode { .. }) {
                    let offense = self.ctx.offense_with_range(
                        "Style/EmptyElse",
                        MSG,
                        Severity::Convention,
                        else_start,
                        else_end,
                    );
                    // Correction: remove from else keyword to end keyword
                    let correction = Correction::delete(else_start, _node_end_offset);
                    self.offenses.push(offense.with_correction(correction));
                }
            }
        }
    }
}

impl Visit<'_> for EmptyElseVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Skip modifier ifs (no if_keyword_loc) and ternaries
        if let Some(kw_loc) = node.if_keyword_loc() {
            let kw = kw_loc.as_slice();
            // Process if/elsif nodes (not ternary - ternary has no 'if'/'elsif' keyword text)
            if kw == b"if" || kw == b"elsif" {
                // Find the end position for the correction.
                // For the else removal, we need to remove from else keyword to end keyword.
                // The end_keyword_loc is on the outermost if, not the elsif.
                // We use the node's end offset (which for elsif is the parent's end).
                let end_offset = if let Some(end_loc) = node.end_keyword_loc() {
                    end_loc.start_offset()
                } else {
                    node.location().end_offset()
                };
                self.check_if_unless(node.subsequent(), end_offset);
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let end_offset = if let Some(end_loc) = node.end_keyword_loc() {
            end_loc.start_offset()
        } else {
            node.location().end_offset()
        };
        self.check_case(node.else_clause(), end_offset);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        let end_offset = node.end_keyword_loc().start_offset();
        self.check_case(node.else_clause(), end_offset);
        ruby_prism::visit_case_node(self, node);
    }
}
