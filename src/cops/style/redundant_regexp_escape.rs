//! Style/RedundantRegexpEscape - Detects redundant backslash escapes inside regexp literals.

use crate::cops::{CheckContext, Cop};
use crate::helpers::escape::{Delimiters, is_interpolation_start, skip_interpolation};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

pub struct RedundantRegexpEscape;

impl RedundantRegexpEscape {
    pub fn new() -> Self {
        Self
    }
}

fn is_meaningful_escape(
    ch: char,
    in_char_class: bool,
    char_class_first: bool,
    char_class_last: bool,
    delimiters: &Delimiters,
    preceded_by_hash: bool,
) -> bool {
    match ch {
        '\\' | '\n' | '#' | ' ' => return true,
        _ if ch.is_ascii_alphanumeric() => return true,
        '@' | '$' if preceded_by_hash => return true,
        c if c == delimiters.close => return true,
        c if delimiters.open != delimiters.close && c == delimiters.open => return true,
        _ => {}
    }

    if in_char_class {
        match ch {
            '[' | ']' | '^' => true,
            '-' => !char_class_first && !char_class_last,
            _ => false,
        }
    } else {
        matches!(ch, '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$')
    }
}

fn find_redundant_escapes(
    source: &str,
    content_start: usize,
    content_end: usize,
    delimiters: &Delimiters,
    extended_mode: bool,
) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut results = Vec::new();
    let mut i = content_start;
    let mut char_class_depth: usize = 0;

    while i < content_end {
        let b = bytes[i];

        if extended_mode && char_class_depth == 0 && b == b'#' {
            while i < content_end && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        if is_interpolation_start(bytes, i, content_end) {
            i = skip_interpolation(bytes, i, content_end);
            continue;
        }

        if b == b'\\' && i + 1 < content_end {
            let next = bytes[i + 1];
            let ch = next as char;

            let escape_len = if next < 0x80 {
                2
            } else {
                let remaining = &source[i + 1..content_end];
                if let Some(c) = remaining.chars().next() {
                    1 + c.len_utf8()
                } else {
                    2
                }
            };

            let preceded_by_hash = i > content_start && bytes[i - 1] == b'#';
            let in_cc = char_class_depth > 0;
            let (cc_first, cc_last) = if in_cc && ch == '-' {
                (
                    is_first_in_char_class(bytes, i),
                    is_last_in_char_class(bytes, content_end, i, escape_len),
                )
            } else {
                (false, false)
            };

            if next >= 0x80 {
                results.push((i, escape_len));
                i += escape_len;
                continue;
            }

            if !is_meaningful_escape(ch, in_cc, cc_first, cc_last, delimiters, preceded_by_hash) {
                results.push((i, escape_len));
            }

            i += escape_len;
            continue;
        }

        if b == b'[' {
            if char_class_depth == 0 {
                char_class_depth = 1;
                i += 1;
                if i < content_end && bytes[i] == b'^' {
                    i += 1;
                }
                continue;
            }
            // POSIX class [:...:]
            if i + 1 < content_end && bytes[i + 1] == b':' {
                let mut j = i + 2;
                while j + 1 < content_end {
                    if bytes[j] == b':' && bytes[j + 1] == b']' {
                        j += 2;
                        break;
                    }
                    j += 1;
                }
                i = j;
                continue;
            }
            char_class_depth += 1;
            i += 1;
            if i < content_end && bytes[i] == b'^' {
                i += 1;
            }
            continue;
        }

        if b == b']' && char_class_depth > 0 {
            char_class_depth -= 1;
            i += 1;
            continue;
        }

        i += 1;
    }

    results
}

fn is_first_in_char_class(bytes: &[u8], esc_pos: usize) -> bool {
    if esc_pos == 0 { return false; }
    let prev = esc_pos - 1;
    if bytes[prev] == b'[' && (prev == 0 || bytes[prev - 1] != b'\\') {
        return true;
    }
    if bytes[prev] == b'^' && prev > 0 && bytes[prev - 1] == b'[' && (prev < 2 || bytes[prev - 2] != b'\\') {
        return true;
    }
    false
}

fn is_last_in_char_class(bytes: &[u8], content_end: usize, esc_pos: usize, esc_len: usize) -> bool {
    let after = esc_pos + esc_len;
    after < content_end && bytes[after] == b']'
}

impl Cop for RedundantRegexpEscape {
    fn name(&self) -> &'static str {
        "Style/RedundantRegexpEscape"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = Visitor {
            source: ctx.source,
            filename: ctx.filename,
            offenses: Vec::new(),
        };
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    source: &'a str,
    filename: &'a str,
    offenses: Vec<Offense>,
}

impl Visitor<'_> {
    fn process_regexp(
        &mut self,
        opening_start: usize,
        opening_end: usize,
        content_start: usize,
        content_end: usize,
        closing_start: usize,
        closing_end: usize,
    ) {
        let opening_text = &self.source[opening_start..opening_end];
        let delimiters = Delimiters::from_opening(opening_text, "%r", '/');
        let extended = self.source[closing_start..closing_end].contains('x');

        for (offset, esc_len) in find_redundant_escapes(self.source, content_start, content_end, &delimiters, extended) {
            let location = Location::from_offsets(self.source, offset, offset + esc_len);
            self.offenses.push(
                Offense::new(
                    "Style/RedundantRegexpEscape",
                    "Redundant escape inside regexp literal".to_string(),
                    Severity::Convention,
                    location,
                    self.filename,
                )
                .with_correction(Correction::delete(offset, offset + 1)),
            );
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let opening = node.opening_loc();
        let content = node.content_loc();
        let closing = node.closing_loc();
        self.process_regexp(
            opening.start_offset(), opening.end_offset(),
            content.start_offset(), content.end_offset(),
            closing.start_offset(), closing.end_offset(),
        );
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        self.process_regexp(
            opening.start_offset(), opening.end_offset(),
            opening.end_offset(), closing.start_offset(),
            closing.start_offset(), closing.end_offset(),
        );

        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = part {
                ruby_prism::visit_embedded_statements_node(
                    self,
                    &part.as_embedded_statements_node().unwrap(),
                );
            }
        }
    }
}
