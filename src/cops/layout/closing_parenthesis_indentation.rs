//! Layout/ClosingParenthesisIndentation - indents hanging `)` consistently.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/closing_parenthesis_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::col_at_offset;
use crate::offense::{Location, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_INDENT: &str = "Indent `)` to column %EXP% (not %ACT%)";
const MSG_ALIGN: &str = "Align `)` with `(`.";

pub struct ClosingParenthesisIndentation {
    indentation_width: usize,
}

impl ClosingParenthesisIndentation {
    pub fn new(indentation_width: usize) -> Self {
        Self { indentation_width }
    }
}

impl Default for ClosingParenthesisIndentation {
    fn default() -> Self {
        Self::new(2)
    }
}

impl Cop for ClosingParenthesisIndentation {
    fn name(&self) -> &'static str {
        "Layout/ClosingParenthesisIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            indentation_width: self.indentation_width,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    indentation_width: usize,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn begins_its_line(&self, off: usize) -> bool {
        self.ctx.begins_its_line(off)
    }

    fn line_indentation(&self, line: usize) -> usize {
        // Column of first non-whitespace on 1-indexed line
        let src = self.ctx.source.as_bytes();
        let mut i = 0usize;
        let mut current_line = 1usize;
        while current_line < line && i < src.len() {
            if src[i] == b'\n' {
                current_line += 1;
            }
            i += 1;
        }
        let line_start = i;
        let mut col = 0usize;
        while i < src.len() && (src[i] == b' ' || src[i] == b'\t') {
            col += 1;
            i += 1;
        }
        let _ = line_start;
        col
    }

    /// Column of left paren. elements: flat list of child elements (each Node<'_>).
    fn expected_column(&self, lparen_col: usize, lparen_line: usize, elements: &[Node<'_>]) -> usize {
        if let Some(first) = elements.first() {
            let first_off = first.location().start_offset();
            let first_line = self.ctx.line_of(first_off);
            if first_line > lparen_line {
                // Line break before 1st element — expected is line_indent(first_arg_line) - width
                let source_indent = self.line_indentation(first_line);
                return source_indent.saturating_sub(self.indentation_width);
            }
            if self.all_elements_aligned(elements) {
                return lparen_col;
            }
            return self.line_indentation(first_line);
        }
        // No elements: fallback to lparen_col (but we call `check_no_elements` separately)
        lparen_col
    }

    fn all_elements_aligned(&self, elements: &[Node<'_>]) -> bool {
        // If first element is a hash, compare its child columns.
        let cols: Vec<usize> = if let Some(hash) = elements.first().and_then(|n| n.as_hash_node()) {
            hash.elements()
                .iter()
                .map(|c| col_at_offset(self.ctx.source, c.location().start_offset()) as usize)
                .collect()
        } else if let Some(khash) = elements.first().and_then(|n| n.as_keyword_hash_node()) {
            khash
                .elements()
                .iter()
                .map(|c| col_at_offset(self.ctx.source, c.location().start_offset()) as usize)
                .collect()
        } else {
            elements
                .iter()
                .map(|e| col_at_offset(self.ctx.source, e.location().start_offset()) as usize)
                .collect()
        };
        let first = match cols.first() {
            Some(c) => *c,
            None => return false,
        };
        cols.iter().all(|c| *c == first)
    }

    fn check_elements(&mut self, lparen_loc: &ruby_prism::Location, rparen_loc: &ruby_prism::Location, elements: Vec<Node<'_>>) {
        let rp_off = rparen_loc.start_offset();
        if !self.begins_its_line(rp_off) {
            return;
        }
        let lp_off = lparen_loc.start_offset();
        let lp_col = col_at_offset(self.ctx.source, lp_off) as usize;
        let lp_line = self.ctx.line_of(lp_off);
        let rp_col = col_at_offset(self.ctx.source, rp_off) as usize;

        let expected = self.expected_column(lp_col, lp_line, &elements);
        if expected == rp_col {
            return;
        }

        let msg = if expected == lp_col {
            MSG_ALIGN.to_string()
        } else {
            MSG_INDENT
                .replace("%EXP%", &expected.to_string())
                .replace("%ACT%", &rp_col.to_string())
        };
        let loc = Location::from_offsets(self.ctx.source, rp_off, rparen_loc.end_offset());
        self.offenses.push(Offense::new(
            "Layout/ClosingParenthesisIndentation",
            msg,
            Severity::Convention,
            loc,
            self.ctx.filename,
        ));
    }

    fn check_no_elements(&mut self, node_start_off: usize, lparen_loc: &ruby_prism::Location, rparen_loc: &ruby_prism::Location) {
        let rp_off = rparen_loc.start_offset();
        if !self.begins_its_line(rp_off) {
            return;
        }
        let lp_off = lparen_loc.start_offset();
        let lp_col = col_at_offset(self.ctx.source, lp_off) as usize;
        let lp_line = self.ctx.line_of(lp_off);
        let rp_col = col_at_offset(self.ctx.source, rp_off) as usize;
        let node_col = col_at_offset(self.ctx.source, node_start_off) as usize;

        let line_ind = self.line_indentation(lp_line);
        let candidates = [line_ind, lp_col, node_col];
        if candidates.contains(&rp_col) {
            return;
        }
        let expected = candidates[0];
        let msg = if expected == lp_col {
            MSG_ALIGN.to_string()
        } else {
            MSG_INDENT
                .replace("%EXP%", &expected.to_string())
                .replace("%ACT%", &rp_col.to_string())
        };
        let loc = Location::from_offsets(self.ctx.source, rp_off, rparen_loc.end_offset());
        self.offenses.push(Offense::new(
            "Layout/ClosingParenthesisIndentation",
            msg,
            Severity::Convention,
            loc,
            self.ctx.filename,
        ));
    }

    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let Some(lp) = node.opening_loc() else { return };
        let Some(rp) = node.closing_loc() else { return };
        // Only parens — skip `[` / `{` style closers
        let lp_src = &self.ctx.source[lp.start_offset()..lp.end_offset()];
        if lp_src != "(" {
            return;
        }
        let args: Vec<Node<'_>> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => Vec::new(),
        };
        if args.is_empty() {
            let node_off = node.location().start_offset();
            self.check_no_elements(node_off, &lp, &rp);
        } else {
            self.check_elements(&lp, &rp, args);
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let (Some(lp), Some(rp)) = (node.lparen_loc(), node.rparen_loc()) else {
            ruby_prism::visit_def_node(self, node);
            return;
        };
        let params: Vec<Node<'_>> = if let Some(p) = node.parameters() {
            // Flatten parameters: requireds, optionals, rest, posts, keywords, keyword_rest, block
            let mut v: Vec<Node<'_>> = Vec::new();
            for r in p.requireds().iter() {
                v.push(r);
            }
            for o in p.optionals().iter() {
                v.push(o);
            }
            if let Some(rest) = p.rest() {
                v.push(rest);
            }
            for po in p.posts().iter() {
                v.push(po);
            }
            for kw in p.keywords().iter() {
                v.push(kw);
            }
            if let Some(kwr) = p.keyword_rest() {
                v.push(kwr);
            }
            if let Some(bp) = p.block() {
                // BlockParameterNode — keep by reconstructing as Node via location? For our usage
                // we just need a Node to get start offset, so we fudge: skip since we rarely need it.
                let _ = bp;
            }
            v
        } else {
            Vec::new()
        };
        let node_off = node.location().start_offset();
        if params.is_empty() {
            self.check_no_elements(node_off, &lp, &rp);
        } else {
            self.check_elements(&lp, &rp, params);
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        let lp = node.opening_loc();
        let rp = node.closing_loc();
        // Only `(`
        let lp_src = &self.ctx.source[lp.start_offset()..lp.end_offset()];
        if lp_src != "(" {
            ruby_prism::visit_parentheses_node(self, node);
            return;
        }
        let children: Vec<Node<'_>> = if let Some(body) = node.body() {
            if let Some(stmts) = body.as_statements_node() {
                stmts.body().iter().collect()
            } else {
                vec![body]
            }
        } else {
            Vec::new()
        };
        let node_off = node.location().start_offset();
        if children.is_empty() {
            self.check_no_elements(node_off, &lp, &rp);
        } else {
            self.check_elements(&lp, &rp, children);
        }
        ruby_prism::visit_parentheses_node(self, node);
    }
}
