//! Style/EmptyStringInsideInterpolation cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct EmptyStringInsideInterpolation {
    style: Style,
}

#[derive(Clone, Copy)]
pub enum Style { TrailingConditional, Ternary }

impl Default for EmptyStringInsideInterpolation {
    fn default() -> Self { Self { style: Style::TrailingConditional } }
}

impl EmptyStringInsideInterpolation {
    pub fn new(style: Style) -> Self { Self { style } }
}

fn src<'a>(n: &Node<'a>, source: &'a str) -> &'a str {
    let l = n.location();
    &source[l.start_offset()..l.end_offset()]
}

/// True if node is an empty-string literal or `nil`.
fn is_empty_or_nil(n: &Node, source: &str) -> bool {
    if n.as_nil_node().is_some() { return true; }
    if let Some(s) = n.as_string_node() {
        let vloc = s.content_loc();
        return vloc.start_offset() == vloc.end_offset();
    }
    // Interpolated empty string "" or ''
    let l = n.location();
    let text = &source[l.start_offset()..l.end_offset()];
    text == "''" || text == "\"\""
}

/// True if node is a literal or atom (not a call/send).
fn is_literal(n: &Node) -> bool {
    n.as_string_node().is_some()
        || n.as_integer_node().is_some()
        || n.as_float_node().is_some()
        || n.as_symbol_node().is_some()
        || n.as_true_node().is_some()
        || n.as_false_node().is_some()
        || n.as_nil_node().is_some()
        || n.as_interpolated_string_node().is_some()
}

impl Cop for EmptyStringInsideInterpolation {
    fn name(&self) -> &'static str { "Style/EmptyStringInsideInterpolation" }
    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, style: self.style, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> { ctx: &'a CheckContext<'a>, style: Style, offenses: Vec<Offense> }

impl<'a> V<'a> {
    fn check_embedded(&mut self, emb: &ruby_prism::EmbeddedStatementsNode<'a>) {
        let source = self.ctx.source;
        let stmts = match emb.statements() { Some(s) => s, None => return };
        let items: Vec<_> = stmts.body().iter().collect();
        if items.len() != 1 { return; }
        let inner = &items[0];
        match self.style {
            Style::TrailingConditional => self.check_trailing(emb, inner, source),
            Style::Ternary => self.check_ternary_style(emb, inner, source),
        }
    }

    /// trailing_conditional style: flag cond ? A : B / if/else where one side is empty-string/nil.
    fn check_trailing(&mut self, emb: &ruby_prism::EmbeddedStatementsNode<'a>, inner: &Node<'a>, source: &'a str) {
        let i = match inner.as_if_node() { Some(i) => i, None => return };
        // Need an else branch.
        let sub = match i.subsequent() { Some(s) => s, None => return };
        let else_n = match sub.as_else_node() { Some(e) => e, None => return };
        // Both branches must be single-statement.
        let if_stmts = match i.statements() { Some(s) => s, None => return };
        let if_items: Vec<_> = if_stmts.body().iter().collect();
        if if_items.len() != 1 { return; }
        let else_stmts = match else_n.statements() { Some(s) => s, None => return };
        let else_items: Vec<_> = else_stmts.body().iter().collect();
        if else_items.len() != 1 { return; }
        let a = &if_items[0];
        let b = &else_items[0];
        // One must be empty/nil, other must be a literal (not a send).
        let a_empty = is_empty_or_nil(a, source);
        let b_empty = is_empty_or_nil(b, source);
        let cond = i.predicate();
        let cond_src = src(&cond, source);
        let replacement_expr;
        if b_empty && !a_empty && is_literal(a) {
            // `cond ? A : ''` → `A if cond`
            replacement_expr = format!("{} if {}", src(a, source), cond_src);
        } else if a_empty && !b_empty && is_literal(b) {
            // `cond ? '' : B` → `B unless cond`
            replacement_expr = format!("{} unless {}", src(b, source), cond_src);
        } else {
            return;
        }
        // Offense range: inside the `#{...}` — from the `#{` + 2 to the `}` - 0 exclusive.
        let emb_loc = emb.location();
        let start = emb_loc.start_offset() + 2; // skip `#{`
        let end = emb_loc.end_offset() - 1;     // skip `}`
        let msg = "Do not return empty strings in string interpolation.";
        // column_end is +1 for `}` based on fixtures: for `"#{condition ? 'foo' : ''}"` range 3..25 includes last char.
        // Actually fixture column_end = 25 means exclusive end at 25; source index 25 is `}`.
        // So end = emb_loc.end_offset() - 1 matches.
        let correction = Correction::replace(inner.location().start_offset(), inner.location().end_offset(), replacement_expr);
        self.offenses.push(
            self.ctx.offense_with_range("Style/EmptyStringInsideInterpolation", msg, Severity::Convention, start, end)
                .with_correction(correction),
        );
    }

    /// ternary style: flag trailing `A if cond` / `A unless cond` inside interpolation.
    fn check_ternary_style(&mut self, emb: &ruby_prism::EmbeddedStatementsNode<'a>, inner: &Node<'a>, source: &'a str) {
        // inner must be a modifier-if or modifier-unless: IfNode or UnlessNode with no else branch
        // Check source pattern: modifier forms are where if_keyword_loc comes AFTER body.
        let (is_unless, cond_src, body_src) = if let Some(i) = inner.as_if_node() {
            if i.subsequent().is_some() { return; }
            // Must be modifier (if_keyword after body)
            let stmts = match i.statements() { Some(s) => s, None => return };
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 { return; }
            let body = &items[0];
            let if_kw = match i.if_keyword_loc() { Some(l) => l, None => return };
            if if_kw.start_offset() <= body.location().start_offset() { return; }
            let cond = i.predicate();
            if !is_literal(body) { return; }
            (false, src(&cond, source).to_string(), src(body, source).to_string())
        } else if let Some(u) = inner.as_unless_node() {
            if u.else_clause().is_some() { return; }
            let stmts = match u.statements() { Some(s) => s, None => return };
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() != 1 { return; }
            let body = &items[0];
            let kw = u.keyword_loc();
            if kw.start_offset() <= body.location().start_offset() { return; }
            let cond = u.predicate();
            if !is_literal(body) { return; }
            (true, src(&cond, source).to_string(), src(body, source).to_string())
        } else {
            return;
        };
        // Offense range inside `#{...}`: from the first char after `#` (i.e. `{`) to before `}` — per fixture column 1..22 for `"#{'foo' if condition}"`.
        // Actually fixture: `"#{'foo' if condition}"` col 1..22. The source is 22 chars long. char[1] = '#', char[21] = '"' (last). So range 1..22 is `#{'foo' if condition}`. So start at emb_loc.start_offset() (which includes `#{`), end at emb_loc.end_offset().
        let emb_loc = emb.location();
        let start = emb_loc.start_offset();
        let end = emb_loc.end_offset();
        let msg = "Do not use trailing conditionals in string interpolation.";
        let replacement_expr = if is_unless {
            format!("{} ? '' : {}", cond_src, body_src)
        } else {
            format!("{} ? {} : ''", cond_src, body_src)
        };
        let correction = Correction::replace(inner.location().start_offset(), inner.location().end_offset(), replacement_expr);
        self.offenses.push(
            self.ctx.offense_with_range("Style/EmptyStringInsideInterpolation", msg, Severity::Convention, start, end)
                .with_correction(correction),
        );
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_embedded_statements_node(&mut self, node: &ruby_prism::EmbeddedStatementsNode<'a>) {
        self.check_embedded(node);
        ruby_prism::visit_embedded_statements_node(self, node);
    }
}

crate::register_cop!("Style/EmptyStringInsideInterpolation", |cfg| {
    let style = cfg.get_cop_config("Style/EmptyStringInsideInterpolation")
        .and_then(|c| c.raw.get("EnforcedStyle").and_then(|v| v.as_str().map(|s| s.to_string())))
        .unwrap_or_else(|| "trailing_conditional".to_string());
    let style = if style == "ternary" { Style::Ternary } else { Style::TrailingConditional };
    Some(Box::new(EmptyStringInsideInterpolation::new(style)))
});
