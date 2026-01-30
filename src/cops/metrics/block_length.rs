//! Metrics/BlockLength - Checks if the length of a block exceeds some maximum value.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/metrics/block_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

/// Checks if the length of a block exceeds some maximum value.
/// Comment lines can optionally be ignored.
pub struct BlockLength {
    max: usize,
    count_comments: bool,
}

impl BlockLength {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            count_comments: false,
        }
    }

    /// Count the number of lines in a block
    fn count_lines(&self, node: &ruby_prism::BlockNode, ctx: &CheckContext) -> usize {
        let loc = node.location();
        let start_line = self.offset_to_line(ctx.source, loc.start_offset());
        let end_line = self.offset_to_line(ctx.source, loc.end_offset());

        if end_line <= start_line {
            return 0;
        }

        let total_lines = end_line - start_line - 1; // Exclude opening and closing lines

        if self.count_comments {
            total_lines
        } else {
            // Count non-comment, non-blank lines
            let lines: Vec<&str> = ctx.source.lines().collect();
            let mut count = 0;
            for i in start_line..end_line.saturating_sub(1) {
                if let Some(line) = lines.get(i) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        count += 1;
                    }
                }
            }
            count
        }
    }

    fn offset_to_line(&self, source: &str, offset: usize) -> usize {
        source[..offset.min(source.len())]
            .chars()
            .filter(|&c| c == '\n')
            .count()
    }
}

impl Default for BlockLength {
    fn default() -> Self {
        Self::new(25) // RuboCop default
    }
}

impl Cop for BlockLength {
    fn name(&self) -> &'static str {
        "Metrics/BlockLength"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_block(&self, node: &ruby_prism::BlockNode, ctx: &CheckContext) -> Vec<Offense> {
        let line_count = self.count_lines(node, ctx);

        if line_count > self.max {
            vec![ctx.offense(
                self.name(),
                &format!(
                    "Block has too many lines. [{}/{}]",
                    line_count, self.max
                ),
                self.severity(),
                &node.location(),
            )]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_max(source: &str, max: usize) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(BlockLength::new(max));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_max(source, 5)
    }

    #[test]
    fn allows_short_block() {
        let source = r#"
foo do
  bar
  baz
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_long_block() {
        let source = r#"
foo do
  line1
  line2
  line3
  line4
  line5
  line6
  line7
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("too many lines"));
    }

    #[test]
    fn respects_custom_max() {
        let source = r#"
foo do
  line1
  line2
  line3
end
"#;
        // With max 2, this should fail
        let offenses = check_with_max(source, 2);
        assert_eq!(offenses.len(), 1);

        // With max 10, this should pass
        let offenses = check_with_max(source, 10);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_brace_blocks() {
        let source = "foo { bar; baz; qux }";
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }
}
