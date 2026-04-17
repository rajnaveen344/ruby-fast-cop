//! Style/PercentLiteralDelimiters cop
//!
//! Enforces the consistent usage of %-literal delimiters.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

/// Default preferred delimiters (matches RuboCop defaults)
fn default_preferred_delimiters() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("default".to_string(), "()".to_string());
    m
}

pub struct PercentLiteralDelimiters {
    preferred_delimiters: HashMap<String, String>,
}

impl PercentLiteralDelimiters {
    pub fn new() -> Self {
        Self {
            preferred_delimiters: default_preferred_delimiters(),
        }
    }

    pub fn with_config(preferred_delimiters: HashMap<String, String>) -> Self {
        Self { preferred_delimiters }
    }

    /// Get the preferred delimiters for a given percent literal type (e.g., "%w", "%Q")
    fn preferred_delimiters_for(&self, literal_type: &str) -> Option<(char, char)> {
        let delim_str = self.preferred_delimiters
            .get(literal_type)
            .or_else(|| self.preferred_delimiters.get("default"))?;
        let chars: Vec<char> = delim_str.chars().collect();
        if chars.len() >= 2 {
            Some((chars[0], chars[1]))
        } else {
            None
        }
    }

    /// Get the matching pair for a delimiter
    fn matchpairs(begin_delim: char) -> Vec<char> {
        match begin_delim {
            '(' => vec!['(', ')'],
            '[' => vec!['[', ']'],
            '{' => vec!['{', '}'],
            '<' => vec!['<', '>'],
            _ => vec![begin_delim],
        }
    }

    /// Extract the percent literal type from source (e.g., "%w", "%Q", "%")
    fn percent_type(source: &str, start: usize, end: usize) -> Option<String> {
        let text = &source[start..end];
        if !text.starts_with('%') {
            return None;
        }
        // The type is everything up to (but not including) the opening delimiter
        // For %w[...], type is "%w"
        // For %(...), type is "%"
        // For %Q{...}, type is "%Q"
        let bytes = text.as_bytes();
        let mut i = 1; // skip '%'
        while i < bytes.len() {
            let ch = bytes[i] as char;
            if ch.is_ascii_alphabetic() {
                i += 1;
            } else {
                break;
            }
        }
        Some(text[..i].to_string())
    }

    /// Extract the opening delimiter char from source
    fn opening_delimiter(source: &str, start: usize, end: usize) -> Option<char> {
        let text = &source[start..end];
        if !text.starts_with('%') {
            return None;
        }
        let bytes = text.as_bytes();
        let mut i = 1;
        while i < bytes.len() {
            let ch = bytes[i] as char;
            if ch.is_ascii_alphabetic() {
                i += 1;
            } else {
                return Some(ch);
            }
        }
        None
    }

    /// Check if the node's content contains any of the given delimiter characters.
    /// Only checks literal string/symbol children, not interpolation.
    fn content_contains_delimiter(&self, source: &str, node_start: usize, node_end: usize, delimiters: &[char]) -> bool {
        let text = &source[node_start..node_end];
        // Skip the opening (e.g., "%w[") and closing delimiter
        let bytes = text.as_bytes();
        let mut content_start = 1; // skip '%'
        while content_start < bytes.len() && (bytes[content_start] as char).is_ascii_alphabetic() {
            content_start += 1;
        }
        if content_start < bytes.len() {
            content_start += 1; // skip opening delimiter
        }
        let content_end = if bytes.len() > 0 { bytes.len() - 1 } else { 0 }; // skip closing delimiter

        if content_start >= content_end {
            return false;
        }

        let content = &text[content_start..content_end];
        // For interpolated content, only check non-interpolated parts
        let mut in_interpolation = false;
        let mut brace_depth = 0;
        let content_bytes = content.as_bytes();
        let mut i = 0;
        while i < content_bytes.len() {
            if !in_interpolation && i + 1 < content_bytes.len() && content_bytes[i] == b'#' && content_bytes[i + 1] == b'{' {
                in_interpolation = true;
                brace_depth = 1;
                i += 2;
                continue;
            }
            if in_interpolation {
                if content_bytes[i] == b'{' {
                    brace_depth += 1;
                } else if content_bytes[i] == b'}' {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        in_interpolation = false;
                    }
                }
                i += 1;
                continue;
            }
            let ch = content_bytes[i] as char;
            if delimiters.contains(&ch) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// For %w and %i, check if content contains the same character as the used delimiter
    fn content_contains_used_delimiter(&self, source: &str, node_start: usize, node_end: usize, literal_type: &str, used_open: char) -> bool {
        if literal_type != "%w" && literal_type != "%i" {
            return false;
        }
        let used_delims = Self::matchpairs(used_open);
        let used_chars: Vec<char> = used_delims;
        self.content_contains_delimiter(source, node_start, node_end, &used_chars)
    }

    fn check_percent_literal(
        &self,
        ctx: &CheckContext,
        node_start: usize,
        node_end: usize,
        offenses: &mut Vec<Offense>,
    ) {
        let source = ctx.source;
        let literal_type = match Self::percent_type(source, node_start, node_end) {
            Some(t) => t,
            None => return,
        };

        let used_open = match Self::opening_delimiter(source, node_start, node_end) {
            Some(d) => d,
            None => return,
        };

        let (pref_open, pref_close) = match self.preferred_delimiters_for(&literal_type) {
            Some(d) => d,
            None => return,
        };

        // Check if already using preferred delimiters
        if used_open == pref_open {
            return;
        }

        // Check if content contains preferred delimiter characters
        let pref_delims = vec![pref_open, pref_close];
        if self.content_contains_delimiter(source, node_start, node_end, &pref_delims) {
            return;
        }

        // For %w and %i, check if content contains the same character as the used delimiter
        if self.content_contains_used_delimiter(source, node_start, node_end, &literal_type, used_open) {
            return;
        }

        let msg = format!(
            "`{}`-literals should be delimited by `{}` and `{}`.",
            literal_type, pref_open, pref_close
        );

        // For multiline literals, report offense on just the opening part (e.g., `%w(`)
        let offense_end = if source[node_start..node_end].contains('\n') {
            // End at the opening delimiter (type + delimiter char)
            node_start + literal_type.len() + 1
        } else {
            node_end
        };
        let mut offense = ctx.offense_with_range(
            "Style/PercentLiteralDelimiters",
            &msg,
            Severity::Convention,
            node_start,
            offense_end,
        );

        // Build correction: replace opening and closing delimiters
        let type_len = literal_type.len();
        let open_start = node_start + type_len;
        let open_end = open_start + 1;

        // Find the closing delimiter - it's the last character of the node
        // But for regexps with options like %r(.*)i, the closing delim is not the last char
        let text = &source[node_start..node_end];
        let close_start = self.find_closing_delimiter_offset(text, node_start);
        let close_end = close_start + 1;

        let correction = Correction {
            edits: vec![
                crate::offense::Edit {
                    start_offset: open_start,
                    end_offset: open_end,
                    replacement: pref_open.to_string(),
                },
                crate::offense::Edit {
                    start_offset: close_start,
                    end_offset: close_end,
                    replacement: pref_close.to_string(),
                },
            ],
        };
        offense = offense.with_correction(correction);

        offenses.push(offense);
    }

    /// Find the byte offset of the closing delimiter within the node
    fn find_closing_delimiter_offset(&self, text: &str, base_offset: usize) -> usize {
        let bytes = text.as_bytes();
        // Skip the type prefix to find the opening delimiter
        let mut i = 1; // skip '%'
        while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() {
            i += 1;
        }
        if i >= bytes.len() {
            return base_offset + bytes.len() - 1;
        }
        let open_delim = bytes[i] as char;
        let close_delim = match open_delim {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '<' => '>',
            _ => open_delim,
        };

        // Find the matching closing delimiter, handling nesting for paired delimiters
        let is_paired = matches!(open_delim, '(' | '[' | '{' | '<');
        let mut depth = 1;
        let mut j = i + 1;
        while j < bytes.len() {
            let ch = bytes[j] as char;
            // Skip escaped characters
            if j > 0 && bytes[j - 1] == b'\\' {
                j += 1;
                continue;
            }
            // Skip interpolation
            if j + 1 < bytes.len() && bytes[j] == b'#' && bytes[j + 1] == b'{' {
                let mut brace_depth = 1;
                j += 2;
                while j < bytes.len() && brace_depth > 0 {
                    if bytes[j] == b'{' { brace_depth += 1; }
                    else if bytes[j] == b'}' { brace_depth -= 1; }
                    j += 1;
                }
                continue;
            }
            if is_paired {
                if ch == open_delim { depth += 1; }
                if ch == close_delim { depth -= 1; }
                if depth == 0 {
                    return base_offset + j;
                }
            } else if ch == close_delim {
                return base_offset + j;
            }
            j += 1;
        }
        base_offset + bytes.len() - 1
    }
}

impl Default for PercentLiteralDelimiters {
    fn default() -> Self {
        Self::new()
    }
}

struct PercentLiteralVisitor<'a> {
    cop: &'a PercentLiteralDelimiters,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> PercentLiteralVisitor<'a> {
    fn is_percent_literal(&self, start: usize, end: usize) -> bool {
        if start >= end || start >= self.ctx.source.len() {
            return false;
        }
        self.ctx.source.as_bytes()[start] == b'%'
    }

    fn check_node(&mut self, node_start: usize, node_end: usize, valid_types: &[&str]) {
        if !self.is_percent_literal(node_start, node_end) {
            return;
        }
        if let Some(literal_type) = PercentLiteralDelimiters::percent_type(self.ctx.source, node_start, node_end) {
            if valid_types.contains(&literal_type.as_str()) {
                self.cop.check_percent_literal(self.ctx, node_start, node_end, &mut self.offenses);
            }
        }
    }
}

impl Visit<'_> for PercentLiteralVisitor<'_> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%w", "%W", "%i", "%I"]);
        ruby_prism::visit_array_node(self, node);
    }

    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%r"]);
        // Don't recurse further
    }

    fn visit_interpolated_regular_expression_node(&mut self, node: &ruby_prism::InterpolatedRegularExpressionNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%r"]);
        // Don't recurse into children to avoid checking embedded strings
    }

    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%", "%Q", "%q"]);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%", "%Q", "%q"]);
        // Don't recurse to avoid double-checking embedded nodes
    }

    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%s"]);
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%x"]);
    }

    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_node(start, end, &["%x"]);
    }
}

impl Cop for PercentLiteralDelimiters {
    fn name(&self) -> &'static str {
        "Style/PercentLiteralDelimiters"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = PercentLiteralVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Style/PercentLiteralDelimiters", |cfg| {
    let cop_config = cfg.get_cop_config("Style/PercentLiteralDelimiters");
    let preferred = cop_config
        .and_then(|c| c.raw.get("PreferredDelimiters"))
        .and_then(|v| v.as_mapping())
        .map(|m| {
            let mut map = std::collections::HashMap::new();
            for (k, v) in m.iter() {
                if let (Some(key), Some(val)) = (k.as_str(), v.as_str()) {
                    map.insert(key.to_string(), val.to_string());
                }
            }
            map
        })
        .unwrap_or_else(|| {
            let mut m = std::collections::HashMap::new();
            m.insert("default".to_string(), "()".to_string());
            m
        });
    Some(Box::new(PercentLiteralDelimiters::with_config(preferred)))
});
