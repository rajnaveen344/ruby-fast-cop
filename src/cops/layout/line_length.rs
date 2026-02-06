//! Layout/LineLength - Checks the length of lines in the source code.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/line_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};
use regex::Regex;

pub struct LineLength {
    max: usize,
    allow_uri: bool,
    allow_heredoc: bool,
    allow_qualified_name: bool,
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
            allow_heredoc: false,
            allow_qualified_name: false,
            uri_schemes: vec!["http".to_string(), "https".to_string()],
            allowed_patterns: Vec::new(),
            tab_width: 2,
        }
    }

    pub fn with_config(
        max: usize,
        allow_uri: bool,
        allow_heredoc: bool,
        allow_qualified_name: bool,
        uri_schemes: Vec<String>,
        allowed_patterns: Vec<String>,
        tab_width: usize,
    ) -> Self {
        Self {
            max,
            allow_uri,
            allow_heredoc,
            allow_qualified_name,
            uri_schemes,
            allowed_patterns,
            tab_width,
        }
    }

    pub fn default_max() -> usize {
        120
    }

    /// Check if line is a shebang (first line starting with #!)
    fn is_shebang(&self, line: &str, line_index: usize) -> bool {
        line_index == 0 && line.starts_with("#!")
    }

    /// Check if line matches any allowed pattern
    fn matches_allowed_pattern(&self, line: &str) -> bool {
        for pattern in &self.allowed_patterns {
            // Strip leading/trailing / if present (regex literal syntax)
            let pat = pattern.trim_matches('/');
            if let Ok(re) = Regex::new(pat) {
                if re.is_match(line) {
                    return true;
                }
            }
        }
        false
    }

    /// Find last URI match in the line. Returns (start_byte, end_byte) of the
    /// URI including any wrapping delimiters (quotes, braces).
    fn find_uri_range(&self, line: &str) -> Option<(usize, usize)> {
        if !self.allow_uri || self.uri_schemes.is_empty() {
            return None;
        }

        let mut last_match: Option<(usize, usize)> = None;

        for scheme in &self.uri_schemes {
            let needle = format!("{}://", scheme);
            // Find all occurrences of this scheme
            let mut search_from = 0;
            while let Some(pos) = line[search_from..].find(&needle) {
                let abs_start = search_from + pos;

                // Find end of URI (whitespace or wrapping delimiter)
                let uri_part = &line[abs_start..];
                let uri_end_offset = uri_part
                    .find(|c: char| {
                        c.is_whitespace() || c == '"' || c == '\'' || c == '>' || c == ')' || c == '}'
                    })
                    .unwrap_or(uri_part.len());
                let abs_end = abs_start + uri_end_offset;

                // Extend range to include wrapping delimiters
                let (range_start, range_end) = self.extend_uri_range(line, abs_start, abs_end);

                // Keep the rightmost match (by start position)
                if last_match.map_or(true, |(prev_start, _)| range_start > prev_start) {
                    last_match = Some((range_start, range_end));
                }
                search_from = abs_end;
            }
        }

        last_match
    }

    /// Extend URI range to include wrapping delimiters (quotes, braces)
    fn extend_uri_range(&self, line: &str, uri_start: usize, uri_end: usize) -> (usize, usize) {
        let bytes = line.as_bytes();
        let mut start = uri_start;
        let mut end = uri_end;

        // Check for opening delimiter before URI
        if uri_start > 0 {
            let prev_byte = bytes[uri_start - 1];
            let (opener, closer) = match prev_byte {
                b'"' => (b'"', b'"'),
                b'\'' => (b'\'', b'\''),
                b'{' => (b'{', b'}'),
                b'(' => (b'(', b')'),
                _ => (0, 0),
            };

            if opener != 0 {
                start = uri_start - 1; // Include opening delimiter
                // Find closing delimiter after URI end
                if let Some(close_pos) = line[uri_end..].find(|c: char| c as u8 == closer) {
                    end = uri_end + close_pos + 1; // Include closing delimiter
                } else if uri_end < line.len() && bytes[uri_end] == closer {
                    end = uri_end + 1;
                }
            }
        }

        (start, end)
    }

    /// Find last qualified name (e.g. ActiveRecord::Base) in the line.
    /// Returns (start_byte, end_byte) including wrapping delimiters.
    fn find_qualified_name_range(&self, line: &str) -> Option<(usize, usize)> {
        if !self.allow_qualified_name {
            return None;
        }

        // Match Ruby qualified names: Word::Word (at least one ::)
        let re = Regex::new(r"[A-Z]\w*(?:::[A-Z]\w*)+(?:\.\w+)?").ok()?;
        let mut last_match: Option<(usize, usize)> = None;

        for m in re.find_iter(line) {
            let abs_start = m.start();
            let abs_end = m.end();

            // Extend to include wrapping delimiters
            let (range_start, range_end) = self.extend_qn_range(line, abs_start, abs_end);
            last_match = Some((range_start, range_end));
        }

        last_match
    }

    /// Extend qualified name range to include wrapping delimiters
    fn extend_qn_range(&self, line: &str, qn_start: usize, qn_end: usize) -> (usize, usize) {
        let bytes = line.as_bytes();
        let mut start = qn_start;
        let mut end = qn_end;

        if qn_start > 0 {
            let prev_byte = bytes[qn_start - 1];
            let (opener, closer) = match prev_byte {
                b'"' => (b'"', b'"'),
                b'\'' => (b'\'', b'\''),
                b'{' => (b'{', b'}'),
                _ => (0, 0),
            };

            if opener != 0 {
                start = qn_start - 1;
                if let Some(close_pos) = line[qn_end..].find(|c: char| c as u8 == closer) {
                    end = qn_end + close_pos + 1;
                } else if qn_end < line.len() && bytes[qn_end] == closer {
                    end = qn_end + 1;
                }
            }
        }

        (start, end)
    }

    /// Compute the excessive position (char position) for a line.
    /// Takes into account URI and qualified name ranges.
    /// Returns None if the line should be accepted (no offense).
    /// Returns Some(char_pos) for the start of the offense.
    fn compute_excessive_position(&self, line: &str) -> Option<usize> {
        let max_char_pos = self.visual_pos_to_char_pos(line, self.max);
        let char_len = line.chars().count();

        // Try URI range
        if let Some((uri_byte_start, uri_byte_end)) = self.find_uri_range(line) {
            let uri_char_start = line[..uri_byte_start].chars().count();
            let uri_char_end = line[..uri_byte_end].chars().count();

            if let Some(max_cp) = max_char_pos {
                // Only consider URI if it extends past max (overlaps with excess)
                if uri_char_end > max_cp {
                    // URI starts before max and extends past it
                    if uri_char_start < max_cp {
                        // Check if URI covers all excess (nothing non-URI after it)
                        if uri_char_end >= char_len {
                            return None; // URI covers all excess, accept line
                        }
                        // URI covers some excess; offense starts after URI
                        return Some(uri_char_end);
                    }
                }
            }
        }

        // Try qualified name range
        if let Some((qn_byte_start, qn_byte_end)) = self.find_qualified_name_range(line) {
            let qn_char_start = line[..qn_byte_start].chars().count();
            let qn_char_end = line[..qn_byte_end].chars().count();

            if let Some(max_cp) = max_char_pos {
                // Only consider QN if it extends past max
                if qn_char_end > max_cp {
                    if qn_char_start < max_cp {
                        if qn_char_end >= char_len {
                            return None; // QN covers all excess
                        }
                        return Some(qn_char_end);
                    }
                }
            }
        }

        // No URI/QN adjustment - use max position
        max_char_pos
    }

    /// Convert visual position (with tabs expanded) to character position
    fn visual_pos_to_char_pos(&self, line: &str, visual_pos: usize) -> Option<usize> {
        let mut current_visual = 0;
        for (char_idx, c) in line.chars().enumerate() {
            if current_visual >= visual_pos {
                return Some(char_idx);
            }
            if c == '\t' {
                current_visual += self.tab_width;
            } else {
                current_visual += 1;
            }
        }
        // If visual_pos is beyond the line, return None
        if current_visual >= visual_pos {
            return Some(line.chars().count());
        }
        None
    }

    /// Get line length with tabs expanded (visual length)
    fn line_length(&self, line: &str) -> usize {
        let mut len = 0;
        for c in line.chars() {
            if c == '\t' {
                len += self.tab_width;
            } else {
                len += 1;
            }
        }
        len
    }

    /// Get character count (not visual length)
    fn char_count(&self, line: &str) -> usize {
        line.chars().count()
    }

    /// Check if a line is inside a heredoc body
    fn is_after_end_marker(line: &str) -> bool {
        line == "__END__"
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

        for (line_index, line) in ctx.source.lines().enumerate() {
            // Skip lines after __END__
            if Self::is_after_end_marker(line) {
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

            // Compute the start of the excessive portion
            let char_len = self.char_count(line);
            let excessive_pos = match self.compute_excessive_position(line) {
                Some(pos) => pos,
                None => continue, // URI/QN covers all excess
            };

            // If excessive_pos >= char_len, nothing to highlight
            if excessive_pos >= char_len {
                continue;
            }

            let line_num = line_index as u32 + 1;

            offenses.push(Offense::new(
                self.name(),
                format!("Line is too long. [{}/{}]", visual_len, self.max),
                self.severity(),
                Location::new(line_num, excessive_pos as u32, line_num, char_len as u32),
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
