//! Style/RedundantStringEscape - Detects redundant backslash escapes inside string literals.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_string_escape.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

pub struct RedundantStringEscape;

impl RedundantStringEscape {
    pub fn new() -> Self {
        Self
    }
}

/// The type of string literal context for escape analysis
#[derive(Debug, Clone, Copy, PartialEq)]
enum StringContext {
    /// Interpolation-enabled (double-quoted, %Q, %W, %(, heredoc without quotes)
    Interpolated,
    /// No interpolation (single-quoted, %q, %w, heredoc with quotes)
    NonInterpolated,
    /// Skip entirely (regexp, xstr, character literal, __FILE__)
    Skip,
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

/// Determine the string context and delimiters from the opening text of a string literal
fn classify_opening(opening: &str) -> (StringContext, char, char) {
    // Heredoc
    if opening.starts_with("<<") {
        let after = opening.trim_start_matches('<').trim_start_matches('~').trim_start_matches('-');
        if after.starts_with('\'') || after.starts_with('"') {
            // <<~'HEREDOC' or <<~"HEREDOC" - both treated differently by Prism
            // Single-quoted heredocs have no interpolation
            if after.starts_with('\'') {
                return (StringContext::NonInterpolated, '\0', '\0');
            }
            return (StringContext::Interpolated, '\0', '\0');
        }
        // Bare heredoc: <<~HEREDOC
        return (StringContext::Interpolated, '\0', '\0');
    }

    // Symbol literal: :"..."
    if opening == ":\"" {
        return (StringContext::Interpolated, '"', '"');
    }
    if opening == ":'" {
        return (StringContext::NonInterpolated, '\'', '\'');
    }

    // %Q(...) or %q(...)
    if opening.starts_with("%Q") || opening.starts_with("%q") {
        let is_interp = opening.starts_with("%Q");
        if opening.len() >= 3 {
            let delim_char = opening.as_bytes()[2] as char;
            let (open, close) = if let Some(c) = closing_delimiter(delim_char) {
                (delim_char, c)
            } else {
                (delim_char, delim_char)
            };
            let ctx = if is_interp { StringContext::Interpolated } else { StringContext::NonInterpolated };
            return (ctx, open, close);
        }
    }

    // %W[...] or %w[...]
    if opening.starts_with("%W") || opening.starts_with("%w") {
        let is_interp = opening.starts_with("%W");
        if opening.len() >= 3 {
            let delim_char = opening.as_bytes()[2] as char;
            let (open, close) = if let Some(c) = closing_delimiter(delim_char) {
                (delim_char, c)
            } else {
                (delim_char, delim_char)
            };
            let ctx = if is_interp { StringContext::Interpolated } else { StringContext::NonInterpolated };
            return (ctx, open, close);
        }
    }

    // %I[...] or %i[...]
    if opening.starts_with("%I") || opening.starts_with("%i") {
        return (StringContext::Skip, '\0', '\0');
    }

    // %(...) (bare percent)
    if opening.starts_with("%(") || (opening.starts_with('%') && opening.len() >= 2 && !opening.as_bytes()[1].is_ascii_alphabetic()) {
        let delim_char = opening.as_bytes()[1] as char;
        let (open, close) = if let Some(c) = closing_delimiter(delim_char) {
            (delim_char, c)
        } else {
            (delim_char, delim_char)
        };
        return (StringContext::Interpolated, open, close);
    }

    // Double-quoted string
    if opening == "\"" {
        return (StringContext::Interpolated, '"', '"');
    }

    // Single-quoted string
    if opening == "'" {
        return (StringContext::NonInterpolated, '\'', '\'');
    }

    (StringContext::Skip, '\0', '\0')
}

/// Returns true if a backslash escape should be skipped (not flagged as redundant).
fn is_allowed_escape(
    ch: char,
    _next_char: Option<char>,
    open_delim: char,
    close_delim: char,
    is_word_array: bool,
    is_heredoc: bool,
) -> bool {
    // Escaped backslash is always meaningful
    if ch == '\\' {
        return true;
    }

    // Line continuation (backslash before newline)
    if ch == '\n' {
        return true;
    }

    // Special escape sequences
    match ch {
        'n' | 't' | 'r' | 'a' | 'b' | 'e' | 'f' | 's' | 'v' => return true,
        '0' => return true,
        'x' => return true,
        'u' => return true,
        'c' | 'C' => return true,
        'M' => return true,
        _ => {}
    }

    // Octal digits
    if ch >= '1' && ch <= '7' {
        return true;
    }

    // Alphanumeric - could have meaning in different Ruby versions
    if ch.is_ascii_alphanumeric() {
        return true;
    }

    // Escaped space in %w/%W arrays and heredocs
    if ch == ' ' && (is_word_array || is_heredoc) {
        return true;
    }

    // Escaped delimiter is always necessary
    if ch == close_delim {
        return true;
    }
    // For bracket-style delimiters, also allow escaping the open delimiter
    if open_delim != close_delim && ch == open_delim {
        return true;
    }

    // \$ and \@ - always allow (can disable interpolation when preceded by #)
    if ch == '$' || ch == '@' {
        return true;
    }

    false
}

/// Scan a content region for redundant escapes.
/// `content_start` and `content_end` are byte offsets into `source` for the content area.
/// Returns a list of (byte_offset_of_backslash, escaped_char) for each redundant escape.
fn find_redundant_escapes(
    source: &str,
    content_start: usize,
    content_end: usize,
    open_delim: char,
    close_delim: char,
    is_word_array: bool,
    is_heredoc: bool,
) -> Vec<(usize, char)> {
    let bytes = source.as_bytes();
    let mut results = Vec::new();
    let mut i = content_start;

    while i < content_end {
        let b = bytes[i];

        // Skip interpolation blocks: #{...}
        if b == b'#' && i + 1 < content_end && bytes[i + 1] == b'{' {
            // Find the matching closing brace, accounting for nesting
            let mut depth = 1;
            let mut j = i + 2;
            while j < content_end && depth > 0 {
                if bytes[j] == b'{' {
                    depth += 1;
                } else if bytes[j] == b'}' {
                    depth -= 1;
                } else if bytes[j] == b'\\' && j + 1 < content_end {
                    j += 1; // skip escaped char inside interpolation
                } else if bytes[j] == b'"' {
                    // Skip string inside interpolation
                    j += 1;
                    while j < content_end && bytes[j] != b'"' {
                        if bytes[j] == b'\\' && j + 1 < content_end {
                            j += 1;
                        }
                        j += 1;
                    }
                } else if bytes[j] == b'\'' {
                    // Skip single-quoted string inside interpolation
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

        // Handle #\{ pattern: # followed by \{
        // This prevents interpolation, so the \{ is NOT redundant
        // But if there's a matching \} later, the \} IS redundant
        if b == b'#' && i + 1 < content_end && bytes[i + 1] == b'\\'
            && i + 2 < content_end && bytes[i + 2] == b'{'
        {
            // #\{ pattern - skip #, then handle the \{
            i += 1; // skip #
            // The \{ at i is NOT redundant
            i += 2; // skip \{
            // Scan forward for \} which would be redundant
            while i < content_end {
                if bytes[i] == b'\\' && i + 1 < content_end && bytes[i + 1] == b'}' {
                    // \} is redundant if } is not the closing delimiter
                    if close_delim != '}' {
                        results.push((i, '}'));
                    }
                    i += 2;
                    break;
                } else if bytes[i] == b'}' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            continue;
        }

        if b == b'\\' && i + 1 < content_end {
            let ch = bytes[i + 1] as char;
            let next_next = if i + 2 < content_end {
                Some(bytes[i + 2] as char)
            } else {
                None
            };

            // \# handling
            if ch == '#' {
                // Check what follows \#
                if let Some(nc) = next_next {
                    if nc == '{' || nc == '$' || nc == '@' {
                        // \#{ or \#$ or \#@ - prevents interpolation, NOT redundant
                        i += 3; // skip \# and the next char
                        continue;
                    }
                }
                // Check if \# followed by \{ (i.e., \#\{)
                if i + 2 < content_end && bytes[i + 2] == b'\\'
                    && i + 3 < content_end && bytes[i + 3] == b'{'
                {
                    // \#\{ pattern: \# is NOT redundant, \{ IS redundant
                    results.push((i + 2, '{')); // Flag the \{
                    i += 4; // skip \#\{
                    continue;
                }
                // \# not followed by {, $, @, \{ -> REDUNDANT
                results.push((i, '#'));
                i += 2;
                continue;
            }

            // \{ handling
            if ch == '{' {
                // \{ NOT preceded by # - check if it's a delimiter
                if ch == open_delim as char || ch == close_delim as char {
                    i += 2;
                    continue;
                }
                // Standalone \{ is redundant (it's not preventing interpolation without #)
                results.push((i, '{'));
                i += 2;
                continue;
            }

            // \} handling
            if ch == '}' {
                if close_delim == '}' {
                    // } is the delimiter, not redundant
                    i += 2;
                    continue;
                }
                // Standalone \} is redundant
                results.push((i, '}'));
                i += 2;
                continue;
            }

            if is_allowed_escape(ch, next_next, open_delim, close_delim, is_word_array, is_heredoc) {
                i += 2;
                continue;
            }

            // This escape is redundant
            results.push((i, ch));
            i += 2;
            continue;
        }

        i += 1;
    }

    results
}

impl Cop for RedundantStringEscape {
    fn name(&self) -> &'static str {
        "Style/RedundantStringEscape"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        _node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = RedundantStringEscapeVisitor {
            source: ctx.source,
            filename: ctx.filename,
            offenses: Vec::new(),
        };
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct RedundantStringEscapeVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    offenses: Vec<Offense>,
}

impl RedundantStringEscapeVisitor<'_> {
    /// Scan a content region for redundant escapes and generate offenses.
    fn scan_content(
        &mut self,
        content_start: usize,
        content_end: usize,
        open_delim: char,
        close_delim: char,
        is_word_array: bool,
        is_heredoc: bool,
    ) {
        if content_start >= content_end {
            return;
        }

        let redundant = find_redundant_escapes(
            self.source,
            content_start,
            content_end,
            open_delim,
            close_delim,
            is_word_array,
            is_heredoc,
        );

        for (offset, ch) in redundant {
            let location = Location::from_offsets(self.source, offset, offset + 2);
            let message = format!("Redundant escape of {} inside string literal.", ch);
            let correction = Correction::delete(offset, offset + 1);
            self.offenses.push(
                Offense::new(
                    "Style/RedundantStringEscape",
                    message,
                    Severity::Convention,
                    location,
                    self.filename,
                )
                .with_correction(correction),
            );
        }
    }

    /// Process a StringNode that has an opening delimiter
    fn process_string_node(&mut self, node: &ruby_prism::StringNode) {
        let opening = match node.opening_loc() {
            Some(loc) => loc,
            None => return, // No opening = part of interpolated string, skip
        };

        let open_text = &self.source[opening.start_offset()..opening.end_offset()];
        let (ctx, open_delim, close_delim) = classify_opening(open_text);

        match ctx {
            StringContext::Skip | StringContext::NonInterpolated => return,
            StringContext::Interpolated => {}
        }

        let is_heredoc = open_text.starts_with("<<");
        let is_word_array = false; // StringNode is never a word array

        // For heredocs, use content_loc directly
        // For regular strings, use content_loc too
        let content_loc = node.content_loc();
        let content_start = content_loc.start_offset();
        let content_end = content_loc.end_offset();

        self.scan_content(content_start, content_end, open_delim, close_delim, is_word_array, is_heredoc);
    }

    /// Process an InterpolatedStringNode
    fn process_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let opening = match node.opening_loc() {
            Some(loc) => loc,
            None => return,
        };

        let open_text = &self.source[opening.start_offset()..opening.end_offset()];
        let (ctx, open_delim, close_delim) = classify_opening(open_text);

        match ctx {
            StringContext::Skip | StringContext::NonInterpolated => return,
            StringContext::Interpolated => {}
        }

        let is_heredoc = open_text.starts_with("<<");
        let is_word_array = false;

        // For interpolated strings, scan each string part (not embedded statements)
        for part in node.parts().iter() {
            match part {
                ruby_prism::Node::StringNode { .. } => {
                    let sn = part.as_string_node().unwrap();
                    // These are bare string parts inside interpolated strings - no opening_loc
                    let loc = sn.location();
                    self.scan_content(
                        loc.start_offset(),
                        loc.end_offset(),
                        open_delim,
                        close_delim,
                        is_word_array,
                        is_heredoc,
                    );
                }
                _ => {} // Skip embedded statements (interpolation blocks)
            }
        }
    }

    /// Process an InterpolatedSymbolNode
    fn process_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        let opening = match node.opening_loc() {
            Some(loc) => loc,
            None => return,
        };

        let open_text = &self.source[opening.start_offset()..opening.end_offset()];
        let (ctx, open_delim, close_delim) = classify_opening(open_text);

        match ctx {
            StringContext::Skip | StringContext::NonInterpolated => return,
            StringContext::Interpolated => {}
        }

        for part in node.parts().iter() {
            match part {
                ruby_prism::Node::StringNode { .. } => {
                    let sn = part.as_string_node().unwrap();
                    let loc = sn.location();
                    self.scan_content(
                        loc.start_offset(),
                        loc.end_offset(),
                        open_delim,
                        close_delim,
                        false,
                        false,
                    );
                }
                _ => {}
            }
        }
    }

    /// Process a %W array node
    fn process_percent_w_array(&mut self, node: &ruby_prism::ArrayNode) {
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        let text = &self.source[start..end];

        if !text.starts_with("%W") {
            return;
        }

        let delim_char = text.as_bytes()[2] as char;
        let (open_delim, close_delim) = if let Some(c) = closing_delimiter(delim_char) {
            (delim_char, c)
        } else {
            (delim_char, delim_char)
        };

        // For %W arrays, we scan each element
        for element in node.elements().iter() {
            match element {
                ruby_prism::Node::StringNode { .. } => {
                    let sn = element.as_string_node().unwrap();
                    let sloc = sn.location();
                    self.scan_content(
                        sloc.start_offset(),
                        sloc.end_offset(),
                        open_delim,
                        close_delim,
                        true,
                        false,
                    );
                }
                ruby_prism::Node::InterpolatedStringNode { .. } => {
                    let isnode = element.as_interpolated_string_node().unwrap();
                    for part in isnode.parts().iter() {
                        match part {
                            ruby_prism::Node::StringNode { .. } => {
                                let sn = part.as_string_node().unwrap();
                                let ploc = sn.location();
                                self.scan_content(
                                    ploc.start_offset(),
                                    ploc.end_offset(),
                                    open_delim,
                                    close_delim,
                                    true,
                                    false,
                                );
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl Visit<'_> for RedundantStringEscapeVisitor<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        self.process_string_node(node);
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if node.opening_loc().is_none() {
            // No opening delimiter on the InterpolatedStringNode itself.
            // This is a continuation string (e.g., "a"\ "b") where Prism wraps
            // individual StringNode parts in an InterpolatedStringNode.
            // Process each StringNode part individually.
            for part in node.parts().iter() {
                match part {
                    ruby_prism::Node::StringNode { .. } => {
                        let sn = part.as_string_node().unwrap();
                        if sn.opening_loc().is_some() {
                            self.process_string_node(&sn);
                        }
                    }
                    ruby_prism::Node::InterpolatedStringNode { .. } => {
                        let isn = part.as_interpolated_string_node().unwrap();
                        self.process_interpolated_string_node(&isn);
                    }
                    _ => {}
                }
            }
            // Recurse into embedded statements for nested strings
            for part in node.parts().iter() {
                if let ruby_prism::Node::EmbeddedStatementsNode { .. } = part {
                    ruby_prism::visit_embedded_statements_node(self, &part.as_embedded_statements_node().unwrap());
                }
            }
            return;
        }
        self.process_interpolated_string_node(node);
        // Recurse into embedded statements for nested strings
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = part {
                ruby_prism::visit_embedded_statements_node(self, &part.as_embedded_statements_node().unwrap());
            }
        }
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        self.process_interpolated_symbol_node(node);
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = part {
                ruby_prism::visit_embedded_statements_node(self, &part.as_embedded_statements_node().unwrap());
            }
        }
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        let text = &self.source[start..end];

        if text.starts_with("%W") {
            self.process_percent_w_array(node);
            // Recurse into interpolation blocks within %W elements
            for element in node.elements().iter() {
                if let ruby_prism::Node::InterpolatedStringNode { .. } = element {
                    let isnode = element.as_interpolated_string_node().unwrap();
                    for part in isnode.parts().iter() {
                        if let ruby_prism::Node::EmbeddedStatementsNode { .. } = part {
                            ruby_prism::visit_embedded_statements_node(
                                self,
                                &part.as_embedded_statements_node().unwrap(),
                            );
                        }
                    }
                }
            }
            return;
        }

        // Skip %w arrays entirely (no interpolation)
        if text.starts_with("%w") {
            return;
        }

        // Normal arrays - recurse normally
        ruby_prism::visit_array_node(self, node);
    }

    fn visit_x_string_node(&mut self, _node: &ruby_prism::XStringNode) {
        // Skip xstr entirely
    }

    fn visit_interpolated_x_string_node(&mut self, _node: &ruby_prism::InterpolatedXStringNode) {
        // Skip xstr entirely
    }

    fn visit_regular_expression_node(&mut self, _node: &ruby_prism::RegularExpressionNode) {
        // Skip regex entirely
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        _node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        // Skip regex entirely
    }
}
