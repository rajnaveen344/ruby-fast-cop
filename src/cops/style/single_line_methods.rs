//! Style/SingleLineMethods cop
//!
//! Checks for single-line method definitions that contain a body.

use crate::cops::{CheckContext, Cop};
use crate::cops::style::trailing_body_on_method_definition::{find_comment_on_line, find_semicolon_before_body};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{DefNode, Node};

pub struct SingleLineMethods {
    allow_if_method_is_empty: bool,
    indent_width: usize,
}

impl Default for SingleLineMethods {
    fn default() -> Self {
        Self { allow_if_method_is_empty: true, indent_width: 2 }
    }
}

impl SingleLineMethods {
    pub fn new(allow_if_method_is_empty: bool, indent_width: usize) -> Self {
        Self { allow_if_method_is_empty, indent_width }
    }

    fn is_single_line(node: &DefNode, source: &str) -> bool {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        !source[start..end].contains('\n')
    }

    fn is_endless(node: &DefNode) -> bool {
        node.equal_loc().is_some()
    }

    fn correct_to_multiline(
        &self,
        node: &DefNode,
        ctx: &CheckContext,
    ) -> Correction {
        let def_start = node.location().start_offset();
        let def_col = ctx.col_of(def_start);
        let indent_body = " ".repeat(def_col + self.indent_width);
        let indent_end = " ".repeat(def_col);
        let source = ctx.source;
        let bytes = source.as_bytes();

        // Get end keyword location
        let end_loc = node.end_keyword_loc().expect("single-line non-endless method has end");
        let end_start = end_loc.start_offset();

        // Collect body parts
        let body_parts: Vec<(usize, usize)> = if let Some(body) = node.body() {
            collect_body_parts(&body)
        } else {
            vec![]
        };

        let def_line_start = ctx.line_start(def_start);
        let def_line_end = source[def_line_start..].find('\n')
            .map(|p| def_line_start + p)
            .unwrap_or(source.len());

        // Find EOL comment
        let search_from = body_parts.last().map(|&(_, e)| e).unwrap_or(def_start + 1);
        let comment_start = find_comment_on_line(bytes, search_from, def_line_end);

        let mut edits: Vec<Edit> = Vec::new();

        // Move comment before def if present
        if let Some(cs) = comment_start {
            let comment_text = &source[cs..def_line_end];
            let before_def_indent = " ".repeat(def_col);
            edits.push(Edit {
                start_offset: def_start,
                end_offset: def_start,
                replacement: format!("{comment_text}\n{before_def_indent}"),
            });
            let remove_from = cs;
            edits.push(Edit {
                start_offset: remove_from,
                end_offset: def_line_end,
                replacement: String::new(),
            });
        }

        // For each body part, insert `\n{indent_body}` before it
        // Also handle semicolons between def header and first body part
        for (i, &(part_start, _)) in body_parts.iter().enumerate() {
            if i == 0 {
                // May have a semicolon before first body stmt (between def and body)
                // We don't remove it (unlike TrailingBodyOnMethodDefinition)
                // SingleLineMethods keeps the semicolon
            }
            edits.push(Edit {
                start_offset: part_start,
                end_offset: part_start,
                replacement: format!("\n{indent_body}"),
            });
        }

        // Insert `\n{indent_end}` before `end` keyword
        // Check for semicolon immediately before end
        let sc = find_semicolon_before_body(bytes, end_start);
        if let Some(sc_pos) = sc {
            // Replace semicolon + space (if any) with nothing up to end keyword
            // Actually: we insert before `end`, which naturally leaves `;` as trailing
            // RuboCop keeps trailing `;` before end on the body line
        } else {
            let _ = sc;
        }
        edits.push(Edit {
            start_offset: end_start,
            end_offset: end_start,
            replacement: format!("\n{indent_end}"),
        });

        Correction { edits }
    }
}

fn collect_body_parts(body: &Node) -> Vec<(usize, usize)> {
    match body {
        Node::StatementsNode { .. } => {
            if let Some(stmts) = body.as_statements_node() {
                stmts.body().iter().map(|n| (n.location().start_offset(), n.location().end_offset())).collect()
            } else {
                vec![]
            }
        }
        _ => {
            let loc = body.location();
            vec![(loc.start_offset(), loc.end_offset())]
        }
    }
}

impl Cop for SingleLineMethods {
    fn name(&self) -> &'static str {
        "Style/SingleLineMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &DefNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_single_line(node, ctx.source) {
            return vec![];
        }
        if Self::is_endless(node) {
            return vec![];
        }
        // Check if body is empty
        let has_body = node.body().is_some();
        if !has_body && self.allow_if_method_is_empty {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let offense = ctx.offense_with_range(
            self.name(),
            "Avoid single-line method definitions.",
            self.severity(),
            start,
            end,
        );
        let correction = self.correct_to_multiline(node, ctx);
        vec![offense.with_correction(correction)]
    }
}

// Re-export for use by other modules
pub use self::helpers::*;
mod helpers {
    // empty — just re-export from trailing_body_on_method_definition
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_if_method_is_empty: Option<bool>,
}

crate::register_cop!("Style/SingleLineMethods", |cfg| {
    let c: Cfg = cfg.typed("Style/SingleLineMethods");
    let allow = c.allow_if_method_is_empty.unwrap_or(true);
    let indent_width = cfg
        .get_cop_config("Layout/IndentationWidth")
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_i64())
        .unwrap_or(2) as usize;
    Some(Box::new(SingleLineMethods::new(allow, indent_width)))
});
