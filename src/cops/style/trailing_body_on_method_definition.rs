//! Style/TrailingBodyOnMethodDefinition cop
//!
//! Checks for trailing code after the method definition line.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/TrailingBodyOnMethodDefinition";
const MSG: &str = "Place the first line of a multi-line method definition's body on its own line.";

pub struct TrailingBodyOnMethodDefinition {
    indent_width: usize,
}

impl Default for TrailingBodyOnMethodDefinition {
    fn default() -> Self {
        Self { indent_width: 2 }
    }
}

impl TrailingBodyOnMethodDefinition {
    pub fn new(indent_width: usize) -> Self {
        Self { indent_width }
    }
}

impl Cop for TrailingBodyOnMethodDefinition {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = TrailingBodyVisitor {
            ctx,
            indent_width: self.indent_width,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct TrailingBodyVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    indent_width: usize,
    offenses: Vec<Offense>,
}

impl<'a> TrailingBodyVisitor<'a> {
    fn check_def(&mut self, def_start: usize, body: Option<Node>, end_loc: Option<ruby_prism::Location>) {
        let body = match body {
            Some(b) => b,
            None => return,
        };

        let end_keyword_loc = match end_loc {
            Some(e) => e,
            None => {
                // Endless method — skip
                return;
            }
        };

        // Get the first statement of the body
        let (first_start, first_end) = match self.first_statement_offsets(&body) {
            Some(s) => s,
            None => return,
        };

        // The def is multi-line if end keyword is on a different line from def
        let def_line = self.ctx.line_of(def_start);
        let end_line = self.ctx.line_of(end_keyword_loc.start_offset());

        if def_line == end_line {
            // Single-line method — no offense
            return;
        }

        // Check if the first statement is on the same line as def
        let first_stmt_line = self.ctx.line_of(first_start);
        if first_stmt_line != def_line {
            // Body already on its own line — no offense
            return;
        }

        // Build correction
        let correction = trailing_body_correction(
            self.ctx,
            def_start,
            first_start,
            self.indent_width,
        );

        // Trailing body on def line — flag the first statement
        let offense = self.ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, first_start, first_end);
        self.offenses.push(offense.with_correction(correction));
    }

    fn first_statement_offsets(&self, body: &Node) -> Option<(usize, usize)> {
        match body {
            Node::StatementsNode { .. } => {
                let stmts = body.as_statements_node().unwrap();
                let parts: Vec<_> = stmts.body().iter().collect();
                parts.first().map(|n| (n.location().start_offset(), n.location().end_offset()))
            }
            Node::BeginNode { .. } => {
                let begin = body.as_begin_node().unwrap();
                if let Some(stmts) = begin.statements() {
                    let parts: Vec<_> = stmts.body().iter().collect();
                    parts.first().map(|n| (n.location().start_offset(), n.location().end_offset()))
                } else {
                    None
                }
            }
            _ => {
                // Single expression body
                let loc = body.location();
                Some((loc.start_offset(), loc.end_offset()))
            }
        }
    }
}

/// Build the trailing-body correction:
/// - Find any `;` between def header and body start on same line, remove it
/// - Find any EOL comment on the def line, move it before the def
/// - Insert `\n{indent}` before first body statement
pub fn trailing_body_correction(
    ctx: &CheckContext,
    def_start: usize,
    first_body_start: usize,
    indent_width: usize,
) -> Correction {
    let def_col = ctx.col_of(def_start);
    let indent = " ".repeat(def_col + indent_width);
    let source = ctx.source;
    let bytes = source.as_bytes();

    // Find EOL comment on def line (before first_body_start or on the def line)
    // A comment starts with `#` not inside a string — simple scan backwards on the line
    let def_line_start = ctx.line_start(def_start);
    let def_line_end = source[def_line_start..].find('\n')
        .map(|p| def_line_start + p)
        .unwrap_or(source.len());

    // Scan def line for `#` that is a comment (not inside string literal)
    // Simple heuristic: find `#` after the body start that is on the def line
    let comment_start = find_comment_on_line(bytes, first_body_start, def_line_end);

    let mut edits: Vec<Edit> = Vec::new();

    // If there's a comment, move it before the def
    if let Some(cs) = comment_start {
        let comment_text = &source[cs..def_line_end];
        let before_def_indent = " ".repeat(def_col);
        // Insert comment + newline before def_start
        edits.push(Edit {
            start_offset: def_start,
            end_offset: def_start,
            replacement: format!("{comment_text}\n{before_def_indent}"),
        });
        // Remove comment from original position (cs..def_line_end)
        // Keep any space before the comment (RuboCop leaves trailing space)
        let remove_from = cs;
        edits.push(Edit {
            start_offset: remove_from,
            end_offset: def_line_end,
            replacement: String::new(),
        });
    }

    // Find semicolon between last def-header char and first_body_start
    // Scan backwards from first_body_start: skip spaces, check for `;`
    let semicolon_pos = find_semicolon_before_body(bytes, first_body_start);

    if let Some(sc_pos) = semicolon_pos {
        // Remove the semicolon (not the space after it)
        edits.push(Edit {
            start_offset: sc_pos,
            end_offset: sc_pos + 1,
            replacement: String::new(),
        });
    }

    // Insert newline + indent before first body statement
    edits.push(Edit {
        start_offset: first_body_start,
        end_offset: first_body_start,
        replacement: format!("\n{indent}"),
    });

    Correction { edits }
}

/// Find the start of a `#` comment on the line between `from` and `line_end`.
/// Returns None if no comment found.
pub fn find_comment_on_line(bytes: &[u8], from: usize, line_end: usize) -> Option<usize> {
    let mut i = from;
    let mut in_string: Option<u8> = None;
    while i < line_end {
        let b = bytes[i];
        if let Some(quote) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == quote {
                in_string = None;
            }
        } else {
            match b {
                b'\'' | b'"' => in_string = Some(b),
                b'#' => return Some(i),
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Find semicolon immediately before `first_body_start` (scanning backwards, skipping spaces).
pub fn find_semicolon_before_body(bytes: &[u8], first_body_start: usize) -> Option<usize> {
    if first_body_start == 0 {
        return None;
    }
    let mut i = first_body_start - 1;
    // Skip spaces
    while i > 0 && bytes[i] == b' ' {
        i -= 1;
    }
    if bytes[i] == b';' {
        Some(i)
    } else {
        None
    }
}

impl Visit<'_> for TrailingBodyVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let def_start = node.location().start_offset();
        let body = node.body();
        let end_loc = node.end_keyword_loc();
        self.check_def(def_start, body, end_loc);
        ruby_prism::visit_def_node(self, node);
    }

}

crate::register_cop!("Style/TrailingBodyOnMethodDefinition", |cfg| {
    let indent_width = cfg
        .get_cop_config("Layout/IndentationWidth")
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_i64())
        .unwrap_or(2) as usize;
    Some(Box::new(TrailingBodyOnMethodDefinition::new(indent_width)))
});
