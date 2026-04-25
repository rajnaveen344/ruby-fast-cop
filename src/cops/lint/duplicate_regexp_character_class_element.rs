//! Lint/DuplicateRegexpCharacterClassElement - flag duplicate elements within `[...]`.
//!
//! Mirrors `RuboCop::Cop::Lint::DuplicateRegexpCharacterClassElement`.
//! Walks each character class in regexp content, tokenizes elements
//! (chars, escapes, ranges, POSIX classes), and flags duplicates by
//! source-text equality. Skips intersection classes (`[ab&&ab]`).
//!
//! For interpolated regexps, content within `#{...}` is ignored
//! (interpolated parts vary at runtime).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use std::collections::HashSet;

#[derive(Default)]
pub struct DuplicateRegexpCharacterClassElement;

impl DuplicateRegexpCharacterClassElement {
    pub fn new() -> Self { Self }
}

const MSG: &str = "Duplicate element inside regexp character class";

impl Cop for DuplicateRegexpCharacterClassElement {
    fn name(&self) -> &'static str { "Lint/DuplicateRegexpCharacterClassElement" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = V { ctx, out: vec![] };
        ruby_prism::Visit::visit(&mut v, &result.node());
        v.out
    }
}

struct V<'a, 'b> { ctx: &'a CheckContext<'b>, out: Vec<Offense> }

/// Span of a region inside the regexp pattern that is interpolation
/// (`#{...}`). Recorded as absolute byte offsets in source.
#[derive(Clone, Copy)]
struct InterpSpan { start: usize, end: usize }

impl<'a, 'b> V<'a, 'b> {
    fn process(&mut self, content_start: usize, content_end: usize, interp: &[InterpSpan]) {
        let bytes = self.ctx.source.as_bytes();
        let mut i = content_start;

        while i < content_end {
            // Skip interpolation regions.
            if let Some(span) = interp.iter().find(|s| s.start == i) {
                i = span.end;
                continue;
            }
            // Skip escapes outside char class.
            if bytes[i] == b'\\' && i + 1 < content_end {
                i += 2;
                continue;
            }
            if bytes[i] == b'[' {
                // Possible POSIX `[:...:]` outside any `[...]` shouldn't exist,
                // but just in case treat as char class.
                let class_start = i + 1;
                let mut j = class_start;
                // Skip leading `^` for negated class.
                if j < content_end && bytes[j] == b'^' { j += 1; }
                let elements_start = j;
                // Find matching `]` for this top-level char class. Inside
                // we have to handle escapes, nested POSIX `[:...:]`, and
                // nested char classes (rare but allowed).
                let class_end_idx = find_class_end(bytes, j, content_end);
                if class_end_idx == content_end {
                    // unbalanced - skip
                    i = content_end;
                    continue;
                }
                // Tokenize elements between `elements_start` and `class_end_idx`.
                let tokens = tokenize_class(bytes, elements_start, class_end_idx, interp);
                let is_intersection = tokens.iter().any(|t| matches!(t.kind, TokenKind::Intersection));
                if !is_intersection {
                    let mut seen: HashSet<&str> = HashSet::new();
                    for tok in &tokens {
                        if matches!(tok.kind, TokenKind::Intersection | TokenKind::Interp) { continue; }
                        let text = &self.ctx.source[tok.start..tok.end];
                        if seen.contains(text) {
                            self.out.push(
                                self.ctx.offense_with_range(
                                    "Lint/DuplicateRegexpCharacterClassElement",
                                    MSG,
                                    Severity::Warning,
                                    tok.start, tok.end,
                                ).with_correction(Correction::delete(tok.start, tok.end)),
                            );
                        } else {
                            seen.insert(text);
                        }
                    }
                }
                let _ = class_start;
                i = class_end_idx + 1;
                continue;
            }
            i += 1;
        }
    }
}

#[derive(Clone, Copy)]
enum TokenKind { Element, Intersection, Interp }

#[derive(Clone, Copy)]
struct Token { start: usize, end: usize, kind: TokenKind }

/// Find the byte offset of the closing `]` for a character class whose
/// elements start at `start` (i.e. after `[` and any leading `^`).
/// Honors escapes, nested POSIX `[:...:]` blocks, and nested char classes.
fn find_class_end(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 0; // depth of nested `[...]`
    while i < end {
        let b = bytes[i];
        if b == b'\\' && i + 1 < end {
            i += 2;
            continue;
        }
        if b == b'[' {
            // POSIX `[:...:]`?
            if i + 1 < end && bytes[i + 1] == b':' {
                let mut j = i + 2;
                while j + 1 < end {
                    if bytes[j] == b':' && bytes[j + 1] == b']' { j += 2; break; }
                    j += 1;
                }
                i = j;
                continue;
            }
            depth += 1;
            i += 1;
            continue;
        }
        if b == b']' {
            if depth == 0 { return i; }
            depth -= 1;
            i += 1;
            continue;
        }
        i += 1;
    }
    end
}

/// Tokenize elements inside a character class.
/// `start` = first byte after `[` (or `[^`), `end` = position of `]`.
fn tokenize_class(bytes: &[u8], start: usize, end: usize, interp: &[InterpSpan]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = start;

    while i < end {
        // Interpolation span?
        if let Some(span) = interp.iter().find(|s| s.start == i) {
            out.push(Token { start: span.start, end: span.end, kind: TokenKind::Interp });
            i = span.end;
            continue;
        }

        // Intersection `&&`
        if bytes[i] == b'&' && i + 1 < end && bytes[i + 1] == b'&' {
            out.push(Token { start: i, end: i + 2, kind: TokenKind::Intersection });
            i += 2;
            continue;
        }

        // POSIX class `[:name:]` or nested char class `[...]`
        if bytes[i] == b'[' {
            if i + 1 < end && bytes[i + 1] == b':' {
                // POSIX class. Find `:]`.
                let posix_start = i;
                let mut j = i + 2;
                while j + 1 < end {
                    if bytes[j] == b':' && bytes[j + 1] == b']' { j += 2; break; }
                    j += 1;
                }
                // After POSIX, check for range `[:alpha:]-X`.
                if j < end && bytes[j] == b'-' && j + 1 < end && bytes[j + 1] != b']' {
                    let after = parse_atom_end(bytes, j + 1, end, interp);
                    out.push(Token { start: posix_start, end: after, kind: TokenKind::Element });
                    i = after;
                } else {
                    out.push(Token { start: posix_start, end: j, kind: TokenKind::Element });
                    i = j;
                }
                continue;
            }
            // Nested char class: treat the entire `[...]` as one element.
            let inner_start = i + 1;
            let mut j = inner_start;
            if j < end && bytes[j] == b'^' { j += 1; }
            let inner_end = find_class_end(bytes, j, end);
            let nested_end = if inner_end < end { inner_end + 1 } else { end };
            out.push(Token { start: i, end: nested_end, kind: TokenKind::Element });
            i = nested_end;
            continue;
        }

        // Atom (escape, char), possibly forming a range with `-`.
        let atom_start = i;
        let atom_end = parse_atom_end(bytes, i, end, interp);
        if atom_end < end && bytes[atom_end] == b'-' && atom_end + 1 < end && bytes[atom_end + 1] != b']' {
            // `-` is part of a range only if not at the end of the class.
            let next_atom_start = atom_end + 1;
            let next_atom_end = parse_atom_end(bytes, next_atom_start, end, interp);
            out.push(Token { start: atom_start, end: next_atom_end, kind: TokenKind::Element });
            i = next_atom_end;
        } else {
            out.push(Token { start: atom_start, end: atom_end, kind: TokenKind::Element });
            i = atom_end;
        }
    }

    out
}

/// Determine the byte length of one atom starting at `i`.
/// Atoms: escape sequences (with octal/hex/unicode handling) or single chars.
fn parse_atom_end(bytes: &[u8], i: usize, end: usize, interp: &[InterpSpan]) -> usize {
    if i >= end { return i; }
    if let Some(span) = interp.iter().find(|s| s.start == i) {
        return span.end;
    }
    if bytes[i] == b'\\' && i + 1 < end {
        let next = bytes[i + 1];
        // Octal `\NNN` — up to 3 octal digits.
        if (b'0'..=b'7').contains(&next) {
            let mut j = i + 1;
            let mut count = 0;
            while j < end && count < 3 && (b'0'..=b'7').contains(&bytes[j]) {
                j += 1;
                count += 1;
            }
            return j;
        }
        // `\xNN` hex (1-2 digits)
        if next == b'x' {
            let mut j = i + 2;
            let mut count = 0;
            while j < end && count < 2 && bytes[j].is_ascii_hexdigit() {
                j += 1;
                count += 1;
            }
            return j;
        }
        // `\uNNNN` or `\u{...}`
        if next == b'u' {
            if i + 2 < end && bytes[i + 2] == b'{' {
                let mut j = i + 3;
                while j < end && bytes[j] != b'}' { j += 1; }
                if j < end { j += 1; }
                return j;
            }
            let mut j = i + 2;
            let mut count = 0;
            while j < end && count < 4 && bytes[j].is_ascii_hexdigit() {
                j += 1;
                count += 1;
            }
            return j;
        }
        // `\cX` or `\C-X`
        if next == b'c' {
            return (i + 3).min(end);
        }
        if next == b'C' && i + 2 < end && bytes[i + 2] == b'-' {
            // `\C-X` -- 4 bytes, but X may itself be escape; simplify
            return (i + 4).min(end);
        }
        // High UTF-8 byte after backslash
        if next >= 0x80 {
            let s = std::str::from_utf8(&bytes[i + 1..end]).ok();
            if let Some(s) = s {
                if let Some(c) = s.chars().next() {
                    return i + 1 + c.len_utf8();
                }
            }
        }
        return i + 2;
    }
    // UTF-8 character
    if bytes[i] >= 0x80 {
        let s = std::str::from_utf8(&bytes[i..end]).ok();
        if let Some(s) = s {
            if let Some(c) = s.chars().next() {
                return i + c.len_utf8();
            }
        }
    }
    i + 1
}

impl<'a, 'b> ruby_prism::Visit<'_> for V<'a, 'b> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let c = node.content_loc();
        self.process(c.start_offset(), c.end_offset(), &[]);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        // Build interp spans from EmbeddedStatements parts.
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        let content_start = opening.end_offset();
        let content_end = closing.start_offset();

        let mut spans = Vec::new();
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = part {
                let es = part.as_embedded_statements_node().unwrap();
                let l = es.location();
                spans.push(InterpSpan { start: l.start_offset(), end: l.end_offset() });
            }
        }
        self.process(content_start, content_end, &spans);
    }
}

crate::register_cop!("Lint/DuplicateRegexpCharacterClassElement", |_cfg| {
    Some(Box::new(DuplicateRegexpCharacterClassElement::new()))
});
