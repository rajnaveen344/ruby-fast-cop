//! Style/RescueModifier cop
//!
//! Checks for uses of `rescue` in its modifier form.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Avoid using `rescue` in its modifier form.";

pub struct RescueModifier {
    indent_width: usize,
}

impl RescueModifier {
    pub fn new(indent_width: usize) -> Self {
        Self { indent_width }
    }
}

impl Default for RescueModifier {
    fn default() -> Self {
        Self { indent_width: 2 }
    }
}

impl Cop for RescueModifier {
    fn name(&self) -> &'static str {
        "Style/RescueModifier"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RescueModifierVisitor {
            ctx,
            indent_width: self.indent_width,
            offenses: Vec::new(),
            skip_rescue_at: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Compute the column (0-based) of a byte offset in the source.
fn col_of(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let start = offset.min(bytes.len());
    let mut col = 0usize;
    let mut i = start;
    while i > 0 {
        i -= 1;
        if bytes[i] == b'\n' {
            break;
        }
        col += 1;
    }
    col
}

/// Find the end offset of heredoc terminators in the arguments of an expression node.
/// Scans all StringNode/InterpolatedStringNode in call args (forward, picks last).
fn find_heredoc_end(expr: &Node, _source: &str) -> Option<usize> {
    let call = expr.as_call_node()?;
    let args = call.arguments()?;
    let mut result: Option<usize> = None;
    for arg in args.arguments().iter() {
        if let Some(end) = heredoc_closing_end(&arg) {
            result = Some(end);
        }
    }
    result
}

/// Get the end offset of a heredoc closing terminator from a node.
fn heredoc_closing_end(node: &Node) -> Option<usize> {
    match node {
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            s.closing_loc().map(|cl| cl.end_offset())
        }
        Node::InterpolatedStringNode { .. } => {
            let s = node.as_interpolated_string_node().unwrap();
            s.closing_loc().map(|cl| cl.end_offset())
        }
        _ => None,
    }
}

/// Build correction edits for a RescueModifierNode.
///
/// Transforms `expr rescue handler` → `begin\n{indent}expr\n{offset}rescue\n{indent}handler\n{offset}end`
fn build_rescue_correction(
    node: &ruby_prism::RescueModifierNode,
    source: &str,
    indent_width: usize,
) -> Vec<Edit> {
    let expr = node.expression();
    let handler = node.rescue_expression();

    let node_start = node.location().start_offset();
    let node_end = node.location().end_offset();
    let expr_start = expr.location().start_offset();
    let expr_end = expr.location().end_offset();
    let handler_src = &source[handler.location().start_offset()..handler.location().end_offset()];

    // Compute column of the rescue_modifier node start
    let node_col = col_of(source, node_start);
    let node_offset = " ".repeat(node_col);
    let node_indentation = " ".repeat(node_col + indent_width);

    // Check if expression is an unbracketed array
    let wrap_array = if let Node::ArrayNode { .. } = &expr {
        let arr = expr.as_array_node().unwrap();
        arr.opening_loc().is_none()
    } else {
        false
    };

    // Find heredoc end (insert rescue/end after heredoc terminator if present)
    let after_offset = find_heredoc_end(&expr, source).unwrap_or(expr_end);

    let mut edits = Vec::new();

    if after_offset > expr_end {
        // Heredoc case: expr has heredoc args.
        // Edit 1: insert "begin\n{indent}" before expr_start (replace node_start..expr_start)
        edits.push(Edit {
            start_offset: node_start,
            end_offset: expr_start,
            replacement: format!("begin\n{}", node_indentation),
        });
        // Edit 2: delete " rescue handler" from first line (expr_end..node_end)
        edits.push(Edit {
            start_offset: expr_end,
            end_offset: node_end,
            replacement: "".to_string(),
        });
        // Edit 3: insert rescue clause after heredoc terminator.
        let rescue_clause = format!(
            "{}rescue\n{}{}\n{}end\n",
            node_offset, node_indentation, handler_src, node_offset
        );
        edits.push(Edit {
            start_offset: after_offset,
            end_offset: after_offset,
            replacement: rescue_clause,
        });
    } else if wrap_array {
        // Array case: build a single replacement of the entire node
        let expr_src = &source[expr_start..expr_end];
        let replacement = format!(
            "begin\n{}[{}]\n{}rescue\n{}{}\n{}end",
            node_indentation, expr_src, node_offset, node_indentation, handler_src, node_offset
        );
        edits.push(Edit {
            start_offset: node_start,
            end_offset: node_end,
            replacement,
        });
    } else {
        // Normal case: insert "begin\n{indent}" before expr, replace " rescue handler" with clause
        edits.push(Edit {
            start_offset: node_start,
            end_offset: expr_start,
            replacement: format!("begin\n{}", node_indentation),
        });
        edits.push(Edit {
            start_offset: expr_end,
            end_offset: node_end,
            replacement: format!(
                "\n{}rescue\n{}{}\n{}end",
                node_offset, node_indentation, handler_src, node_offset
            ),
        });
    }

    edits
}

struct RescueModifierVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    indent_width: usize,
    offenses: Vec<Offense>,
    /// Start offsets of RescueModifierNodes already reported via a MultiWriteNode
    skip_rescue_at: Vec<usize>,
}

impl<'a> Visit<'_> for RescueModifierVisitor<'a> {
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        // In Ruby >= 2.6, `a, b = 1, 2 rescue nil` creates a MultiWriteNode where
        // the direct value is a RescueModifierNode. Report offense at MultiWriteNode.
        if self.ctx.ruby_version_at_least(2, 6) {
            let value = node.value();
            if let Some(rescue_mod) = value.as_rescue_modifier_node() {
                // Skip inner rescue modifier (it will be reported here instead)
                let rescue_mod_start = rescue_mod.location().start_offset();
                self.skip_rescue_at.push(rescue_mod_start);
                let start = node.location().start_offset();
                let end = node.location().end_offset();

                let source = self.ctx.source;
                let correction = if self.ctx.ruby_version_at_least(2, 7) {
                    // Ruby >= 2.7: wrap only the inner rescue_modifier (RHS),
                    // leaving the LHS assignment intact.
                    let edits = build_rescue_correction(&rescue_mod, source, self.indent_width);
                    Correction { edits }
                } else {
                    // Ruby 2.6: wrap entire multi_write in begin..rescue..end
                    let node_col = col_of(source, start);
                    let node_offset = " ".repeat(node_col);
                    let node_indentation = " ".repeat(node_col + self.indent_width);

                    let handler = rescue_mod.rescue_expression();
                    let handler_src = &source[handler.location().start_offset()..handler.location().end_offset()];
                    let rescue_mod_expr = rescue_mod.expression();
                    let multi_src = &source[start..rescue_mod_expr.location().end_offset()];

                    let replacement = format!(
                        "begin\n{}{}\n{}rescue\n{}{}\n{}end",
                        node_indentation, multi_src,
                        node_offset,
                        node_indentation, handler_src,
                        node_offset
                    );
                    Correction::replace(start, end, replacement)
                };

                let offense = self.ctx.offense_with_range(
                    "Style/RescueModifier",
                    MSG,
                    Severity::Convention,
                    start,
                    end,
                );
                self.offenses.push(offense.with_correction(correction));
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode) {
        let start = node.location().start_offset();
        if !self.skip_rescue_at.contains(&start) {
            let end = node.location().end_offset();
            let source = self.ctx.source;
            let bytes = source.as_bytes();

            // Check if this rescue modifier is parenthesized: `(expr rescue handler)`
            let is_paren = start > 0
                && bytes.get(start.wrapping_sub(1)).copied() == Some(b'(')
                && bytes.get(end).copied() == Some(b')');

            let (edits, offense_start, offense_end) = if is_paren {
                let paren_start = start - 1;
                let paren_end = end + 1;
                // Use column of the paren (parent) as the node column
                let node_col = col_of(source, paren_start);
                let node_offset = " ".repeat(node_col);
                let node_indentation = " ".repeat(node_col + self.indent_width);

                let expr = node.expression();
                let handler = node.rescue_expression();
                let expr_src = &source[expr.location().start_offset()..expr.location().end_offset()];
                let handler_src = &source[handler.location().start_offset()..handler.location().end_offset()];

                let replacement = format!(
                    "begin\n{}{}\n{}rescue\n{}{}\n{}end",
                    node_indentation, expr_src, node_offset, node_indentation, handler_src, node_offset
                );
                let e = vec![Edit {
                    start_offset: paren_start,
                    end_offset: paren_end,
                    replacement,
                }];
                (e, start, end)
            } else {
                let e = build_rescue_correction(node, source, self.indent_width);
                (e, start, end)
            };

            let offense = self.ctx.offense_with_range(
                "Style/RescueModifier",
                MSG,
                Severity::Convention,
                offense_start,
                offense_end,
            );
            let correction = Correction { edits };
            self.offenses.push(offense.with_correction(correction));
        }
        ruby_prism::visit_rescue_modifier_node(self, node);
    }
}

crate::register_cop!("Style/RescueModifier", |cfg| {
    let indent_width = cfg
        .get_cop_config("Layout/IndentationWidth")
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_i64())
        .unwrap_or(2) as usize;
    Some(Box::new(RescueModifier::new(indent_width)))
});
