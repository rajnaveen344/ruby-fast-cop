//! Lint/MixedCaseRange - flag character ranges spanning upper and lower
//! case ASCII (e.g. `A-z`), both in regexp character classes and in
//! `Range` objects (`'A'..'z'`).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct MixedCaseRange;

impl MixedCaseRange {
    pub fn new() -> Self { Self }
}

const MSG: &str = "Ranges from upper to lower case ASCII letters may include unintended characters. Instead of `A-z` (which also includes several symbols) specify each range individually: `A-Za-z` and individually specify any symbols.";

impl Cop for MixedCaseRange {
    fn name(&self) -> &'static str { "Lint/MixedCaseRange" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = V { ctx, out: vec![] };
        v.visit(&result.node());
        v.out
    }
}

/// Which `[a-z]` / `[A-Z]` group a single ASCII character belongs to.
fn group_for(b: u8) -> Option<u8> {
    if (b'a'..=b'z').contains(&b) { Some(0) }
    else if (b'A'..=b'Z').contains(&b) { Some(1) }
    else { None }
}

fn unsafe_pair(a: u8, b: u8) -> bool {
    matches!((group_for(a), group_for(b)), (Some(x), Some(y)) if x != y)
}

struct V<'a, 'b> { ctx: &'a CheckContext<'b>, out: Vec<Offense> }

impl<'a, 'b> V<'a, 'b> {
    fn process_regexp(&mut self, content_start: usize, content_end: usize, interp: &[(usize, usize)]) {
        let bytes = self.ctx.source.as_bytes();
        let mut i = content_start;

        while i < content_end {
            // Skip interpolation regions.
            if let Some(span) = interp.iter().find(|(s, _)| *s == i) {
                i = span.1;
                continue;
            }
            // Skip escapes outside char class.
            if bytes[i] == b'\\' && i + 1 < content_end {
                i += 2;
                continue;
            }
            if bytes[i] == b'[' {
                let class_start = i + 1;
                let mut j = class_start;
                if j < content_end && bytes[j] == b'^' { j += 1; }
                let cls_end = find_class_end(bytes, j, content_end);
                self.scan_class(j, cls_end, interp);
                i = if cls_end < content_end { cls_end + 1 } else { content_end };
                continue;
            }
            i += 1;
        }
    }

    fn scan_class(&mut self, start: usize, end: usize, interp: &[(usize, usize)]) {
        let bytes = self.ctx.source.as_bytes();
        let mut i = start;
        while i < end {
            if let Some(span) = interp.iter().find(|(s, _)| *s == i) {
                i = span.1;
                continue;
            }
            // POSIX [:...:]
            if bytes[i] == b'[' && i + 1 < end && bytes[i + 1] == b':' {
                let mut j = i + 2;
                while j + 1 < end {
                    if bytes[j] == b':' && bytes[j + 1] == b']' { j += 2; break; }
                    j += 1;
                }
                i = j;
                continue;
            }
            // Nested class — recurse.
            if bytes[i] == b'[' {
                let inner_start = i + 1;
                let mut j = inner_start;
                if j < end && bytes[j] == b'^' { j += 1; }
                let inner_end = find_class_end(bytes, j, end);
                self.scan_class(j, inner_end, interp);
                i = if inner_end < end { inner_end + 1 } else { end };
                continue;
            }
            // Try to detect range: <atom> '-' <atom>
            let atom1_start = i;
            let atom1_end = atom_end(bytes, i, end);
            if atom1_end < end && bytes[atom1_end] == b'-' && atom1_end + 1 < end && bytes[atom1_end + 1] != b']' {
                let atom2_start = atom1_end + 1;
                let atom2_end = atom_end(bytes, atom2_start, end);
                // Check both atoms are simple literal single-byte ASCII letter.
                let a = simple_letter(bytes, atom1_start, atom1_end);
                let b = simple_letter(bytes, atom2_start, atom2_end);
                if let (Some(la), Some(lb)) = (a, b) {
                    if unsafe_pair(la, lb) {
                        self.out.push(self.ctx.offense_with_range(
                            "Lint/MixedCaseRange",
                            MSG,
                            Severity::Warning,
                            atom1_start, atom2_end,
                        ));
                    }
                }
                i = atom2_end;
                continue;
            }
            i = atom1_end;
        }
    }
}

/// Return Some(letter_byte) if the atom is a single un-escaped ASCII letter.
fn simple_letter(bytes: &[u8], s: usize, e: usize) -> Option<u8> {
    if e - s != 1 { return None; }
    let b = bytes[s];
    if b.is_ascii_alphabetic() { Some(b) } else { None }
}

fn atom_end(bytes: &[u8], i: usize, end: usize) -> usize {
    if i >= end { return i; }
    if bytes[i] == b'\\' && i + 1 < end {
        let next = bytes[i + 1];
        if (b'0'..=b'7').contains(&next) {
            let mut j = i + 1;
            let mut count = 0;
            while j < end && count < 3 && (b'0'..=b'7').contains(&bytes[j]) {
                j += 1;
                count += 1;
            }
            return j;
        }
        if next == b'x' {
            let mut j = i + 2;
            let mut count = 0;
            while j < end && count < 2 && bytes[j].is_ascii_hexdigit() {
                j += 1;
                count += 1;
            }
            return j;
        }
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
        if next == b'c' { return (i + 3).min(end); }
        if next == b'C' && i + 2 < end && bytes[i + 2] == b'-' { return (i + 4).min(end); }
        if next >= 0x80 {
            if let Ok(s) = std::str::from_utf8(&bytes[i + 1..end]) {
                if let Some(c) = s.chars().next() { return i + 1 + c.len_utf8(); }
            }
        }
        return i + 2;
    }
    if bytes[i] >= 0x80 {
        if let Ok(s) = std::str::from_utf8(&bytes[i..end]) {
            if let Some(c) = s.chars().next() { return i + c.len_utf8(); }
        }
    }
    i + 1
}

fn find_class_end(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut i = start;
    let mut depth: usize = 0;
    while i < end {
        let b = bytes[i];
        if b == b'\\' && i + 1 < end { i += 2; continue; }
        if b == b'[' {
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

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let c = node.content_loc();
        self.process_regexp(c.start_offset(), c.end_offset(), &[]);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for part in node.parts().iter() {
            if let Node::EmbeddedStatementsNode { .. } = part {
                let es = part.as_embedded_statements_node().unwrap();
                let l = es.location();
                spans.push((l.start_offset(), l.end_offset()));
            }
        }
        self.process_regexp(opening.end_offset(), closing.start_offset(), &spans);
    }

    fn visit_range_node(&mut self, node: &ruby_prism::RangeNode) {
        let (Some(l), Some(r)) = (node.left(), node.right()) else { return };
        // Both must be StringNode with single-character content.
        let Some(ls) = l.as_string_node() else { return };
        let Some(rs) = r.as_string_node() else { return };
        let lc = ls.content_loc();
        let rc = rs.content_loc();
        let lbytes = &self.ctx.source.as_bytes()[lc.start_offset()..lc.end_offset()];
        let rbytes = &self.ctx.source.as_bytes()[rc.start_offset()..rc.end_offset()];
        if lbytes.len() != 1 || rbytes.len() != 1 { return; }
        if unsafe_pair(lbytes[0], rbytes[0]) {
            let nloc = node.location();
            self.out.push(self.ctx.offense_with_range(
                "Lint/MixedCaseRange",
                MSG,
                Severity::Warning,
                nloc.start_offset(), nloc.end_offset(),
            ));
        }
    }
}

crate::register_cop!("Lint/MixedCaseRange", |_cfg| Some(Box::new(MixedCaseRange::new())));
