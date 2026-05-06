//! Style/EndlessMethod
//!
//! Five enforced styles govern endless method definitions (`def foo = x`).
//! Ruby 3.0+ only.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{DefNode, Node, Visit};

const MSG: &str = "Avoid endless method definitions.";
const MSG_MULTI_LINE: &str = "Avoid endless method definitions with multiple lines.";
const MSG_REQUIRE_SINGLE: &str = "Use endless method definitions for single line methods.";
const MSG_REQUIRE_ALWAYS: &str = "Use endless method definitions.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndlessMethodStyle {
    AllowSingleLine,
    AllowAlways,
    Disallow,
    RequireSingleLine,
    RequireAlways,
}

pub struct EndlessMethod {
    style: EndlessMethodStyle,
    max_line_length: Option<usize>,
}

impl EndlessMethod {
    pub fn new() -> Self { Self { style: EndlessMethodStyle::AllowSingleLine, max_line_length: None } }
    pub fn with_style(style: EndlessMethodStyle) -> Self {
        Self { style, max_line_length: None }
    }
    pub fn with_config(style: EndlessMethodStyle, max_line_length: Option<usize>) -> Self {
        Self { style, max_line_length }
    }
}

impl Default for EndlessMethod {
    fn default() -> Self { Self::new() }
}

fn is_endless(node: &DefNode) -> bool { node.equal_loc().is_some() }

fn is_assignment_method(node: &DefNode) -> bool {
    node.name().as_slice().last() == Some(&b'=')
}

fn source_of<'a>(source: &'a str, loc: &ruby_prism::Location<'_>) -> &'a str {
    &source[loc.start_offset()..loc.end_offset()]
}

fn body_is_string_heredoc(body: &Node, source: &str) -> bool {
    if let Some(s) = body.as_string_node() {
        if let Some(open) = s.opening_loc() {
            let opening = source_of(source, &open);
            return opening.starts_with("<<");
        }
    }
    if let Some(s) = body.as_interpolated_string_node() {
        if let Some(open) = s.opening_loc() {
            let opening = source_of(source, &open);
            return opening.starts_with("<<");
        }
    }
    if let Some(s) = body.as_x_string_node() {
        let open = s.opening_loc();
        let opening = source_of(source, &open);
        return opening.starts_with("<<");
    }
    if let Some(s) = body.as_interpolated_x_string_node() {
        let open = s.opening_loc();
        let opening = source_of(source, &open);
        return opening.starts_with("<<");
    }
    false
}

struct HeredocFinder<'a> {
    source: &'a str,
    found: bool,
}

impl<'a> Visit<'_> for HeredocFinder<'a> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if self.found { return }
        if let Some(open) = node.opening_loc() {
            if source_of(self.source, &open).starts_with("<<") { self.found = true; return }
        }
        ruby_prism::visit_string_node(self, node);
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if self.found { return }
        if let Some(open) = node.opening_loc() {
            if source_of(self.source, &open).starts_with("<<") { self.found = true; return }
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode) {
        if self.found { return }
        let open = node.opening_loc();
        if source_of(self.source, &open).starts_with("<<") { self.found = true; return }
        ruby_prism::visit_x_string_node(self, node);
    }
    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        if self.found { return }
        let open = node.opening_loc();
        if source_of(self.source, &open).starts_with("<<") { self.found = true; return }
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

fn uses_heredoc(node: &DefNode, source: &str) -> bool {
    let Some(body) = node.body() else { return false };
    if body_is_string_heredoc(&body, source) { return true }
    let mut f = HeredocFinder { source, found: false };
    f.visit(&body);
    f.found
}

fn is_single_line(ctx: &CheckContext, node: &DefNode) -> bool {
    let loc = node.location();
    ctx.same_line(loc.start_offset(), loc.end_offset())
}

fn body_single_line(ctx: &CheckContext, node: &DefNode) -> bool {
    let Some(body) = node.body() else { return false };
    let loc = body.location();
    ctx.same_line(loc.start_offset(), loc.end_offset())
}

fn can_be_made_endless(node: &DefNode) -> bool {
    let Some(body) = node.body() else { return false };
    if let Some(stmts) = body.as_statements_node() {
        let mut iter = stmts.body().iter();
        let Some(first) = iter.next() else { return false };
        if iter.next().is_some() { return false }
        // Single statement must not be a BeginNode (multi-statement via begin/end)
        return first.as_begin_node().is_none();
    }
    body.as_begin_node().is_none()
}

// RuboCop's offense range ≈ `def` keyword through the method name.
// That produces column_end of "def my_method" for plain, "def self.my_method"
// for self-receiver, "def my_method(a, b)" — actually the fixture indicates
// the full first-line signature (up to `= body`). We use node.location() which
// covers the entire def; from_offsets truncates to first newline column.
fn offense_range(node: &DefNode) -> (usize, usize) {
    let loc = node.location();
    (loc.start_offset(), loc.end_offset())
}

/// Estimate `def NAME = BODY` length at def's start column; skip if > Layout/LineLength.Max.
fn would_exceed_line_length(ctx: &CheckContext, node: &DefNode, max: Option<usize>) -> bool {
    let Some(max) = max else { return false };
    let Some(body) = node.body() else { return false };
    let body_loc = body.location();
    let body_src = &ctx.source[body_loc.start_offset()..body_loc.end_offset()];
    // If body itself is a StatementsNode wrapping a single stmt, use that stmt's source.
    let body_trimmed = body_src.trim();
    let def_start = node.location().start_offset();
    // Find start-of-line before def_start.
    let line_start = ctx.source[..def_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let start_col = def_start - line_start;
    let name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
    // Parameters source, if any.
    let params_src = node
        .parameters()
        .map(|p| {
            let l = p.location();
            ctx.source[l.start_offset()..l.end_offset()].to_string()
        })
        .map(|s| format!("({})", s))
        .unwrap_or_default();
    let line = format!("def {}{} = {}", name, params_src, body_trimmed);
    start_col + line.chars().count() > max
}

fn push_with_correction(offenses: &mut Vec<Offense>, ctx: &CheckContext, node: &DefNode, msg: &str, correction: Option<Correction>) {
    let (start, end) = offense_range(node);
    let offense = ctx.offense_with_range("Style/EndlessMethod", msg, Severity::Convention, start, end);
    let offense = if let Some(c) = correction { offense.with_correction(c) } else { offense };
    offenses.push(offense);
}

/// Build `def SIGNATURE\n  BODY\nend` from an endless method node.
fn correction_endless_to_regular(node: &DefNode, ctx: &CheckContext) -> Correction {
    let source = ctx.source;
    let def_start = node.location().start_offset();
    let def_end = node.location().end_offset();

    // The `=` location: node.equal_loc() gives us the `=` position.
    let eq_loc = node.equal_loc().unwrap();
    // Body starts after `= `
    let body_start = {
        let after_eq = eq_loc.end_offset();
        // skip whitespace
        let trimmed = source[after_eq..].trim_start_matches(' ');
        after_eq + (source[after_eq..].len() - trimmed.len())
    };

    // Signature: from def_start to before `=` (trim trailing space and empty parens)
    let sig_src = &source[def_start..eq_loc.start_offset()];
    let sig = sig_src.trim_end();

    // Remove empty `()` from signature
    let sig = if sig.ends_with("()") {
        sig[..sig.len()-2].trim_end()
    } else {
        sig
    };

    // Body source: from body_start to def_end
    let body_src = &source[body_start..def_end];

    // Compute indentation: 2 spaces relative to def's column
    let line_start = source[..def_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let def_col = def_start - line_start;
    let body_indent = " ".repeat(def_col + 2);
    let end_indent = " ".repeat(def_col);

    let replacement = format!("{}\n{}{}\n{}end", sig, body_indent, body_src, end_indent);
    Correction::replace(def_start, def_end, replacement)
}

/// Build `def SIGNATURE = BODY` from a regular multi-statement-candidate method.
fn correction_regular_to_endless(node: &DefNode, ctx: &CheckContext) -> Correction {
    let source = ctx.source;
    let def_start = node.location().start_offset();
    let def_end = node.location().end_offset();

    // Find the body: single statement inside.
    let body = node.body().unwrap();
    let (body_start, body_end) = if let Some(stmts) = body.as_statements_node() {
        let stmt = stmts.body().iter().next().unwrap();
        let l = stmt.location();
        (l.start_offset(), l.end_offset())
    } else {
        let l = body.location();
        (l.start_offset(), l.end_offset())
    };

    // Signature: from def_start to end of first line
    // Find end of the `def NAME(params)` line (the newline after params)
    let first_line_end = source[def_start..].find('\n').map(|p| def_start + p).unwrap_or(def_end);

    // sig = first line, trim trailing whitespace
    let sig_src = source[def_start..first_line_end].trim_end();

    // Body first line (what becomes the `= X` part)
    let body_first_line_end = source[body_start..].find('\n').map(|p| body_start + p).unwrap_or(body_end);
    let body_first_line = source[body_start..body_first_line_end].trim();

    // Any remaining lines after the first body line (for multiline body expressions)
    let after_first_body = if body_first_line_end < body_end {
        &source[body_first_line_end..body_end]
    } else {
        ""
    };

    // The part after body_end until def_end (could contain `\nend`)
    let after_body = &source[body_end..def_end];
    // Find `end` keyword location — skip the `\nend` part
    let trimmed_after = after_body.trim_end_matches(|c: char| c.is_whitespace());
    let end_keyword = if trimmed_after == "end" {
        // `\nend` or `  end`
        ""
    } else {
        trimmed_after
    };
    let _ = end_keyword;

    // Build replacement: `def sig = body_first_line` + any continuation lines
    let replacement = if after_first_body.is_empty() {
        format!("{} = {}", sig_src, body_first_line)
    } else {
        format!("{} = {}{}", sig_src, body_first_line, after_first_body)
    };

    Correction::replace(def_start, def_end, replacement)
}

impl Cop for EndlessMethod {
    fn name(&self) -> &'static str { "Style/EndlessMethod" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &DefNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 0) { return vec![] }
        if is_assignment_method(node) { return vec![] }
        if uses_heredoc(node, ctx.source) { return vec![] }

        let mut offenses = Vec::new();
        match self.style {
            EndlessMethodStyle::AllowAlways => {}
            EndlessMethodStyle::AllowSingleLine => {
                if is_endless(node) && !is_single_line(ctx, node) {
                    let corr = correction_endless_to_regular(node, ctx);
                    push_with_correction(&mut offenses, ctx, node, MSG_MULTI_LINE, Some(corr));
                }
            }
            EndlessMethodStyle::Disallow => {
                if is_endless(node) {
                    let corr = correction_endless_to_regular(node, ctx);
                    push_with_correction(&mut offenses, ctx, node, MSG, Some(corr));
                }
            }
            EndlessMethodStyle::RequireSingleLine => {
                if is_endless(node) && !is_single_line(ctx, node) {
                    let corr = correction_endless_to_regular(node, ctx);
                    push_with_correction(&mut offenses, ctx, node, MSG_MULTI_LINE, Some(corr));
                } else if !is_endless(node) && can_be_made_endless(node)
                    && body_single_line(ctx, node)
                    && !would_exceed_line_length(ctx, node, self.max_line_length)
                {
                    let corr = correction_regular_to_endless(node, ctx);
                    push_with_correction(&mut offenses, ctx, node, MSG_REQUIRE_SINGLE, Some(corr));
                }
            }
            EndlessMethodStyle::RequireAlways => {
                if !is_endless(node) && can_be_made_endless(node)
                    && !would_exceed_line_length(ctx, node, self.max_line_length)
                {
                    let corr = correction_regular_to_endless(node, ctx);
                    push_with_correction(&mut offenses, ctx, node, MSG_REQUIRE_ALWAYS, Some(corr));
                }
            }
        }
        offenses
    }
}

crate::register_cop!("Style/EndlessMethod", |cfg| {
    let style = cfg.get_cop_config("Style/EndlessMethod")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "allow_always" => EndlessMethodStyle::AllowAlways,
            "disallow" => EndlessMethodStyle::Disallow,
            "require_single_line" => EndlessMethodStyle::RequireSingleLine,
            "require_always" => EndlessMethodStyle::RequireAlways,
            _ => EndlessMethodStyle::AllowSingleLine,
        })
        .unwrap_or(EndlessMethodStyle::AllowSingleLine);
    let max_line_length = if cfg.is_cop_enabled("Layout/LineLength") {
        cfg.get_cop_config("Layout/LineLength").and_then(|c| c.max).map(|m| m as usize)
    } else { None };
    Some(Box::new(EndlessMethod::with_config(style, max_line_length)))
});
