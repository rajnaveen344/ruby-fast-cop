//! Style/RedundantRegexpEscape - Detects redundant backslash escapes inside regexp literals.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_regexp_escape.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

pub struct RedundantRegexpEscape;

impl RedundantRegexpEscape {
    pub fn new() -> Self {
        Self
    }
}

/// Delimiter info for a regexp literal
#[derive(Debug, Clone, Copy)]
struct RegexpDelimiters {
    open: char,
    close: char,
}

/// Get the matching closing delimiter for an opening bracket-type delimiter
fn closing_delimiter(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '<' => Some('>'),
        _ => None,
    }
}

/// Determine delimiters from the regexp opening text
fn get_delimiters(opening: &str) -> RegexpDelimiters {
    if opening.starts_with("%r") && opening.len() >= 3 {
        let delim_char = opening.as_bytes()[2] as char;
        if let Some(c) = closing_delimiter(delim_char) {
            RegexpDelimiters {
                open: delim_char,
                close: c,
            }
        } else {
            RegexpDelimiters {
                open: delim_char,
                close: delim_char,
            }
        }
    } else {
        // / ... / literal
        RegexpDelimiters {
            open: '/',
            close: '/',
        }
    }
}

/// Check if this is an extended-mode regexp (has `x` flag)
fn is_extended_mode(source: &str, closing_start: usize, closing_end: usize) -> bool {
    let closing = &source[closing_start..closing_end];
    closing.contains('x')
}

/// Returns true if this escape is meaningful (NOT redundant) in regexp context.
fn is_meaningful_escape(
    ch: char,
    in_char_class: bool,
    char_class_first: bool,
    char_class_last: bool,
    delimiters: &RegexpDelimiters,
    preceded_by_hash: bool,
) -> bool {
    // Escaped backslash is always meaningful
    if ch == '\\' {
        return true;
    }

    // Line continuation
    if ch == '\n' {
        return true;
    }

    // All alphabetic escapes are meaningful in regex
    if ch.is_ascii_alphabetic() {
        return true;
    }

    // Numeric: \0-\9 (backreferences, octal)
    if ch.is_ascii_digit() {
        return true;
    }

    // \# is always meaningful in regexp (prevents interpolation)
    if ch == '#' {
        return true;
    }

    // \@ and \$ after # prevent interpolation (#@var, #@@var, #$var)
    if preceded_by_hash && (ch == '@' || ch == '$') {
        return true;
    }

    // Escaped space is always meaningful in regexp (could be in free-space mode)
    if ch == ' ' {
        return true;
    }

    // Delimiter escapes are always meaningful
    if ch == delimiters.close {
        return true;
    }
    if delimiters.open != delimiters.close && ch == delimiters.open {
        return true;
    }

    // Context-dependent escapes
    if in_char_class {
        match ch {
            '[' | ']' | '^' => return true,
            '-' => {
                // \- is redundant when first or last in the char class
                // \- is meaningful when in the middle (between range endpoints)
                return !char_class_first && !char_class_last;
            }
            _ => return false,
        }
    } else {
        match ch {
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' => {
                return true;
            }
            '-' => return false,
            _ => return false,
        }
    }
}

/// Scan the content of a regexp literal for redundant escapes.
/// Returns (byte_offset_of_backslash, byte_length_of_escape_sequence) pairs.
fn find_redundant_escapes(
    source: &str,
    content_start: usize,
    content_end: usize,
    delimiters: &RegexpDelimiters,
    extended_mode: bool,
) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut results = Vec::new();
    let mut i = content_start;
    let mut char_class_depth: usize = 0;

    // We do two-pass for character classes to determine first/last position of \-
    // But actually we can do it in one pass by collecting elements and then deciding.
    // However, for simplicity, let's scan and collect char class elements, then decide.

    // Actually, the simplest approach: scan linearly, for \- inside char class,
    // we need to know if it's first or last. We'll use a helper that scans ahead.

    while i < content_end {
        let b = bytes[i];

        // Handle extended mode comments: # to end of line (outside char class)
        if extended_mode && char_class_depth == 0 && b == b'#' {
            // Skip to end of line (this is a comment, not an escape)
            while i < content_end && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Handle interpolation: #{...}
        if b == b'#' && i + 1 < content_end && bytes[i + 1] == b'{' {
            let mut depth = 1;
            let mut j = i + 2;
            while j < content_end && depth > 0 {
                if bytes[j] == b'{' {
                    depth += 1;
                } else if bytes[j] == b'}' {
                    depth -= 1;
                } else if bytes[j] == b'\\' && j + 1 < content_end {
                    j += 1;
                } else if bytes[j] == b'"' {
                    j += 1;
                    while j < content_end && bytes[j] != b'"' {
                        if bytes[j] == b'\\' && j + 1 < content_end {
                            j += 1;
                        }
                        j += 1;
                    }
                } else if bytes[j] == b'\'' {
                    j += 1;
                    while j < content_end && bytes[j] != b'\'' {
                        if bytes[j] == b'\\' && j + 1 < content_end {
                            j += 1;
                        }
                        j += 1;
                    }
                }
                j += 1;
            }
            i = j;
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

            // Check if preceded by # (for \@ and \$ handling)
            let preceded_by_hash = i > content_start && bytes[i - 1] == b'#';

            // For \- inside char class, determine first/last
            let in_cc = char_class_depth > 0;
            let (cc_first, cc_last) = if in_cc && ch == '-' {
                (
                    is_first_in_char_class_v2(bytes, content_start, i),
                    is_last_in_char_class_v2(bytes, content_end, i, escape_len),
                )
            } else {
                (false, false)
            };

            if next >= 0x80 {
                // Escaped multibyte character - always redundant
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

        // Track character class state
        if b == b'[' && char_class_depth == 0 {
            char_class_depth = 1;
            i += 1;
            // Skip ^ after [ if present
            if i < content_end && bytes[i] == b'^' {
                i += 1;
            }
            continue;
        }

        // Inside a char class, handle nested [ for:
        // - POSIX classes like [:alpha:]
        // - Nested char classes like [a-z&&[^0-9]]
        // - Intersection operator &&
        if char_class_depth > 0 && b == b'[' {
            // Check if this is a POSIX class [:...:]
            if i + 1 < content_end && bytes[i + 1] == b':' {
                // POSIX class - skip to closing :]
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
            // Nested character class
            char_class_depth += 1;
            i += 1;
            // Skip ^ after [ if present
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

/// Check if the escape at position `esc_pos` is the first element inside a character class.
/// "First" means right after `[` or `[^`, or after `\[` (escaped opening bracket).
fn is_first_in_char_class_v2(bytes: &[u8], _content_start: usize, esc_pos: usize) -> bool {
    if esc_pos == 0 {
        return false;
    }
    let prev = esc_pos - 1;
    // Right after [ or [^ — but make sure the [ is not escaped
    if bytes[prev] == b'[' {
        // Check it's not an escaped \[
        if prev == 0 || bytes[prev - 1] != b'\\' {
            return true;
        }
    }
    if bytes[prev] == b'^' && prev > 0 && bytes[prev - 1] == b'[' {
        // Check the [ is not escaped
        if prev < 2 || bytes[prev - 2] != b'\\' {
            return true;
        }
    }
    false
}

/// Check if the escape at position `esc_pos` with length `esc_len` is the last element
/// before the `]` that closes the character class.
fn is_last_in_char_class_v2(
    bytes: &[u8],
    content_end: usize,
    esc_pos: usize,
    esc_len: usize,
) -> bool {
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
        let mut visitor = RedundantRegexpEscapeVisitor {
            source: ctx.source,
            filename: ctx.filename,
            offenses: Vec::new(),
        };
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct RedundantRegexpEscapeVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    offenses: Vec<Offense>,
}

impl RedundantRegexpEscapeVisitor<'_> {
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
        let delimiters = get_delimiters(opening_text);
        let extended = is_extended_mode(self.source, closing_start, closing_end);

        let redundant = find_redundant_escapes(
            self.source,
            content_start,
            content_end,
            &delimiters,
            extended,
        );

        for (offset, esc_len) in redundant {
            let location = Location::from_offsets(self.source, offset, offset + esc_len);
            let message = "Redundant escape inside regexp literal".to_string();
            let correction = Correction::delete(offset, offset + 1);
            self.offenses.push(
                Offense::new(
                    "Style/RedundantRegexpEscape",
                    message,
                    Severity::Convention,
                    location,
                    self.filename,
                )
                .with_correction(correction),
            );
        }
    }
}

impl Visit<'_> for RedundantRegexpEscapeVisitor<'_> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let opening = node.opening_loc();
        let content = node.content_loc();
        let closing = node.closing_loc();

        self.process_regexp(
            opening.start_offset(),
            opening.end_offset(),
            content.start_offset(),
            content.end_offset(),
            closing.start_offset(),
            closing.end_offset(),
        );
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        let opening = node.opening_loc();
        let closing = node.closing_loc();

        let opening_text = &self.source[opening.start_offset()..opening.end_offset()];
        let delimiters = get_delimiters(opening_text);
        let extended = is_extended_mode(self.source, closing.start_offset(), closing.end_offset());

        // Scan the full source range between opening and closing delimiters.
        // The interpolation skip logic in find_redundant_escapes handles #{...} blocks.
        let content_start = opening.end_offset();
        let content_end = closing.start_offset();

        let redundant = find_redundant_escapes(
            self.source,
            content_start,
            content_end,
            &delimiters,
            extended,
        );

        for (offset, esc_len) in redundant {
            let location = Location::from_offsets(self.source, offset, offset + esc_len);
            let message = "Redundant escape inside regexp literal".to_string();
            let correction = Correction::delete(offset, offset + 1);
            self.offenses.push(
                Offense::new(
                    "Style/RedundantRegexpEscape",
                    message,
                    Severity::Convention,
                    location,
                    self.filename,
                )
                .with_correction(correction),
            );
        }

        // Recurse into embedded statements for nested regexps
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
