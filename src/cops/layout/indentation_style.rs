//! Layout/IndentationStyle
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/indentation_style.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Layout/IndentationStyle";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IndentationStyleMode {
    Spaces,
    Tabs,
}

pub struct IndentationStyle {
    mode: IndentationStyleMode,
    indentation_width: usize,
}

impl IndentationStyle {
    pub fn new(mode: IndentationStyleMode, indentation_width: usize) -> Self {
        Self { mode, indentation_width }
    }
}

impl Default for IndentationStyle {
    fn default() -> Self {
        Self::new(IndentationStyleMode::Spaces, 2)
    }
}

/// Collect byte ranges of string/heredoc literals (to skip tabs inside them)
struct StringRangeCollector {
    ranges: Vec<(usize, usize)>,
}

impl<'a> Visit<'a> for StringRangeCollector {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'a>) {
        self.ranges.push((node.location().start_offset(), node.location().end_offset()));
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'a>) {
        self.ranges.push((node.location().start_offset(), node.location().end_offset()));
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

fn collect_string_ranges(source: &str) -> Vec<(usize, usize)> {
    // Use a simpler approach: scan for heredocs and multi-line strings using line analysis
    // RuboCop skips lines that start inside a string literal or heredoc.
    // We track which lines are inside heredocs.
    let mut heredoc_lines: Vec<usize> = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Look for heredoc markers <<IDENT or <<~IDENT or <<-IDENT
        if let Some(pos) = find_heredoc_start(line) {
            let marker = &line[pos..];
            // Extract heredoc delimiter
            if let Some(delim) = extract_heredoc_delim(marker) {
                let start_line = i + 1; // body starts on next line
                let mut j = start_line;
                while j < lines.len() {
                    let body_line = lines[j].trim_end();
                    if body_line == delim || body_line.trim() == delim {
                        // end of heredoc
                        for k in start_line..=j {
                            heredoc_lines.push(k + 1); // 1-based
                        }
                        i = j;
                        break;
                    }
                    j += 1;
                }
            }
        }
        // Multi-line string/regex: also check single/double quoted spanning lines
        // Detect start of multi-line string (unmatched quote)
        if is_start_of_multiline_string(line) {
            // find closing quote
            let quote = if line.contains('"') { '"' } else { '\'' };
            let mut j = i + 1;
            while j < lines.len() {
                if lines[j].contains(quote) {
                    for k in (i + 1)..=j {
                        heredoc_lines.push(k + 1); // these lines start in string
                    }
                    i = j;
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }

    // Convert line numbers to byte ranges
    heredoc_lines
        .into_iter()
        .map(|line_no| {
            let start = line_byte_offset(source, line_no);
            let end = {
                let mut e = start;
                let bytes = source.as_bytes();
                while e < bytes.len() && bytes[e] != b'\n' {
                    e += 1;
                }
                e
            };
            (start, end)
        })
        .collect()
}

fn find_heredoc_start(line: &str) -> Option<usize> {
    // Find << that starts a heredoc (not inside string)
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut str_char = b'"';
    let mut i = 0;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' if !in_str => { in_str = true; str_char = bytes[i]; }
            c if in_str && c == str_char => { in_str = false; }
            b'<' if !in_str && bytes[i + 1] == b'<' => {
                // check it's a heredoc (not <<= or <<<)
                let next = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
                if next != b'<' && next != b'=' {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_heredoc_delim(s: &str) -> Option<String> {
    // s starts at <<
    let rest = s.get(2..)?;
    // skip - or ~
    let rest = rest.trim_start_matches(['-', '~']);
    // optional quotes
    let (quote, inner) = if rest.starts_with('"') || rest.starts_with('\'') || rest.starts_with('`') {
        let q = &rest[..1];
        let inner = rest[1..].split(q).next().unwrap_or("");
        (q, inner)
    } else {
        ("", rest.split_whitespace().next().unwrap_or(""))
    };
    let _ = quote;
    if inner.is_empty() {
        None
    } else {
        // strip non-alpha trailing
        let delim: String = inner.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if delim.is_empty() { None } else { Some(delim) }
    }
}

fn is_start_of_multiline_string(line: &str) -> bool {
    // Very rough heuristic: count unescaped quotes; if unmatched, line opens a multiline string
    // Skip for simplicity — RuboCop uses actual AST ranges; we'll use parse-based detection
    false
}

fn line_is_in_string_range(line_start: usize, line_end: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|&(rs, re)| rs <= line_start && line_end <= re)
}

impl Cop for IndentationStyle {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let lines: Vec<&str> = source.lines().collect();

        // Find __END__ boundary
        let end_marker_line = lines.iter().position(|&l| l == "__END__");

        // Collect string ranges using Prism AST
        let mut collector = StringRangeCollector { ranges: Vec::new() };
        collector.visit_program_node(node);
        let str_ranges = collector.ranges;

        // Also collect heredoc line ranges
        let heredoc_ranges = collect_string_ranges(source);

        let mut all_skip_ranges = str_ranges;
        all_skip_ranges.extend(heredoc_ranges);

        let mut offenses = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            let lineno = idx + 1; // 1-based
            if let Some(end_idx) = end_marker_line {
                if idx >= end_idx {
                    break;
                }
            }

            let line_start_off = line_byte_offset(source, lineno);

            let (offense_end, message, tab_source) = match self.mode {
                IndentationStyleMode::Spaces => {
                    // RuboCop pattern: /\A\s*\t+/
                    // Match: optional whitespace, then one or more tabs (stop at first non-tab)
                    let bytes = line.as_bytes();
                    let mut i = 0;
                    // Skip spaces/tabs until we hit a tab, then consume tabs
                    // `\s*` = any whitespace (spaces and tabs), `\t+` = trailing tabs
                    // Find the END of the tab sequence
                    let mut last_tab_end = 0usize;
                    let mut has_tab = false;
                    // Walk: match \s* then \t+
                    // Simple approach: find last position where we have tabs then non-tab follows
                    let mut end = 0;
                    while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
                        if bytes[end] == b'\t' { has_tab = true; }
                        end += 1;
                    }
                    if !has_tab { continue; }
                    // Trim trailing spaces after the last tab in the match
                    // /\A\s*\t+/ matches: spaces, then tabs (no trailing spaces)
                    // Find end: go backward until we find the last tab
                    let mut match_end = end;
                    // The match ends after the last tab in the leading whitespace
                    // Find last tab position in [0..end]
                    let last_tab_pos = line[..end].rfind('\t').unwrap_or(0);
                    match_end = last_tab_pos + 1;
                    // The match range is [0..match_end]
                    (match_end, "Tab detected in indentation.", true)
                }
                IndentationStyleMode::Tabs => {
                    // RuboCop pattern: /\A\s* +/
                    // Match: optional whitespace, then one or more spaces
                    let bytes = line.as_bytes();
                    let mut end = 0;
                    let mut has_space = false;
                    while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
                        if bytes[end] == b' ' { has_space = true; }
                        end += 1;
                    }
                    if !has_space { continue; }
                    // Trim trailing tabs after last space
                    let last_space_pos = line[..end].rfind(' ').unwrap_or(0);
                    let match_end = last_space_pos + 1;
                    (match_end, "Space detected in indentation.", false)
                }
            };

            if offense_end == 0 {
                continue;
            }

            let range_start = line_start_off;
            let range_end = line_start_off + offense_end;

            // Skip if inside string literal or heredoc
            if line_is_in_string_range(range_start, range_end, &all_skip_ranges) {
                continue;
            }

            let loc = Location::from_offsets(source, range_start, range_end);
            let indent_text = &line[..offense_end];

            // Correction
            let corrected = if tab_source {
                // tabs -> spaces: each tab becomes `width` spaces, spaces stay
                let spaces = " ".repeat(self.indentation_width);
                indent_text.replace('\t', &spaces)
            } else {
                // spaces -> tabs: RuboCop gsub(/\A\s+/) { "\t" * (match.size / width) }
                // The match is the entire leading whitespace [0..offense_end]
                // tab_count = match.size / width (integer div)
                // But spaces before tabs remain: we need to preserve non-space chars? No,
                // the whole range becomes tabs.
                // Actually: tabs are already 1 tab each, spaces are counted
                // RuboCop: match.size = number of chars in range, divided by width
                let tab_count = indent_text.len() / self.indentation_width;
                let remainder = indent_text.len() % self.indentation_width;
                format!("{}{}", " ".repeat(remainder), "\t".repeat(tab_count))
            };
            let correction = Correction::replace(range_start, range_end, corrected);

            offenses.push(
                Offense::new(COP_NAME, message, Severity::Convention, loc, ctx.filename)
                    .with_correction(correction),
            );
        }

        offenses
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
    indentation_width: Option<u32>,
}

crate::register_cop!("Layout/IndentationStyle", |cfg| {
    let c: Cfg = cfg.typed("Layout/IndentationStyle");
    let mode = if c.enforced_style == "tabs" {
        IndentationStyleMode::Tabs
    } else {
        IndentationStyleMode::Spaces
    };
    // IndentationWidth in cop config overrides Layout/IndentationWidth
    let global_width = cfg.get_cop_config("Layout/IndentationWidth")
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let width = c.indentation_width
        .map(|n| n as usize)
        .or(global_width)
        .unwrap_or(2);
    Some(Box::new(IndentationStyle::new(mode, width)))
});
