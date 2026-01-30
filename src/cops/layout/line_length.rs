//! Layout/LineLength - Checks the length of lines in the source code.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/line_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

pub struct LineLength {
    max: usize,
    allow_heredoc: bool,
    allow_uri: bool,
}

impl LineLength {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            allow_heredoc: true,
            allow_uri: true,
        }
    }

    pub fn default_max() -> usize {
        120
    }

    /// Check if line is a shebang (first line starting with #!)
    fn is_shebang(&self, line: &str, line_index: usize) -> bool {
        line_index == 0 && line.starts_with("#!")
    }

    /// Check if line contains a URI that makes it too long
    /// URIs are allowed to exceed the limit if they can't be broken
    fn line_has_uri(&self, line: &str) -> bool {
        if !self.allow_uri {
            return false;
        }
        // Simple check for common URI patterns
        line.contains("http://")
            || line.contains("https://")
            || line.contains("ftp://")
            || line.contains("file://")
    }

    /// Get line length in characters (not bytes)
    fn line_length(&self, line: &str) -> usize {
        line.chars().count()
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

    /// LineLength checks at the program level (whole file)
    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();

        for (line_index, line) in ctx.source.lines().enumerate() {
            let len = self.line_length(line);

            // Skip if within limit
            if len <= self.max {
                continue;
            }

            // Skip shebang lines
            if self.is_shebang(line, line_index) {
                continue;
            }

            // Skip lines with URIs (they often can't be broken)
            if self.line_has_uri(line) {
                continue;
            }

            let line_num = line_index as u32 + 1; // 1-indexed
            offenses.push(Offense::new(
                self.name(),
                format!("Line is too long. [{}/{}]", len, self.max),
                self.severity(),
                Location::new(line_num, (self.max + 1) as u32, line_num, len as u32 + 1),
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
        assert!(offenses[0].message.contains("[81/80]"));
    }

    #[test]
    fn respects_custom_max() {
        let line = "x".repeat(100);

        // Should fail with max 80
        let offenses = check_with_max(&line, 80);
        assert_eq!(offenses.len(), 1);

        // Should pass with max 160
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
        // 80 emoji = 80 characters (not 320 bytes)
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
}
