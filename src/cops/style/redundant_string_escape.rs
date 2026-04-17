//! Style/RedundantStringEscape - Detects redundant backslash escapes inside string literals.

use crate::cops::{CheckContext, Cop};
use crate::helpers::escape::{closing_delimiter, is_interpolation_start, skip_interpolation};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

pub struct RedundantStringEscape;

impl RedundantStringEscape {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum StringContext {
    Interpolated,
    NonInterpolated,
    Skip,
}

fn classify_opening(opening: &str) -> (StringContext, char, char) {
    if opening.starts_with("<<") {
        let after = opening.trim_start_matches('<').trim_start_matches('~').trim_start_matches('-');
        return if after.starts_with('\'') {
            (StringContext::NonInterpolated, '\0', '\0')
        } else {
            (StringContext::Interpolated, '\0', '\0')
        };
    }

    match opening {
        ":\"" => return (StringContext::Interpolated, '"', '"'),
        ":'" => return (StringContext::NonInterpolated, '\'', '\''),
        "\"" => return (StringContext::Interpolated, '"', '"'),
        "'" => return (StringContext::NonInterpolated, '\'', '\''),
        _ => {}
    }

    // %I/%i -> Skip
    if opening.starts_with("%I") || opening.starts_with("%i") {
        return (StringContext::Skip, '\0', '\0');
    }

    // Percent literals: %Q, %q, %W, %w, or bare %
    let (ctx, skip) = if opening.starts_with("%Q") || opening.starts_with("%W") {
        (StringContext::Interpolated, 2)
    } else if opening.starts_with("%q") || opening.starts_with("%w") {
        (StringContext::NonInterpolated, 2)
    } else if opening.starts_with('%') && opening.len() >= 2 && !opening.as_bytes()[1].is_ascii_alphabetic() {
        (StringContext::Interpolated, 1)
    } else {
        return (StringContext::Skip, '\0', '\0');
    };

    if opening.len() > skip {
        let dc = opening.as_bytes()[skip] as char;
        let (o, c) = closing_delimiter(dc).map_or((dc, dc), |c| (dc, c));
        (ctx, o, c)
    } else {
        (ctx, '\0', '\0')
    }
}

fn is_allowed_escape(
    ch: char,
    open_delim: char,
    close_delim: char,
    is_word_array: bool,
    is_heredoc: bool,
) -> bool {
    match ch {
        '\\' | '\n' => true,
        'n' | 't' | 'r' | 'a' | 'b' | 'e' | 'f' | 's' | 'v' | '0' | 'x' | 'u' | 'c' | 'C' | 'M' => true,
        '1'..='7' => true,
        '$' | '@' => true,
        ' ' if is_word_array || is_heredoc => true,
        c if c.is_ascii_alphanumeric() => true,
        c if c == close_delim => true,
        c if open_delim != close_delim && c == open_delim => true,
        _ => false,
    }
}

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

        if is_interpolation_start(bytes, i, content_end) {
            i = skip_interpolation(bytes, i, content_end);
            continue;
        }

        // #\{ pattern: # followed by \{
        if b == b'#' && i + 1 < content_end && bytes[i + 1] == b'\\'
            && i + 2 < content_end && bytes[i + 2] == b'{'
        {
            i += 3; // skip #\{
            while i < content_end {
                if bytes[i] == b'\\' && i + 1 < content_end && bytes[i + 1] == b'}' {
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
            let next_next = if i + 2 < content_end { Some(bytes[i + 2] as char) } else { None };

            if ch == '#' {
                if let Some(nc) = next_next {
                    if nc == '{' || nc == '$' || nc == '@' {
                        i += 3;
                        continue;
                    }
                }
                if i + 2 < content_end && bytes[i + 2] == b'\\' && i + 3 < content_end && bytes[i + 3] == b'{' {
                    results.push((i + 2, '{'));
                    i += 4;
                    continue;
                }
                results.push((i, '#'));
                i += 2;
                continue;
            }

            if ch == '{' {
                if ch == open_delim as char || ch == close_delim as char {
                    i += 2;
                    continue;
                }
                results.push((i, '{'));
                i += 2;
                continue;
            }

            if ch == '}' {
                if close_delim == '}' {
                    i += 2;
                    continue;
                }
                results.push((i, '}'));
                i += 2;
                continue;
            }

            if is_allowed_escape(ch, open_delim, close_delim, is_word_array, is_heredoc) {
                i += 2;
                continue;
            }

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
    fn scan_content(
        &mut self,
        content_start: usize,
        content_end: usize,
        open_delim: char,
        close_delim: char,
        is_word_array: bool,
        is_heredoc: bool,
    ) {
        if content_start >= content_end { return; }
        for (offset, ch) in find_redundant_escapes(self.source, content_start, content_end, open_delim, close_delim, is_word_array, is_heredoc) {
            let location = Location::from_offsets(self.source, offset, offset + 2);
            self.offenses.push(
                Offense::new(
                    "Style/RedundantStringEscape",
                    format!("Redundant escape of {} inside string literal.", ch),
                    Severity::Convention,
                    location,
                    self.filename,
                )
                .with_correction(Correction::delete(offset, offset + 1)),
            );
        }
    }

    fn process_string_node(&mut self, node: &ruby_prism::StringNode) {
        let opening = match node.opening_loc() {
            Some(loc) => loc,
            None => return,
        };
        let open_text = &self.source[opening.start_offset()..opening.end_offset()];
        let (ctx, open_delim, close_delim) = classify_opening(open_text);
        if ctx != StringContext::Interpolated { return; }

        let content_loc = node.content_loc();
        self.scan_content(
            content_loc.start_offset(), content_loc.end_offset(),
            open_delim, close_delim, false, open_text.starts_with("<<"),
        );
    }

    fn process_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let opening = match node.opening_loc() {
            Some(loc) => loc,
            None => return,
        };
        let open_text = &self.source[opening.start_offset()..opening.end_offset()];
        let (ctx, open_delim, close_delim) = classify_opening(open_text);
        if ctx != StringContext::Interpolated { return; }

        let is_heredoc = open_text.starts_with("<<");
        self.scan_string_parts(&node.parts(), open_delim, close_delim, false, is_heredoc);
    }

    fn process_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        let opening = match node.opening_loc() {
            Some(loc) => loc,
            None => return,
        };
        let open_text = &self.source[opening.start_offset()..opening.end_offset()];
        let (ctx, open_delim, close_delim) = classify_opening(open_text);
        if ctx != StringContext::Interpolated { return; }

        self.scan_string_parts(&node.parts(), open_delim, close_delim, false, false);
    }

    fn scan_string_parts(
        &mut self,
        parts: &ruby_prism::NodeList,
        open_delim: char,
        close_delim: char,
        is_word_array: bool,
        is_heredoc: bool,
    ) {
        for part in parts.iter() {
            if let ruby_prism::Node::StringNode { .. } = part {
                let sn = part.as_string_node().unwrap();
                let loc = sn.location();
                self.scan_content(loc.start_offset(), loc.end_offset(), open_delim, close_delim, is_word_array, is_heredoc);
            }
        }
    }

    fn recurse_embedded(&mut self, parts: &ruby_prism::NodeList) {
        for part in parts.iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = part {
                ruby_prism::visit_embedded_statements_node(self, &part.as_embedded_statements_node().unwrap());
            }
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        self.process_string_node(node);
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if node.opening_loc().is_none() {
            for part in node.parts().iter() {
                match part {
                    ruby_prism::Node::StringNode { .. } => {
                        let sn = part.as_string_node().unwrap();
                        if sn.opening_loc().is_some() {
                            self.process_string_node(&sn);
                        }
                    }
                    ruby_prism::Node::InterpolatedStringNode { .. } => {
                        self.process_interpolated_string_node(&part.as_interpolated_string_node().unwrap());
                    }
                    _ => {}
                }
            }
            self.recurse_embedded(&node.parts());
            return;
        }
        self.process_interpolated_string_node(node);
        self.recurse_embedded(&node.parts());
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        self.process_interpolated_symbol_node(node);
        self.recurse_embedded(&node.parts());
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let loc = node.location();
        let text = &self.source[loc.start_offset()..loc.end_offset()];

        if text.starts_with("%W") {
            let dc = text.as_bytes()[2] as char;
            let (open_delim, close_delim) = closing_delimiter(dc).map_or((dc, dc), |c| (dc, c));

            for element in node.elements().iter() {
                match element {
                    ruby_prism::Node::StringNode { .. } => {
                        let sn = element.as_string_node().unwrap();
                        let sloc = sn.location();
                        self.scan_content(sloc.start_offset(), sloc.end_offset(), open_delim, close_delim, true, false);
                    }
                    ruby_prism::Node::InterpolatedStringNode { .. } => {
                        let isnode = element.as_interpolated_string_node().unwrap();
                        self.scan_string_parts(&isnode.parts(), open_delim, close_delim, true, false);
                    }
                    _ => {}
                }
            }
            // Recurse into interpolation blocks within %W elements
            for element in node.elements().iter() {
                if let ruby_prism::Node::InterpolatedStringNode { .. } = element {
                    self.recurse_embedded(&element.as_interpolated_string_node().unwrap().parts());
                }
            }
            return;
        }

        if text.starts_with("%w") { return; }
        ruby_prism::visit_array_node(self, node);
    }

    fn visit_x_string_node(&mut self, _node: &ruby_prism::XStringNode) {}
    fn visit_interpolated_x_string_node(&mut self, _node: &ruby_prism::InterpolatedXStringNode) {}
    fn visit_regular_expression_node(&mut self, _node: &ruby_prism::RegularExpressionNode) {}
    fn visit_interpolated_regular_expression_node(&mut self, _node: &ruby_prism::InterpolatedRegularExpressionNode) {}
}

crate::register_cop!("Style/RedundantStringEscape", |_cfg| {
    Some(Box::new(RedundantStringEscape::new()))
});
