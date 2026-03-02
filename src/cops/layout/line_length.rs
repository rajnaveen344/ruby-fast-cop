//! Layout/LineLength - Checks the length of lines in the source code.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/line_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};
use regex::Regex;
use std::collections::VecDeque;

/// How AllowHeredoc is configured
#[derive(Debug, Clone)]
pub enum AllowHeredoc {
    /// AllowHeredoc: false (default)
    Disabled,
    /// AllowHeredoc: true — all heredocs permitted
    All,
    /// AllowHeredoc: ["SQL", "HEREDOC"] — only specific delimiters permitted
    Specific(Vec<String>),
}

pub struct LineLength {
    max: usize,
    allow_uri: bool,
    allow_heredoc: AllowHeredoc,
    allow_qualified_name: bool,
    allow_cop_directives: bool,
    allow_rbs_inline_annotation: bool,
    uri_schemes: Vec<String>,
    allowed_patterns: Vec<String>,
    /// Width to use for tab characters (default: IndentationWidth, typically 2)
    tab_width: usize,
}

impl LineLength {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            allow_uri: true,
            allow_heredoc: AllowHeredoc::Disabled,
            allow_qualified_name: false,
            allow_cop_directives: false,
            allow_rbs_inline_annotation: false,
            uri_schemes: vec!["http".to_string(), "https".to_string()],
            allowed_patterns: Vec::new(),
            tab_width: 2,
        }
    }

    pub fn with_config(
        max: usize,
        allow_uri: bool,
        allow_heredoc: AllowHeredoc,
        allow_qualified_name: bool,
        allow_cop_directives: bool,
        allow_rbs_inline_annotation: bool,
        uri_schemes: Vec<String>,
        allowed_patterns: Vec<String>,
        tab_width: usize,
    ) -> Self {
        Self {
            max,
            allow_uri,
            allow_heredoc,
            allow_qualified_name,
            allow_cop_directives,
            allow_rbs_inline_annotation,
            uri_schemes,
            allowed_patterns,
            tab_width,
        }
    }

    pub fn default_max() -> usize {
        120
    }

    // ── Line length computation (matches RuboCop) ──────────────────────

    /// Line length = raw character count + indentation_difference.
    /// Only leading tabs are expanded; mid-line tabs count as 1 char.
    fn line_length(&self, line: &str) -> usize {
        line.chars().count() + self.indentation_difference(line)
    }

    /// Extra visual width from leading tab characters.
    /// Each leading tab adds (tab_width - 1) extra visual positions.
    fn indentation_difference(&self, line: &str) -> usize {
        if self.tab_width <= 1 {
            return 0;
        }
        // If line doesn't start with a tab, no difference
        if !line.starts_with('\t') {
            return 0;
        }
        let n_leading_tabs = line.chars().take_while(|&c| c == '\t').count();
        n_leading_tabs * (self.tab_width - 1)
    }

    /// Character position where `max` falls, accounting for tab indentation.
    /// This is the default offense column_start.
    fn highlight_start(&self, line: &str) -> usize {
        let diff = self.indentation_difference(line);
        if self.max > diff {
            self.max - diff
        } else {
            0
        }
    }

    // ── Allowed-line checks ────────────────────────────────────────────

    fn is_shebang(&self, line: &str, line_index: usize) -> bool {
        line_index == 0 && line.starts_with("#!")
    }

    fn matches_allowed_pattern(&self, line: &str) -> bool {
        for pattern in &self.allowed_patterns {
            let pat = pattern.trim_matches('/');
            if let Ok(re) = Regex::new(pat) {
                if re.is_match(line) {
                    return true;
                }
            }
        }
        false
    }

    // ── Heredoc detection (text-based) ─────────────────────────────────

    /// Detect all heredoc body lines in the source.
    /// Returns Vec of (0-indexed line number, all enclosing heredoc delimiters).
    /// Tracks nesting so a line inside XXX nested inside SQL records both delimiters.
    fn find_heredoc_body_lines(source: &str) -> Vec<(usize, Vec<String>)> {
        let lines: Vec<&str> = source.lines().collect();
        let heredoc_re = Regex::new(r#"<<[-~]?['"]?(\w+)['"]?"#).unwrap();
        let mut result: Vec<(usize, Vec<String>)> = Vec::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        // Stack of heredocs whose bodies we're currently inside of
        let mut nesting: Vec<String> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if let Some(current_delim) = queue.front().cloned() {
                // If this heredoc isn't yet on the nesting stack, push it
                // (happens when transitioning from a sibling or entering a nested heredoc)
                if nesting.last().map_or(true, |d| *d != current_delim) {
                    nesting.push(current_delim.clone());
                }

                let trimmed = line.trim();
                if trimmed == current_delim {
                    // Closing delimiter line — pop from queue and nesting stack
                    queue.pop_front();
                    nesting.pop();
                } else {
                    // Body line — record with ALL enclosing heredoc delimiters
                    result.push((i, nesting.clone()));

                    // Check for nested heredoc openings (e.g. #{<<-OK})
                    let openings: Vec<String> = heredoc_re
                        .captures_iter(line)
                        .map(|c| c[1].to_string())
                        .collect();
                    // Push nested openings to front (they must complete before parent resumes)
                    for delim in openings.into_iter().rev() {
                        queue.push_front(delim);
                    }
                }
            } else {
                // Not inside any heredoc — clear nesting and check for openings
                nesting.clear();
                for cap in heredoc_re.captures_iter(line) {
                    queue.push_back(cap[1].to_string());
                }
            }
        }

        result
    }

    /// Check if a line is in a permitted heredoc body.
    fn is_in_permitted_heredoc(
        &self,
        line_index: usize,
        heredoc_lines: &[(usize, Vec<String>)],
    ) -> bool {
        match &self.allow_heredoc {
            AllowHeredoc::Disabled => false,
            AllowHeredoc::All => heredoc_lines.iter().any(|(idx, _)| *idx == line_index),
            AllowHeredoc::Specific(delimiters) => heredoc_lines.iter().any(|(idx, enclosing)| {
                *idx == line_index
                    && enclosing.iter().any(|d| delimiters.contains(d))
            }),
        }
    }

    // ── RBS inline annotation ──────────────────────────────────────────

    /// Check if line contains an RBS inline annotation (#:, #[...], #|).
    fn is_rbs_annotation(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.starts_with("#:") || trimmed.starts_with("#|") {
            return true;
        }
        // Check for trailing RBS annotation after code: ' #:' or ' #|'
        if let Some(pos) = line.rfind(" #:").or_else(|| line.rfind("\t#:")) {
            // Make sure it's not inside a string (heuristic: after some code)
            return pos > 0;
        }
        if let Some(pos) = line.rfind(" #|").or_else(|| line.rfind("\t#|")) {
            return pos > 0;
        }
        false
    }

    // ── Cop directive handling ─────────────────────────────────────────

    /// Regex pattern for rubocop directives: # rubocop:(disable|enable|todo)
    fn cop_directive_regex() -> Regex {
        Regex::new(r"#\s*rubocop\s*:\s*(?:disable|enable|todo)\b").unwrap()
    }

    /// Check if a line contains a cop directive comment.
    fn has_cop_directive(line: &str) -> bool {
        Self::cop_directive_regex().is_match(line)
    }

    /// Get the line length excluding the cop directive portion.
    /// Returns the visual length of the code before the directive.
    fn line_length_without_directive(&self, line: &str) -> usize {
        if let Some(m) = Self::cop_directive_regex().find(line) {
            let before = &line[..m.start()];
            let trimmed = before.trim_end();
            trimmed.len() + self.indentation_difference(trimmed)
        } else {
            self.line_length(line)
        }
    }

    // ── URI / Qualified Name matching (RuboCop approach) ───────────────

    /// Find the last URI match in the line. Returns raw char (start, end) positions.
    fn find_last_uri_match(&self, line: &str) -> Option<(usize, usize)> {
        if self.uri_schemes.is_empty() {
            return None;
        }

        let mut last_match: Option<(usize, usize)> = None;

        for scheme in &self.uri_schemes {
            let needle = format!("{}://", scheme);
            let mut search_from = 0;
            while let Some(byte_pos) = line[search_from..].find(&needle) {
                let abs_byte_start = search_from + byte_pos;

                // Find end of URI: stop at whitespace
                let uri_part = &line[abs_byte_start..];
                let uri_byte_end = uri_part
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(uri_part.len());
                let abs_byte_end = abs_byte_start + uri_byte_end;

                // Convert to char positions
                let char_start = line[..abs_byte_start].chars().count();
                let char_end = line[..abs_byte_end].chars().count();

                if last_match.map_or(true, |(prev_start, _)| char_start > prev_start) {
                    last_match = Some((char_start, char_end));
                }
                search_from = abs_byte_end;
            }
        }

        last_match
    }

    /// Find the last qualified name match. Returns raw char (start, end) positions.
    /// Pattern: \b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b
    fn find_last_qn_match(line: &str) -> Option<(usize, usize)> {
        let re =
            Regex::new(r"\b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b").unwrap();
        let mut last_match: Option<(usize, usize)> = None;

        for m in re.find_iter(line) {
            let char_start = line[..m.start()].chars().count();
            let char_end = line[..m.end()].chars().count();
            last_match = Some((char_start, char_end));
        }

        last_match
    }

    /// Extend match end position to include trailing non-whitespace characters.
    /// This handles URIs/QNs wrapped in quotes or parens: ("https://...") → includes ")
    /// Also handles YARD comments with linked URLs of the form {<uri> <title>}
    fn extend_end_position(line: &str, char_end: usize) -> usize {
        let chars: Vec<char> = line.chars().collect();
        let mut end = char_end;

        // Extend for YARD comments: {<uri> <title>} at end of line
        // If the line contains {...} ending at line end, extend through to closing }
        if Self::has_yard_braces(line) {
            // Find the closing } from end_position forward
            if let Some(brace_pos) = chars[end..].iter().rposition(|&c| c == '}') {
                end += brace_pos + 1;
            }
        }

        // Extend past trailing non-whitespace (handles closing quotes, parens, etc.)
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }

        end
    }

    /// Check if a line has YARD-style braces: {<something>} at end of line
    fn has_yard_braces(line: &str) -> bool {
        let trimmed = line.trim_end();
        if !trimmed.ends_with('}') {
            return false;
        }
        // Check there's a matching { somewhere in the line
        trimmed.contains('{')
    }

    /// Find the "excessive range" for a URI or QN match.
    /// Returns adjusted (begin, end) positions (with indentation_difference applied),
    /// or None if the match is entirely before max.
    fn find_excessive_uri_range(&self, line: &str) -> Option<(usize, usize)> {
        let (begin, end) = self.find_last_uri_match(line)?;
        let end = Self::extend_end_position(line, end);

        let indent_diff = self.indentation_difference(line);
        let adj_begin = begin + indent_diff;
        let adj_end = end + indent_diff;

        // If both positions are before max, the match doesn't overlap with excess
        if adj_begin < self.max && adj_end < self.max {
            return None;
        }

        Some((adj_begin, adj_end))
    }

    fn find_excessive_qn_range(&self, line: &str) -> Option<(usize, usize)> {
        let (begin, end) = Self::find_last_qn_match(line)?;
        let end = Self::extend_end_position(line, end);

        let indent_diff = self.indentation_difference(line);
        let adj_begin = begin + indent_diff;
        let adj_end = end + indent_diff;

        if adj_begin < self.max && adj_end < self.max {
            return None;
        }

        Some((adj_begin, adj_end))
    }

    /// Check if a range is in an "allowed position":
    /// starts before max AND extends to end of line.
    fn allowed_position(&self, range: (usize, usize), line: &str) -> bool {
        range.0 < self.max && range.1 == self.line_length(line)
    }

    /// Check if the combination of URI and QN ranges allows the line.
    fn allowed_combination(
        &self,
        line: &str,
        uri_range: &Option<(usize, usize)>,
        qn_range: &Option<(usize, usize)>,
    ) -> bool {
        match (uri_range, qn_range) {
            (Some(ur), Some(qr)) => {
                self.allowed_position(*ur, line) && self.allowed_position(*qr, line)
            }
            (Some(ur), None) => self.allowed_position(*ur, line),
            (None, Some(qr)) => self.allowed_position(*qr, line),
            (None, None) => false,
        }
    }

    /// Get the excessive position (column_start) given a URI/QN range.
    fn excess_position(&self, line: &str, range: &Option<(usize, usize)>) -> usize {
        if let Some((begin, end)) = range {
            if *begin < self.max {
                // Range straddles max: highlight starts after the range
                let indent_diff = self.indentation_difference(line);
                return end.saturating_sub(indent_diff);
            }
        }
        self.highlight_start(line)
    }
}

impl Default for LineLength {
    fn default() -> Self {
        Self::new(Self::default_max())
    }
}

impl Cop for LineLength {
    fn name(&self) -> &'static str {
        "Layout/LineLength"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let mut past_end = false;

        // Pre-compute heredoc body lines if AllowHeredoc is enabled
        let heredoc_lines = match &self.allow_heredoc {
            AllowHeredoc::Disabled => Vec::new(),
            _ => Self::find_heredoc_body_lines(ctx.source),
        };

        for (line_index, line) in ctx.source.lines().enumerate() {
            // Skip lines after __END__
            if line == "__END__" {
                past_end = true;
                continue;
            }
            if past_end {
                continue;
            }

            let visual_len = self.line_length(line);

            // Skip if within limit
            if visual_len <= self.max {
                continue;
            }

            // Skip shebang lines
            if self.is_shebang(line, line_index) {
                continue;
            }

            // Skip lines matching allowed patterns
            if self.matches_allowed_pattern(line) {
                continue;
            }

            // Skip lines in permitted heredoc bodies
            if self.is_in_permitted_heredoc(line_index, &heredoc_lines) {
                continue;
            }

            // Skip RBS inline annotations
            if self.allow_rbs_inline_annotation && self.is_rbs_annotation(line) {
                continue;
            }

            let char_len = line.chars().count();
            let line_num = (line_index + 1) as u32;

            // Handle cop directives
            if self.allow_cop_directives && Self::has_cop_directive(line) {
                let len_without = self.line_length_without_directive(line);
                if len_without <= self.max {
                    continue; // directive covers all excess
                }
                // Still too long even without directive — report adjusted length
                let col_start = self.highlight_start(line) as u32;
                // Column end = char position for len_without_directive
                let indent_diff = self.indentation_difference(line);
                let col_end = if len_without > indent_diff {
                    (len_without - indent_diff) as u32
                } else {
                    char_len as u32
                };
                offenses.push(Offense::new(
                    self.name(),
                    format!("Line is too long. [{}/{}]", len_without, self.max),
                    self.severity(),
                    Location::new(line_num, col_start, line_num, col_end),
                    ctx.filename,
                ));
                continue;
            }

            // Handle URI / qualified name exemptions
            if self.allow_uri || self.allow_qualified_name {
                let uri_range = if self.allow_uri {
                    self.find_excessive_uri_range(line)
                } else {
                    None
                };
                let qn_range = if self.allow_qualified_name {
                    self.find_excessive_qn_range(line)
                } else {
                    None
                };

                if uri_range.is_some() || qn_range.is_some() {
                    if self.allowed_combination(line, &uri_range, &qn_range) {
                        continue; // URI/QN covers all excess to end of line
                    }

                    // Still too long — report with adjusted column
                    let range = uri_range.or(qn_range);
                    let excessive_pos = self.excess_position(line, &range) as u32;

                    offenses.push(Offense::new(
                        self.name(),
                        format!("Line is too long. [{}/{}]", visual_len, self.max),
                        self.severity(),
                        Location::new(line_num, excessive_pos, line_num, char_len as u32),
                        ctx.filename,
                    ));
                    continue;
                }
            }

            // Default offense
            let col_start = self.highlight_start(line) as u32;
            offenses.push(Offense::new(
                self.name(),
                format!("Line is too long. [{}/{}]", visual_len, self.max),
                self.severity(),
                Location::new(line_num, col_start, line_num, char_len as u32),
                ctx.filename,
            ));
        }

        offenses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_max(source: &str, max: usize) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(LineLength::new(max));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_max(source, 80)
    }

    #[test]
    fn allows_short_lines() {
        let offenses = check("puts 'hello'");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_exactly_max_length() {
        let line = "x".repeat(80);
        let offenses = check(&line);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_line_over_max() {
        let line = "x".repeat(81);
        let offenses = check(&line);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].location.line, 1);
        assert_eq!(offenses[0].location.column, 80); // highlights from max
        assert_eq!(offenses[0].location.last_column, 81);
        assert!(offenses[0].message.contains("[81/80]"));
    }

    #[test]
    fn respects_custom_max() {
        let line = "x".repeat(100);

        let offenses = check_with_max(&line, 80);
        assert_eq!(offenses.len(), 1);

        let offenses = check_with_max(&line, 160);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_multiple_long_lines() {
        let source = format!("short\n{}\nokay\n{}\n", "a".repeat(100), "b".repeat(90));
        let offenses = check(&source);
        assert_eq!(offenses.len(), 2);
        assert_eq!(offenses[0].location.line, 2);
        assert_eq!(offenses[1].location.line, 4);
    }

    #[test]
    fn counts_unicode_correctly() {
        let emojis = "🎉".repeat(80);
        let offenses = check(&emojis);
        assert_eq!(offenses.len(), 0);

        let emojis = "🎉".repeat(81);
        let offenses = check(&emojis);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn ignores_shebang() {
        let source = format!("#!/usr/bin/env ruby {}\nputs 'ok'", "x".repeat(100));
        let offenses = check(&source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn ignores_lines_with_uri() {
        let source = format!(
            "# See: https://example.com/very/long/path/to/resource{}",
            "/x".repeat(50)
        );
        let offenses = check(&source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn message_format_matches_rubocop() {
        let line = "x".repeat(100);
        let offenses = check(&line);
        assert_eq!(offenses[0].message, "Line is too long. [100/80]");
    }

    #[test]
    fn skips_lines_after_end() {
        let source = format!("{}\n__END__\n{}", "x".repeat(81), "y".repeat(200));
        let offenses = check(&source);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].location.line, 1);
    }
}
