//! Style/MultilineIfModifier cop
//!
//! Checks for multiline bodies with trailing if/unless modifier.

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::col_at_offset;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{IfNode, Node, UnlessNode};

#[derive(Default)]
pub struct MultilineIfModifier;

impl MultilineIfModifier {
    pub fn new() -> Self {
        Self
    }

    /// Is this a modifier-form if? (no `then` keyword, no `end`)
    fn is_modifier_if(node: &IfNode) -> bool {
        node.then_keyword_loc().is_none() && node.end_keyword_loc().is_none()
    }

    fn is_modifier_unless(node: &UnlessNode) -> bool {
        node.end_keyword_loc().is_none()
    }

    fn body_is_multiline(start: usize, end: usize, source: &str) -> bool {
        source[start..end].contains('\n')
    }

    fn cond_is_multiline(start: usize, end: usize, source: &str) -> bool {
        source[start..end].contains('\n')
    }

    /// Convert a modifier if/unless node to block form, recursively expanding nested modifiers.
    /// Returns the expanded source text (including leading indent — edit starts at line_start).
    fn to_normal_form(body_src: &str, cond_src: &str, keyword: &str, col: usize) -> String {
        let indent = " ".repeat(col);
        let inner_indent = " ".repeat(col + 2);

        // Expand body: if body is itself a modifier if/unless, recursively expand
        let expanded_body = Self::expand_body_if_modifier(body_src);

        // Re-indent body: RuboCop's indented_body algorithm
        // 1. Prepend offset (= col spaces) to body source
        // 2. Replace leading col spaces with col+2 spaces on each line
        let body_with_offset = format!("{}{}", indent, expanded_body);
        let indented_body: String = body_with_offset
            .lines()
            .map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else if line.len() >= col && &line[..col] == indent.as_str() {
                    format!("{}{}", inner_indent, &line[col..])
                } else {
                    // Replace as many leading spaces as possible
                    let trimmed = line.trim_start_matches(' ');
                    format!("{}{}", inner_indent, trimmed)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Include leading indent in replacement so the full node is self-contained
        format!("{}{} {}\n{}\n{}end", indent, keyword, cond_src, indented_body, indent)
    }

    /// If body_src is a modifier if/unless, recursively convert to block form at col=0.
    /// Returns the expanded form without leading indent on first line.
    fn expand_body_if_modifier(body_src: &str) -> String {
        let trimmed = body_src.trim_end();
        if let Some((sub_body, sub_kw, sub_cond)) = find_trailing_modifier(trimmed) {
            // Recursively expand sub_body
            let expanded = Self::expand_body_if_modifier(sub_body);
            // Indent expanded by 2 spaces
            let indented: String = expanded.lines()
                .map(|l| if l.trim().is_empty() { String::new() } else { format!("  {}", l) })
                .collect::<Vec<_>>().join("\n");
            return format!("{} {}\n{}\nend", sub_kw, sub_cond, indented);
        }
        body_src.to_string()
    }

    /// Check if a StatementsNode contains exactly one child that is itself a modifier if/unless.
    /// If so, we skip — the inner modifier will be flagged instead (avoids duplicate offenses).
    fn body_is_modifier_conditional(stmts_node: &ruby_prism::StatementsNode) -> bool {
        let items: Vec<Node> = stmts_node.body().iter().collect();
        if items.len() != 1 { return false; }
        match &items[0] {
            Node::IfNode { .. } => {
                let inner = items[0].as_if_node().unwrap();
                // Is inner a modifier if? (no then/end keyword)
                inner.then_keyword_loc().is_none() && inner.end_keyword_loc().is_none()
            }
            Node::UnlessNode { .. } => {
                let inner = items[0].as_unless_node().unwrap();
                inner.end_keyword_loc().is_none()
            }
            _ => false,
        }
    }
}

impl Cop for MultilineIfModifier {
    fn name(&self) -> &'static str {
        "Style/MultilineIfModifier"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &IfNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_modifier_if(node) {
            return vec![];
        }

        let stmts = match node.statements() {
            Some(s) => s,
            None => return vec![],
        };

        let body_start = stmts.location().start_offset();
        let body_end = stmts.location().end_offset();

        // Allow if condition is multiline
        let cond = node.predicate();
        if Self::cond_is_multiline(cond.location().start_offset(), cond.location().end_offset(), ctx.source) {
            return vec![];
        }

        if !Self::body_is_multiline(body_start, body_end, ctx.source) {
            return vec![];
        }

        // If body is itself a modifier conditional, skip — outer node will fire instead
        if Self::body_is_modifier_conditional(&stmts) {
            return vec![];
        }

        let msg = "Favor a normal if-statement over a modifier clause in a multiline statement.";
        let cond = node.predicate();

        // Determine effective range: check if source after node_end has outer modifier
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let body_src = &ctx.source[body_start..body_end];
        let cond_src = &ctx.source[cond.location().start_offset()..cond.location().end_offset()];

        let (eff_start, eff_end, eff_body, eff_cond, eff_kw) =
            expand_to_outer_modifier(node_start, node_end, body_src, cond_src, "if", ctx.source);

        let col = col_at_offset(ctx.source, eff_start) as usize;
        let replacement = Self::to_normal_form(&eff_body, &eff_cond, &eff_kw, col);
        let correction = Correction {
            edits: vec![Edit { start_offset: eff_start, end_offset: eff_end, replacement }],
        };

        vec![ctx.offense_with_range(self.name(), msg, self.severity(), body_start, body_start + 1)
            .with_correction(correction)]
    }

    fn check_unless(&self, node: &UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_modifier_unless(node) {
            return vec![];
        }

        let stmts = match node.statements() {
            Some(s) => s,
            None => return vec![],
        };

        let body_start = stmts.location().start_offset();
        let body_end = stmts.location().end_offset();

        let cond = node.predicate();
        if Self::cond_is_multiline(cond.location().start_offset(), cond.location().end_offset(), ctx.source) {
            return vec![];
        }

        if !Self::body_is_multiline(body_start, body_end, ctx.source) {
            return vec![];
        }

        if Self::body_is_modifier_conditional(&stmts) {
            return vec![];
        }

        let msg = "Favor a normal unless-statement over a modifier clause in a multiline statement.";
        let cond = node.predicate();

        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let body_src = &ctx.source[body_start..body_end];
        let cond_src = &ctx.source[cond.location().start_offset()..cond.location().end_offset()];

        let (eff_start, eff_end, eff_body, eff_cond, eff_kw) =
            expand_to_outer_modifier(node_start, node_end, body_src, cond_src, "unless", ctx.source);

        let col = col_at_offset(ctx.source, eff_start) as usize;
        let replacement = Self::to_normal_form(&eff_body, &eff_cond, &eff_kw, col);
        let correction = Correction {
            edits: vec![Edit { start_offset: eff_start, end_offset: eff_end, replacement }],
        };

        vec![ctx.offense_with_range(self.name(), msg, self.severity(), body_start, body_start + 1)
            .with_correction(correction)]
    }
}

/// Check if the node at [node_start..node_end] is itself the body of an outer modifier.
/// If so, expand to cover the full outer modifier.
/// Returns (eff_start, eff_end, eff_body_src, eff_cond_src, eff_keyword).
fn expand_to_outer_modifier<'a>(
    node_start: usize,
    node_end: usize,
    body_src: &'a str,
    cond_src: &'a str,
    keyword: &'a str,
    source: &'a str,
) -> (usize, usize, String, String, &'a str) {
    let src_bytes = source.as_bytes();
    // Look at source after node_end: skip whitespace then check for ` if ` or ` unless `
    let mut pos = node_end;
    // The node_end might be at a position like `] if outer`. Check for ` if ` or ` unless `.
    let remaining = &source[pos..];

    // Check for outer modifier: ` if X` or ` unless X` at same level
    // remaining should look like ` if cond` or ` unless cond`
    let stripped = remaining.trim_end_matches('\n').trim_end();
    if let Some(rest) = stripped.strip_prefix(" if ") {
        let outer_cond = rest.trim_end();
        let outer_end = pos + 1 + 3 + outer_cond.len(); // " if " + cond
        // eff_body = current node source (node_start..node_end includes body + if keyword + inner cond)
        let eff_body = source[node_start..node_end].to_string();
        return (node_start, outer_end, eff_body, outer_cond.to_string(), "if");
    } else if let Some(rest) = stripped.strip_prefix(" unless ") {
        let outer_cond = rest.trim_end();
        let outer_end = pos + 1 + 7 + outer_cond.len(); // " unless " + cond
        let eff_body = source[node_start..node_end].to_string();
        return (node_start, outer_end, eff_body, outer_cond.to_string(), "unless");
    }

    // No outer modifier — use node as-is
    (node_start, node_end, body_src.to_string(), cond_src.to_string(), keyword)
}

/// Find trailing `if COND` or `unless COND` modifier in source text.
/// Returns (sub_body, keyword, cond) or None.
fn find_trailing_modifier(src: &str) -> Option<(&str, &str, &str)> {
    // Scan from right: find ` if ` or ` unless ` not inside brackets/quotes
    let bytes = src.as_bytes();
    let mut depth_paren: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut in_string = false;
    let mut string_char = b'"';

    // Simple forward scan to track depth, then find modifier
    // Instead of scanning backwards, find keywords from left but only at depth 0
    let mut i = 0;
    let mut last_modifier_pos: Option<(usize, &str)> = None;
    while i < bytes.len() {
        if in_string {
            if bytes[i] == string_char && (i == 0 || bytes[i-1] != b'\\') {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => { in_string = true; string_char = bytes[i]; i += 1; }
            b'(' => { depth_paren += 1; i += 1; }
            b')' => { depth_paren -= 1; i += 1; }
            b'[' => { depth_bracket += 1; i += 1; }
            b']' => { depth_bracket -= 1; i += 1; }
            b'{' => { depth_brace += 1; i += 1; }
            b'}' => { depth_brace -= 1; i += 1; }
            b' ' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                // Check for ` if ` or ` unless `
                if src[i..].starts_with(" if ") {
                    last_modifier_pos = Some((i, "if"));
                } else if src[i..].starts_with(" unless ") {
                    last_modifier_pos = Some((i, "unless"));
                }
                i += 1;
            }
            _ => { i += 1; }
        }
    }

    if let Some((pos, kw)) = last_modifier_pos {
        let sub_body = &src[..pos];
        let kw_len = kw.len() + 2; // " if " or " unless " (with spaces)
        let cond_start = pos + kw_len;
        if cond_start <= src.len() {
            let cond = &src[cond_start..];
            return Some((sub_body, kw, cond));
        }
    }
    None
}

crate::register_cop!("Style/MultilineIfModifier", |_cfg| Some(Box::new(MultilineIfModifier::new())));
