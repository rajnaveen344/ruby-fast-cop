//! Port of RuboCop's `CheckSingleLineSuitability` mixin.
//! See https://raw.githubusercontent.com/rubocop/rubocop/v1.85.0/lib/rubocop/cop/mixin/check_single_line_suitability.rb
//!
//! Provides three primitives used by `Layout/RedundantLineBreak`:
//! - `to_single_line(src)` collapses line breaks (with the same regex pipeline as RuboCop)
//! - `safe_to_split(node)` rejects nodes whose descendants forbid single-lining
//! - `comment_within(line_range, comments)` checks if comments fall within a node

use ruby_prism::{Node, Visit};

/// Mirrors RuboCop's `to_single_line`. Order matters.
pub fn to_single_line(source: &str) -> String {
    // Step 1: Double quote, backslash, then single quote: `" \n\s*'` -> `" + '`
    let s = re_replace_dq_bs_sq(source);
    // Step 2: Single quote, backslash, then double quote
    let s = re_replace_sq_bs_dq(&s);
    // Step 3: Same-quote string concatenation `("|') *\\\n\s*\1` -> ``
    let s = re_replace_same_quote_concat(&s);
    // Step 4: chain `\n\s*(?=&?\.\w)` -> ``
    let s = re_replace_chain_break(&s);
    // Step 5: any other `\s*\\?\n\s*` -> ` `
    re_replace_any_break(&s)
}

fn re_replace_dq_bs_sq(s: &str) -> String {
    // pattern: `"` then optional spaces, `\`, `\n`, optional whitespace, `'`
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if let Some(end) = match_pattern(bytes, i, b'"', b'\'') {
                out.push_str("\" + '");
                i = end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn re_replace_sq_bs_dq(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if let Some(end) = match_pattern(bytes, i, b'\'', b'"') {
                out.push_str("' + \"");
                i = end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Match `Q *\\\n\s*Q2` where Q is `bytes[start]` and Q2 is provided.
/// Returns end-index after Q2.
fn match_pattern(bytes: &[u8], start: usize, q: u8, q2: u8) -> Option<usize> {
    if bytes.get(start) != Some(&q) {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    if bytes.get(i) != Some(&b'\\') {
        return None;
    }
    i += 1;
    if bytes.get(i) != Some(&b'\n') {
        return None;
    }
    i += 1;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if bytes.get(i) != Some(&q2) {
        return None;
    }
    Some(i + 1)
}

fn re_replace_same_quote_concat(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            // Try to match `<q> *\\\n\s*<q>`
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] == b' ' {
                j += 1;
            }
            if bytes.get(j) == Some(&b'\\') && bytes.get(j + 1) == Some(&b'\n') {
                let mut k = j + 2;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if bytes.get(k) == Some(&c) {
                    // Consume entire match producing nothing
                    i = k + 1;
                    continue;
                }
            }
        }
        out.push(c as char);
        i += 1;
    }
    out
}

fn re_replace_chain_break(s: &str) -> String {
    // pattern: `\n\s*` followed (lookahead) by `(&)?\.\w` - replace with empty
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // Look ahead: skip whitespace, then check `&?\.<word>`
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            let mut k = j;
            if bytes.get(k) == Some(&b'&') {
                k += 1;
            }
            if bytes.get(k) == Some(&b'.') {
                k += 1;
                if k < bytes.len() && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_') {
                    // Match: skip from i to j (exclusive of `&?\.`)
                    i = j;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn re_replace_any_break(s: &str) -> String {
    // `\s*\\?\n\s*` -> ` `
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        // Try to match `\s*\\?\n\s*`. Must contain at least one `\n`.
        // Scan: any horizontal+vertical ws, optionally `\\\n`, then more ws.
        // Algorithm: find a window of whitespace+optional-backslash-newline that contains
        // at least one `\n` (with or without preceding `\`).
        let start = i;
        let mut j = i;
        // consume leading whitespace
        while j < bytes.len() && is_ws(bytes[j]) {
            j += 1;
        }
        let mut had_newline = false;
        if j > i && bytes[..j].contains(&b'\n') {
            had_newline = true;
        }
        // optional `\\\n`
        if bytes.get(j) == Some(&b'\\') && bytes.get(j + 1) == Some(&b'\n') {
            j += 2;
            had_newline = true;
            // trailing whitespace
            while j < bytes.len() && is_ws(bytes[j]) {
                j += 1;
            }
        }
        if had_newline {
            out.push(' ');
            i = j;
            continue;
        }
        // No match — emit char and advance.
        out.push(bytes[start] as char);
        i = start + 1;
    }
    out
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Check if any line within `[first_line, last_line]` (1-indexed) contains a `#` comment
/// outside of strings. Falls back to a regex-free string-aware scan.
pub fn comment_within(source: &str, first_line: usize, last_line: usize) -> bool {
    let mut line_no = 1usize;
    let mut in_str = false;
    let mut delim = b'"';
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if line_no > last_line {
            break;
        }
        match b {
            b'\n' => { line_no += 1; in_str = false; }
            b'"' | b'\'' if !in_str => { in_str = true; delim = b; }
            c if in_str && c == delim => { in_str = false; }
            b'\\' if in_str => { i += 1; }
            b'#' if !in_str && line_no >= first_line => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Walk descendants of `node` looking for shapes that forbid single-lining.
/// Mirrors RuboCop:
///   node.each_descendant(:if, :case, :kwbegin, :any_def).none? &&
///   node.each_descendant(:dstr, :str).none? { |n| n.heredoc? || n.value.include?("\n") } &&
///   node.each_descendant(:begin, :sym).none?(&:multiline?)
pub fn safe_to_split(node: &Node<'_>, source: &str) -> bool {
    let mut v = SafetyVisitor { source, depth: 0, unsafe_: false };
    visit_dispatch(&mut v, node);
    !v.unsafe_
}

struct SafetyVisitor<'a> {
    source: &'a str,
    depth: usize,
    unsafe_: bool,
}

impl<'a> SafetyVisitor<'a> {
    fn check_string_node(&mut self, n: &ruby_prism::StringNode) {
        // heredoc?
        if let Some(open) = n.opening_loc() {
            if open.as_slice().starts_with(b"<<") {
                self.unsafe_ = true;
                return;
            }
        }
        // value contains "\n"?
        let v = n.unescaped();
        if v.contains(&b'\n') {
            self.unsafe_ = true;
        }
    }

    fn check_interpolated_string(&mut self, n: &ruby_prism::InterpolatedStringNode) {
        if let Some(open) = n.opening_loc() {
            if open.as_slice().starts_with(b"<<") {
                self.unsafe_ = true;
                return;
            }
        }
        // RuboCop checks `n.value.include?("\n")`. For dstr (interpolated string),
        // value is the concatenation of static-string parts. Interpolated parts have
        // no static value but don't introduce `\n` syntactically; we just check the
        // string parts for embedded newlines.
        for part in n.parts().iter() {
            if let Some(s) = part.as_string_node() {
                if s.unescaped().contains(&b'\n') {
                    self.unsafe_ = true;
                    return;
                }
            }
        }
    }
}

impl<'a, 'b> Visit<'b> for SafetyVisitor<'a> {
    fn visit_branch_node_enter(&mut self, _node: Node<'b>) { self.depth += 1; }
    fn visit_branch_node_leave(&mut self) { self.depth -= 1; }
    fn visit_leaf_node_enter(&mut self, _node: Node<'b>) { self.depth += 1; }
    fn visit_leaf_node_leave(&mut self) { self.depth -= 1; }
    fn visit_if_node(&mut self, n: &ruby_prism::IfNode) {
        if self.depth > 1 {
            // Skip ternaries (no `end` keyword)
            if n.end_keyword_loc().is_some() {
                self.unsafe_ = true;
                return;
            }
        }
        ruby_prism::visit_if_node(self, n);
    }
    fn visit_case_node(&mut self, n: &ruby_prism::CaseNode) {
        if self.depth > 1 {
            self.unsafe_ = true;
            return;
        }
        ruby_prism::visit_case_node(self, n);
    }
    fn visit_case_match_node(&mut self, n: &ruby_prism::CaseMatchNode) {
        if self.depth > 1 {
            self.unsafe_ = true;
            return;
        }
        ruby_prism::visit_case_match_node(self, n);
    }
    fn visit_begin_node(&mut self, n: &ruby_prism::BeginNode) {
        if self.depth > 1 {
            if n.begin_keyword_loc().is_some() {
                self.unsafe_ = true;
                return;
            }
            let loc = n.location();
            let span = &self.source.as_bytes()[loc.start_offset()..loc.end_offset()];
            if span.contains(&b'\n') {
                self.unsafe_ = true;
                return;
            }
        }
        ruby_prism::visit_begin_node(self, n);
    }
    fn visit_statements_node(&mut self, n: &ruby_prism::StatementsNode) {
        if self.depth > 1 {
            let count = n.body().iter().count();
            if count > 1 {
                let loc = n.location();
                let span = &self.source.as_bytes()[loc.start_offset()..loc.end_offset()];
                if span.contains(&b'\n') {
                    self.unsafe_ = true;
                    return;
                }
            }
        }
        ruby_prism::visit_statements_node(self, n);
    }
    fn visit_def_node(&mut self, n: &ruby_prism::DefNode) {
        if self.depth > 1 {
            self.unsafe_ = true;
            return;
        }
        ruby_prism::visit_def_node(self, n);
    }
    fn visit_string_node(&mut self, n: &ruby_prism::StringNode) {
        self.check_string_node(n);
        ruby_prism::visit_string_node(self, n);
    }
    fn visit_interpolated_string_node(&mut self, n: &ruby_prism::InterpolatedStringNode) {
        self.check_interpolated_string(n);
        ruby_prism::visit_interpolated_string_node(self, n);
    }
    fn visit_symbol_node(&mut self, n: &ruby_prism::SymbolNode) {
        let loc = n.location();
        let span = &self.source.as_bytes()[loc.start_offset()..loc.end_offset()];
        if span.contains(&b'\n') {
            self.unsafe_ = true;
        }
        ruby_prism::visit_symbol_node(self, n);
    }
    fn visit_interpolated_symbol_node(&mut self, n: &ruby_prism::InterpolatedSymbolNode) {
        let loc = n.location();
        let span = &self.source.as_bytes()[loc.start_offset()..loc.end_offset()];
        if span.contains(&b'\n') {
            self.unsafe_ = true;
        }
        ruby_prism::visit_interpolated_symbol_node(self, n);
    }
}

fn visit_dispatch<'b>(v: &mut SafetyVisitor<'_>, node: &Node<'b>) {
    v.visit(node);
}
