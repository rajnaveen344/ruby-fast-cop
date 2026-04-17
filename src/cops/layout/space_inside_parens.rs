//! Layout/SpaceInsideParens cop
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_inside_parens.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Space inside parentheses detected.";
const MSG_SPACE: &str = "No space inside parentheses detected.";

#[derive(Clone, Copy, Debug)]
pub enum SpaceInsideParensStyle {
    NoSpace,
    Space,
    Compact,
}

pub struct SpaceInsideParens {
    style: SpaceInsideParensStyle,
}

impl SpaceInsideParens {
    pub fn new(style: SpaceInsideParensStyle) -> Self {
        Self { style }
    }
}

impl Default for SpaceInsideParens {
    fn default() -> Self {
        Self::new(SpaceInsideParensStyle::NoSpace)
    }
}

/// Paren token (either `(` or `)`).
#[derive(Clone, Copy, Debug)]
struct ParenTok {
    is_left: bool,
    begin: usize, // byte offset of paren char
    end: usize,   // begin+1
}

struct Collector {
    toks: Vec<ParenTok>,
}

impl Collector {
    fn push(&mut self, is_left: bool, begin: usize) {
        self.toks.push(ParenTok { is_left, begin, end: begin + 1 });
    }
}

impl<'a> Visit<'_> for Collector {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let (Some(o), Some(c)) = (node.opening_loc(), node.closing_loc()) {
            let bytes_o = o.as_slice();
            let bytes_c = c.as_slice();
            if bytes_o == b"(" {
                self.push(true, o.start_offset());
            }
            if bytes_c == b")" {
                self.push(false, c.start_offset());
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        let o = node.opening_loc();
        let c = node.closing_loc();
        if o.as_slice() == b"(" {
            self.push(true, o.start_offset());
        }
        if c.as_slice() == b")" {
            self.push(false, c.start_offset());
        }
        ruby_prism::visit_parentheses_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if let Some(l) = node.lparen_loc() {
            self.push(true, l.start_offset());
        }
        if let Some(r) = node.rparen_loc() {
            self.push(false, r.start_offset());
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_multi_target_node(&mut self, node: &ruby_prism::MultiTargetNode) {
        if let Some(l) = node.lparen_loc() {
            if l.as_slice() == b"(" {
                self.push(true, l.start_offset());
            }
        }
        if let Some(r) = node.rparen_loc() {
            if r.as_slice() == b")" {
                self.push(false, r.start_offset());
            }
        }
        ruby_prism::visit_multi_target_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        if let Some(l) = node.lparen_loc() {
            if l.as_slice() == b"(" {
                self.push(true, l.start_offset());
            }
        }
        if let Some(r) = node.rparen_loc() {
            if r.as_slice() == b")" {
                self.push(false, r.start_offset());
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_array_pattern_node(&mut self, node: &ruby_prism::ArrayPatternNode) {
        if let Some(o) = node.opening_loc() {
            if o.as_slice() == b"(" {
                self.push(true, o.start_offset());
            }
        }
        if let Some(c) = node.closing_loc() {
            if c.as_slice() == b")" {
                self.push(false, c.start_offset());
            }
        }
        ruby_prism::visit_array_pattern_node(self, node);
    }

    fn visit_hash_pattern_node(&mut self, node: &ruby_prism::HashPatternNode) {
        if let Some(o) = node.opening_loc() {
            if o.as_slice() == b"(" {
                self.push(true, o.start_offset());
            }
        }
        if let Some(c) = node.closing_loc() {
            if c.as_slice() == b")" {
                self.push(false, c.start_offset());
            }
        }
        ruby_prism::visit_hash_pattern_node(self, node);
    }

    fn visit_find_pattern_node(&mut self, node: &ruby_prism::FindPatternNode) {
        if let Some(o) = node.opening_loc() {
            if o.as_slice() == b"(" {
                self.push(true, o.start_offset());
            }
        }
        if let Some(c) = node.closing_loc() {
            if c.as_slice() == b")" {
                self.push(false, c.start_offset());
            }
        }
        ruby_prism::visit_find_pattern_node(self, node);
    }
}

impl SpaceInsideParens {
    fn collect_parens(&self, program: &ruby_prism::ProgramNode) -> Vec<ParenTok> {
        let mut c = Collector { toks: Vec::new() };
        c.visit_program_node(program);
        c.toks.sort_by_key(|t| t.begin);
        c.toks
    }

    fn same_line(src: &str, a: usize, b: usize) -> bool {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        !src.as_bytes()[lo..hi].contains(&b'\n')
    }

    fn space_after(src: &str, off: usize) -> bool {
        src.as_bytes().get(off).is_some_and(|&b| b == b' ' || b == b'\t')
    }

    fn emit_extraneous(
        &self,
        ctx: &CheckContext,
        from: usize,
        to: usize,
        out: &mut Vec<Offense>,
    ) {
        let offense = ctx
            .offense_with_range(self.name(), MSG, Severity::Convention, from, to)
            .with_correction(Correction::delete(from, to));
        out.push(offense);
    }

    fn emit_missing(
        &self,
        ctx: &CheckContext,
        flag_from: usize,
        flag_to: usize,
        insert_before: usize,
        out: &mut Vec<Offense>,
    ) {
        let offense = ctx
            .offense_with_range(self.name(), MSG_SPACE, Severity::Convention, flag_from, flag_to)
            .with_correction(Correction::insert(insert_before, " "));
        out.push(offense);
    }
}

impl Cop for SpaceInsideParens {
    fn name(&self) -> &'static str {
        "Layout/SpaceInsideParens"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let toks = self.collect_parens(node);
        let src = ctx.source;
        let mut out: Vec<Offense> = Vec::new();

        match self.style {
            SpaceInsideParensStyle::NoSpace => self.run_no_space(&toks, src, ctx, &mut out),
            SpaceInsideParensStyle::Space => self.run_space(&toks, src, ctx, &mut out),
            SpaceInsideParensStyle::Compact => self.run_compact(&toks, src, ctx, &mut out),
        }
        out
    }
}

impl SpaceInsideParens {
    fn run_no_space(
        &self,
        toks: &[ParenTok],
        src: &str,
        ctx: &CheckContext,
        out: &mut Vec<Offense>,
    ) {
        // Flag individual parens: ( with trailing space, ) with leading space — same line,
        // not followed by comment/newline (for `(`) or preceded by newline (for `)`).
        for &t in toks {
            if t.is_left {
                // space after `(` on same line, next non-space is not newline, not `#`
                let after = t.end;
                if !Self::space_after(src, after) {
                    continue;
                }
                // Find end of whitespace run
                let mut j = after;
                let bytes = src.as_bytes();
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                if j >= bytes.len() || bytes[j] == b'\n' || bytes[j] == b'#' {
                    continue;
                }
                self.emit_extraneous(ctx, after, j, out);
            } else {
                // `)` preceded by space on same line, previous non-space not newline
                let bytes = src.as_bytes();
                if t.begin == 0 {
                    continue;
                }
                if !matches!(bytes[t.begin - 1], b' ' | b'\t') {
                    continue;
                }
                let mut j = t.begin;
                while j > 0 && matches!(bytes[j - 1], b' ' | b'\t') {
                    j -= 1;
                }
                if j == 0 || bytes[j - 1] == b'\n' {
                    continue;
                }
                if bytes[j - 1] == b'(' {
                    // empty parens "( )" — already flagged on the `(` side
                    continue;
                }
                self.emit_extraneous(ctx, j, t.begin, out);
            }
        }
    }

    fn run_space(&self, toks: &[ParenTok], src: &str, ctx: &CheckContext, out: &mut Vec<Offense>) {
        self.space_or_compact(toks, src, ctx, out, false);
    }

    fn run_compact(&self, toks: &[ParenTok], src: &str, ctx: &CheckContext, out: &mut Vec<Offense>) {
        self.space_or_compact(toks, src, ctx, out, true);
    }

    fn space_or_compact(
        &self,
        toks: &[ParenTok],
        src: &str,
        ctx: &CheckContext,
        out: &mut Vec<Offense>,
        compact: bool,
    ) {
        let bytes = src.as_bytes();

        for &t in toks {
            if t.is_left {
                // Peek next non-blank on same line
                let mut j = t.end;
                let mut saw_space = false;
                while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
                    j += 1;
                    saw_space = true;
                }
                if j >= bytes.len() || bytes[j] == b'\n' || bytes[j] == b'#' {
                    continue;
                }
                // Empty parens: next is `)` immediately — flag extraneous if space; else skip
                if bytes[j] == b')' {
                    if saw_space {
                        self.emit_extraneous(ctx, t.end, j, out);
                    }
                    continue;
                }
                // Compact: if next is another `(`, forbid single space between them
                if compact && bytes[j] == b'(' {
                    if saw_space {
                        self.emit_extraneous(ctx, t.end, j, out);
                    }
                    continue;
                }
                // Normal rule: require at least one space after `(`
                if !saw_space {
                    self.emit_missing(ctx, t.end, t.end + 1, t.end, out);
                }
            } else {
                // `)` — peek previous non-blank on same line
                let mut j = t.begin;
                let mut saw_space = false;
                while j > 0 && matches!(bytes[j - 1], b' ' | b'\t') {
                    j -= 1;
                    saw_space = true;
                }
                if j == 0 || bytes[j - 1] == b'\n' {
                    continue;
                }
                let prev = bytes[j - 1];
                if prev == b'(' {
                    // empty parens — handled on left side
                    continue;
                }
                if compact && prev == b')' {
                    if saw_space {
                        self.emit_extraneous(ctx, j, t.begin, out);
                    }
                    continue;
                }
                if !saw_space {
                    self.emit_missing(ctx, t.begin, t.end, t.begin, out);
                }
            }
        }
    }
}

crate::register_cop!("Layout/SpaceInsideParens", |cfg| {
    let c = cfg.get_cop_config("Layout/SpaceInsideParens");
    let style = c
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "space" => SpaceInsideParensStyle::Space,
            "compact" => SpaceInsideParensStyle::Compact,
            _ => SpaceInsideParensStyle::NoSpace,
        })
        .unwrap_or(SpaceInsideParensStyle::NoSpace);
    Some(Box::new(SpaceInsideParens::new(style)))
});
