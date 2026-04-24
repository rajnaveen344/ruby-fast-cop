//! Style/RedundantLineContinuation
//!
//! Flags `\` at end-of-line when removing it doesn't break parsing.
//! Heuristics mirror RuboCop's token-based checks with Prism-friendly
//! approximations.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{ProgramNode, Visit};

const MSG: &str = "Redundant line continuation.";

#[derive(Default)]
pub struct RedundantLineContinuation;

impl RedundantLineContinuation {
    pub fn new() -> Self { Self }
}

impl Cop for RedundantLineContinuation {
    fn name(&self) -> &'static str { "Style/RedundantLineContinuation" }

    fn check_program(&self, node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let bytes_full = ctx.source.as_bytes();
        // Truncate scanning at `__END__` (start-of-line marker).
        let end_data_pos = find_end_data_marker(bytes_full);
        let bytes = &bytes_full[..end_data_pos];

        let mut string_ranges: Vec<(usize, usize)> = Vec::new();
        {
            let mut v = StringCollector { ranges: &mut string_ranges };
            v.visit_program_node(node);
        }

        let mut offenses = Vec::new();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'\\' && bytes[i + 1] == b'\n' {
                let line_start = find_line_start(bytes, i);
                let line_src = &bytes[line_start..i];

                // Skip scenarios that require the backslash.
                if has_uncommented_hash(line_src, line_start, &string_ranges)
                    || is_string_concat(line_src)
                    || inside_string(i, &string_ranges)
                    || next_line_starts_with_arith(bytes, i + 2)
                    || next_line_starts_with_binop(bytes, i + 2)
                    || method_with_arg_shape(line_src, bytes, i + 2)
                {
                    i += 1;
                    continue;
                }

                // Validity: replace `\` with ` ` (keep newline), reparse.
                let mut modified = Vec::with_capacity(bytes.len());
                modified.extend_from_slice(&bytes[..i]);
                modified.push(b' ');
                modified.extend_from_slice(&bytes[i + 1..]);
                let pr = ruby_prism::parse(&modified);
                if pr.errors().next().is_some() {
                    i += 1;
                    continue;
                }

                let correction = Correction::delete(i, i + 1);
                offenses.push(
                    ctx.offense_with_range(self.name(), MSG, Severity::Convention, i, i + 1)
                        .with_correction(correction),
                );
            }
            i += 1;
        }
        offenses
    }
}

fn find_end_data_marker(bytes: &[u8]) -> usize {
    // Match `__END__` at the start of a line, followed by `\n` or EOF.
    let needle = b"__END__";
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if at_line_start && &bytes[i..i + needle.len()] == needle {
            let after = bytes.get(i + needle.len()).copied().unwrap_or(b'\n');
            if after == b'\n' || after == 0 {
                return i;
            }
        }
        i += 1;
    }
    bytes.len()
}

fn find_line_start(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i > 0 && bytes[i - 1] != b'\n' { i -= 1; }
    i
}

fn inside_string(offset: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|(s, e)| offset > *s && offset < *e)
}

fn has_uncommented_hash(line_src: &[u8], line_start: usize, ranges: &[(usize, usize)]) -> bool {
    for (idx, b) in line_src.iter().enumerate() {
        if *b == b'#' && !inside_string(line_start + idx, ranges) {
            return true;
        }
    }
    false
}

fn is_string_concat(line_src: &[u8]) -> bool {
    let mut j = line_src.len();
    while j > 0 && matches!(line_src[j - 1], b' ' | b'\t') { j -= 1; }
    if j == 0 { return false; }
    matches!(line_src[j - 1], b'"' | b'\'')
}

fn next_line_starts_with_arith(bytes: &[u8], after_nl: usize) -> bool {
    let mut j = after_nl;
    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') { j += 1; }
    if j >= bytes.len() { return false; }
    let c = bytes[j];
    if !matches!(c, b'+' | b'-' | b'*' | b'/' | b'%') { return false; }
    // `%i[...]`, `%w[...]`, `%q{...}` etc. are percent literals, not modulo.
    // A letter or `!`/`?` following `%` signals a percent literal.
    if c == b'%' {
        let c2 = bytes.get(j + 1).copied().unwrap_or(0);
        if c2.is_ascii_alphabetic() { return false; }
    }
    // `/regex/` starting a line is likely a regex literal, but we can't tell
    // easily; keep the heuristic conservative.
    true
}

/// Next line's first non-ws character is a binary operator (`&&`, `||`, `?`, `:`).
/// These are conservative skips — RuboCop via Parser's `valid_syntax?` rejects the
/// join, so we do it structurally.
fn next_line_starts_with_binop(bytes: &[u8], after_nl: usize) -> bool {
    let mut j = after_nl;
    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') { j += 1; }
    if j >= bytes.len() { return false; }
    let c = bytes[j];
    if matches!(c, b'?' | b':' | b'=') { return true; }
    // `&&`, `||`, `&.`
    let c2 = bytes.get(j + 1).copied().unwrap_or(0);
    (c == b'&' && (c2 == b'&' || c2 == b'.')) || (c == b'|' && c2 == b'|')
}

fn method_with_arg_shape(line_src: &[u8], bytes: &[u8], after_nl: usize) -> bool {
    let mut j = line_src.len();
    while j > 0 && matches!(line_src[j - 1], b' ' | b'\t') { j -= 1; }
    if j == 0 { return false; }
    let last = line_src[j - 1];
    let id_tail = last.is_ascii_alphanumeric() || last == b'_' || last == b'?' || last == b'!';
    if !id_tail { return false; }

    let mut s = j;
    while s > 0 {
        let c = line_src[s - 1];
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'?' || c == b'!' { s -= 1; } else { break; }
    }
    let ident = &line_src[s..j];
    // Accept identifiers regardless of leading `.` (method chain) — RuboCop's
    // token-based check treats them uniformly via tIDENTIFIER.
    let _ = if s > 0 { Some(line_src[s - 1]) } else { None };
    // RuboCop only flags tIDENTIFIER / break / next / return / super / yield.
    // Constant-like names (start uppercase) and most keywords don't qualify.
    if ident[0].is_ascii_uppercase() { return false; }
    match ident {
        b"end" | b"do" | b"then" | b"else" | b"elsif" | b"begin" | b"rescue" | b"ensure"
        | b"when" | b"case" | b"if" | b"unless" | b"while" | b"until" | b"class" | b"module"
        | b"def" | b"and" | b"or" | b"not" | b"in" | b"nil" | b"true" | b"false" | b"self"
        | b"defined?" => return false,
        _ => {}
    }
    // Restrict to identifiers that can take arguments: any lowercase identifier,
    // plus `break/next/return/super/yield` (which we include in lowercase).
    // Additionally exclude trailing `?`/`!` which typically end a predicate call.

    let mut k = after_nl;
    while k < bytes.len() && matches!(bytes[k], b' ' | b'\t') { k += 1; }
    if k >= bytes.len() { return false; }
    let c = bytes[k];
    let c2 = bytes.get(k + 1).copied().unwrap_or(0);
    // `..N` / `...N` range literal is argument-like.
    if c == b'.' && c2 == b'.' { return true; }
    // `:foo` (symbol), xstring backtick, are argument-like.
    if c == b':' && c2 != b':' { return true; }
    if c == b'`' { return true; }
    // `%w[...]`, `%i[...]`, `%r{...}`, `%q{...}`, etc. are literals — argument-like.
    if c == b'%' && c2.is_ascii_alphabetic() { return true; }
    if matches!(
        c,
        b'.' | b',' | b')' | b']' | b'}' | b'+' | b'-' | b'/' | b'%' | b'&' | b'|' | b'^'
        | b'<' | b'>' | b'=' | b'?' | b':'
    ) {
        return false;
    }
    // `*` is splat operator (argument). `**` is double-splat.
    if c == b'*' { return true; }
    // If next line starts with a keyword that can't be an argument, reject.
    if c.is_ascii_alphabetic() || c == b'_' {
        // Extract identifier-like prefix.
        let mut e = k;
        while e < bytes.len()
            && (bytes[e].is_ascii_alphanumeric() || bytes[e] == b'_')
        { e += 1; }
        let ident = &bytes[k..e];
        match ident {
            b"end" | b"do" | b"then" | b"else" | b"elsif" | b"begin" | b"rescue" | b"ensure"
            | b"when" | b"case" | b"if" | b"unless" | b"while" | b"until" | b"class" | b"module"
            | b"in" | b"and" | b"or" | b"not" => return false,
            _ => {}
        }
    }
    c.is_ascii_alphanumeric() || c == b'_' || c == b'"' || c == b'\'' || c == b'(' || c == b'['
        || c == b'{' || c == b'@' || c == b'$' || c == b'!' || c == b'~'
}

struct StringCollector<'a> {
    ranges: &'a mut Vec<(usize, usize)>,
}

impl<'pr> Visit<'pr> for StringCollector<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        let loc = node.location();
        self.ranges.push((loc.start_offset(), loc.end_offset()));
        ruby_prism::visit_string_node(self, node);
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        let loc = node.location();
        self.ranges.push((loc.start_offset(), loc.end_offset()));
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
        let loc = node.location();
        self.ranges.push((loc.start_offset(), loc.end_offset()));
        ruby_prism::visit_x_string_node(self, node);
    }
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode<'pr>) {
        let loc = node.location();
        self.ranges.push((loc.start_offset(), loc.end_offset()));
        ruby_prism::visit_regular_expression_node(self, node);
    }
    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode<'pr>) {
        let loc = node.location();
        self.ranges.push((loc.start_offset(), loc.end_offset()));
        ruby_prism::visit_symbol_node(self, node);
    }
}

crate::register_cop!("Style/RedundantLineContinuation", |_cfg| Some(Box::new(RedundantLineContinuation::new())));
