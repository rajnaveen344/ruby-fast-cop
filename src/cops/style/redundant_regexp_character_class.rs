//! Style/RedundantRegexpCharacterClass - Checks for unnecessary single-element
//! character classes in regexps like `[a]` -> `a`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_regexp_character_class.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::escape::is_interpolation_start;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

/// Characters that require escaping outside a character class but not inside.
const REQUIRES_ESCAPE_OUTSIDE: &[u8] = b".*+?{}()|$";

pub struct RedundantRegexpCharacterClass;

impl RedundantRegexpCharacterClass {
    pub fn new() -> Self {
        Self
    }
}

/// Represents a single-element character class found in a regexp.
struct CharClass {
    /// Byte offset of `[` in source
    bracket_start: usize,
    /// Byte offset after `]` in source
    bracket_end: usize,
    /// The content between `[` and `]`
    content: String,
}

/// Find all redundant single-element character classes in a regexp content region.
fn find_redundant_char_classes(
    source: &str,
    content_start: usize,
    content_end: usize,
    extended_mode: bool,
) -> Vec<CharClass> {
    let bytes = source.as_bytes();
    let mut results = Vec::new();
    let mut i = content_start;

    while i < content_end {
        let b = bytes[i];

        // Skip comments in extended mode
        if extended_mode && b == b'#' {
            while i < content_end && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Skip interpolations
        if is_interpolation_start(bytes, i, content_end) {
            i = skip_interpolation_block(bytes, i, content_end);
            continue;
        }

        // Skip escaped characters
        if b == b'\\' && i + 1 < content_end {
            i += 2;
            continue;
        }

        // Found a `[` - potential character class
        if b == b'[' {
            if let Some(cc) = try_parse_single_element_char_class(bytes, source, i, content_end, extended_mode) {
                results.push(cc);
                // Skip past the `]`
                i = results.last().unwrap().bracket_end;
                continue;
            }
            // Multi-element or nested, skip past the char class
            i = skip_char_class(bytes, i, content_end);
            continue;
        }

        i += 1;
    }

    results
}

/// Try to parse a single-element character class starting at `[`.
/// Returns None if it's negated, multi-element, contains POSIX classes,
/// intersection, or interpolation.
fn try_parse_single_element_char_class(
    bytes: &[u8],
    source: &str,
    open: usize,
    content_end: usize,
    extended_mode: bool,
) -> Option<CharClass> {
    let mut i = open + 1; // skip `[`
    if i >= content_end {
        return None;
    }

    // Negated character class `[^...]` - skip
    if bytes[i] == b'^' {
        return None;
    }

    // POSIX class `[[:...]]` - skip
    if bytes[i] == b'[' {
        return None;
    }

    // Intersection `[...&&...]` - handled below by checking content

    // Parse the single element
    let elem_start = i;
    let elem_end;

    if bytes[i] == b'\\' && i + 1 < content_end {
        // Escaped character - the element is the escape sequence
        let next = bytes[i + 1];
        if next == b'p' || next == b'P' {
            // Unicode property like \p{Digit} or \P{Digit}
            if i + 2 < content_end && bytes[i + 2] == b'{' {
                let mut j = i + 3;
                while j < content_end && bytes[j] != b'}' {
                    j += 1;
                }
                if j < content_end {
                    elem_end = j + 1; // include the `}`
                } else {
                    return None;
                }
            } else {
                elem_end = i + 2;
            }
        } else if next == b'u' && i + 2 < content_end && bytes[i + 2] == b'{' {
            // Unicode codepoint like \u{06F2}
            let mut j = i + 3;
            while j < content_end && bytes[j] != b'}' {
                // Check for multiple codepoints (space-separated)
                if bytes[j] == b' ' {
                    return None; // Multiple codepoints like \u{0061 0062}
                }
                j += 1;
            }
            if j < content_end {
                elem_end = j + 1;
            } else {
                return None;
            }
        } else if next == b'0' || (next >= b'1' && next <= b'7' && i + 2 < content_end && bytes[i + 2].is_ascii_digit()) {
            // Octal escape: \032 etc. - scan digits
            let mut j = i + 1;
            while j < content_end && bytes[j].is_ascii_digit() && j < i + 4 {
                j += 1;
            }
            elem_end = j;
        } else {
            elem_end = i + 2;
        }
    } else if is_interpolation_start(bytes, i, content_end) {
        // Interpolation inside char class - skip
        return None;
    } else {
        // Single unescaped character
        elem_end = i + 1;
    }

    i = elem_end;

    // The next char must be `]` for it to be a single-element class
    if i >= content_end || bytes[i] != b']' {
        return None;
    }

    let content = &source[elem_start..elem_end];

    // Check for intersection (&&)
    if content.contains("&&") {
        return None;
    }

    // Check if this is redundant
    if !is_redundant(content, extended_mode) {
        return None;
    }

    Some(CharClass {
        bracket_start: open,
        bracket_end: i + 1, // include `]`
        content: content.to_string(),
    })
}

/// Check if the single element can be safely used outside a character class.
fn is_redundant(content: &str, extended_mode: bool) -> bool {
    // \b behaves differently inside vs outside character class
    if content == "\\b" {
        return false;
    }

    // Octal escapes \1 to \7 are backreferences outside character class
    if content.len() == 2 && content.starts_with('\\') {
        let ch = content.as_bytes()[1];
        if ch >= b'1' && ch <= b'7' {
            return false;
        }
    }

    // Whitespace in free-space mode is significant inside char class
    if extended_mode {
        if content.chars().any(|c| c.is_ascii_whitespace()) {
            return false;
        }
    }

    // Unescaped chars that require escaping outside char class
    if content.len() == 1 {
        let b = content.as_bytes()[0];
        if REQUIRES_ESCAPE_OUTSIDE.contains(&b) {
            return false;
        }
    }

    true
}

/// Compute the replacement for a character class (strip `[` and `]`).
fn without_character_class(content: &str) -> String {
    // Special case: `[#]` -> `\#` to prevent interpolation
    if content == "#" {
        return "\\#".to_string();
    }
    content.to_string()
}

/// Skip past a character class (potentially nested), starting at `[`.
fn skip_char_class(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut i = start + 1;
    // Skip `^` in negated class
    if i < end && bytes[i] == b'^' {
        i += 1;
    }
    // Skip `]` if it's the first character (literal `]`)
    if i < end && bytes[i] == b']' {
        i += 1;
    }
    while i < end {
        match bytes[i] {
            b'\\' if i + 1 < end => i += 2,
            b'[' => {
                // Nested class or POSIX
                i = skip_char_class(bytes, i, end);
            }
            b']' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

/// Skip an interpolation block `#{...}`, handling nested braces.
fn skip_interpolation_block(bytes: &[u8], start: usize, end: usize) -> usize {
    // start is at `#`, next is `{`
    let mut i = start + 2; // skip `#{`
    let mut depth = 1;
    while i < end && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'\\' if i + 1 < end => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    i
}

impl Cop for RedundantRegexpCharacterClass {
    fn name(&self) -> &'static str {
        "Style/RedundantRegexpCharacterClass"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = RegexpVisitor {
            source: ctx.source,
            filename: ctx.filename,
            offenses: Vec::new(),
        };
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct RegexpVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    offenses: Vec<Offense>,
}

impl RegexpVisitor<'_> {
    fn process_regexp(
        &mut self,
        content_start: usize,
        content_end: usize,
        closing_start: usize,
        closing_end: usize,
    ) {
        let closing = &self.source[closing_start..closing_end];
        let extended = closing.contains('x');

        for cc in find_redundant_char_classes(self.source, content_start, content_end, extended) {
            let char_class_src = &self.source[cc.bracket_start..cc.bracket_end];
            let replacement = without_character_class(&cc.content);

            let message = format!(
                "Redundant single-element character class, `{}` can be replaced with `{}`.",
                char_class_src, replacement
            );

            let location =
                crate::offense::Location::from_offsets(self.source, cc.bracket_start, cc.bracket_end);
            let offense = Offense::new(
                "Style/RedundantRegexpCharacterClass",
                message,
                Severity::Convention,
                location,
                self.filename,
            )
            .with_correction(Correction::replace(
                cc.bracket_start,
                cc.bracket_end,
                &replacement,
            ));
            self.offenses.push(offense);
        }
    }
}

impl Visit<'_> for RegexpVisitor<'_> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let content = node.content_loc();
        let closing = node.closing_loc();
        self.process_regexp(
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

        // Process non-interpolated parts
        self.process_regexp(
            opening.end_offset(),
            closing.start_offset(),
            closing.start_offset(),
            closing.end_offset(),
        );

        // Visit embedded statements for nested regexps
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

crate::register_cop!("Style/RedundantRegexpCharacterClass", |_cfg| {
    Some(Box::new(RedundantRegexpCharacterClass::new()))
});
