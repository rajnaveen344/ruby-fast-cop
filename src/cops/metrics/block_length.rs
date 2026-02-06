//! Metrics/BlockLength - Checks if the length of a block exceeds some maximum value.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/metrics/block_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

pub struct BlockLength {
    max: usize,
    count_comments: bool,
    count_as_one: Vec<String>,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl BlockLength {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }

    pub fn with_config(
        max: usize,
        count_comments: bool,
        count_as_one: Vec<String>,
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self {
            max,
            count_comments,
            count_as_one,
            allowed_methods,
            allowed_patterns,
        }
    }

    /// Count the effective number of lines in a block body (excluding open/close lines).
    fn count_lines(&self, node: &ruby_prism::BlockNode, ctx: &CheckContext) -> usize {
        let loc = node.location();
        let start_line = line_number_at(ctx.source, loc.start_offset());
        let end_line = line_number_at(ctx.source, loc.end_offset());

        if end_line <= start_line {
            return 0;
        }

        // Body lines are between the opening and closing lines (exclusive of both)
        let lines: Vec<&str> = ctx.source.lines().collect();
        let body_start = start_line + 1; // first line after opening (do/{ line)
        let body_end = end_line; // closing line (end/} line), exclusive

        if self.count_as_one.is_empty() {
            // Simple counting
            let mut count = 0;
            for i in body_start..body_end {
                if let Some(line) = lines.get(i) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if !self.count_comments && trimmed.starts_with('#') {
                        continue;
                    }
                    count += 1;
                }
            }
            count
        } else {
            self.count_lines_with_folds(ctx, body_start, body_end)
        }
    }

    /// Count lines, folding multi-line constructs (arrays, hashes, heredocs) into one line each.
    fn count_lines_with_folds(
        &self,
        ctx: &CheckContext,
        body_start: usize,
        body_end: usize,
    ) -> usize {
        let lines: Vec<&str> = ctx.source.lines().collect();
        let mut count = 0;
        let mut i = body_start;

        while i < body_end {
            let line = match lines.get(i) {
                Some(l) => l,
                None => break,
            };
            let trimmed = line.trim();

            if trimmed.is_empty() {
                i += 1;
                continue;
            }
            if !self.count_comments && trimmed.starts_with('#') {
                i += 1;
                continue;
            }

            // Check if this line starts a foldable construct
            if self.count_as_one.contains(&"array".to_string()) && trimmed.contains('[') {
                // Find matching closing bracket
                if let Some(end_idx) = self.find_closing_bracket(&lines, i, body_end, '[', ']') {
                    count += 1;
                    i = end_idx + 1;
                    continue;
                }
            }

            count += 1;
            i += 1;
        }

        count
    }

    /// Find the line containing the matching closing bracket.
    fn find_closing_bracket(
        &self,
        lines: &[&str],
        start: usize,
        end: usize,
        open: char,
        close: char,
    ) -> Option<usize> {
        let mut depth = 0;
        for i in start..end {
            if let Some(line) = lines.get(i) {
                for ch in line.chars() {
                    if ch == open {
                        depth += 1;
                    } else if ch == close {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                }
            }
        }
        None
    }

    /// Build the fully qualified method name including receiver chain.
    /// e.g., `Foo::Bar.baz` for a call like `Foo::Bar.baz do ... end`
    fn qualified_method_name(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> String {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        match node.receiver() {
            Some(receiver) => {
                let receiver_text = self.receiver_source(&receiver, ctx);
                if receiver_text.is_empty() {
                    method_name
                } else {
                    // Normalize whitespace in the receiver
                    let normalized: String = receiver_text
                        .split_whitespace()
                        .collect::<Vec<&str>>()
                        .join("");
                    format!("{}.{}", normalized, method_name)
                }
            }
            None => method_name,
        }
    }

    /// Extract the source text for a receiver node.
    fn receiver_source(&self, node: &ruby_prism::Node, ctx: &CheckContext) -> String {
        let loc = node.location();
        ctx.source
            .get(loc.start_offset()..loc.end_offset())
            .unwrap_or("")
            .to_string()
    }

    /// Check if the method is in any of the allowed lists.
    fn is_method_allowed(&self, method_name: &str, qualified_name: &str) -> bool {
        for allowed in &self.allowed_methods {
            // Entries wrapped in / or starting with (?  are treated as regex patterns
            let is_regex = (allowed.starts_with('/') && allowed.ends_with('/') && allowed.len() > 2)
                || allowed.starts_with("(?");
            if is_regex {
                let pat = if allowed.starts_with('/') {
                    &allowed[1..allowed.len() - 1]
                } else {
                    allowed.as_str()
                };
                if let Ok(re) = regex::Regex::new(pat) {
                    if re.is_match(qualified_name) || re.is_match(method_name) {
                        return true;
                    }
                }
            } else if allowed == method_name || allowed == qualified_name {
                return true;
            }
        }

        for pattern in &self.allowed_patterns {
            let pat = pattern.trim_matches('/');
            if let Ok(re) = regex::Regex::new(pat) {
                if re.is_match(qualified_name) || re.is_match(method_name) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if this is a class/module/struct definition using a block.
    fn is_class_or_module_definition(
        &self,
        method_name: &str,
        node: &ruby_prism::CallNode,
    ) -> bool {
        if method_name != "new" {
            return false;
        }

        if let Some(receiver) = node.receiver() {
            if let ruby_prism::Node::ConstantReadNode { .. } = receiver {
                let const_node = receiver.as_constant_read_node().unwrap();
                let const_name = String::from_utf8_lossy(const_node.name().as_slice());
                return matches!(const_name.as_ref(), "Class" | "Module" | "Struct");
            }
        }

        false
    }

    /// Find the byte offset of the end of the first line starting from `start`.
    fn find_end_of_first_line(&self, start: usize, source: &str) -> usize {
        let bytes = source.as_bytes();
        for i in start..bytes.len() {
            if bytes[i] == b'\n' {
                return i;
            }
        }
        source.len()
    }
}

impl Default for BlockLength {
    fn default() -> Self {
        Self {
            max: 25,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }
}

impl Cop for BlockLength {
    fn name(&self) -> &'static str {
        "Metrics/BlockLength"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let block = match node.block() {
            Some(block) => block,
            None => return vec![],
        };

        let block_node = match block {
            ruby_prism::Node::BlockNode { .. } => block.as_block_node().unwrap(),
            _ => return vec![],
        };

        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let qualified_name = self.qualified_method_name(node, ctx);

        // Skip class/module/struct definitions
        if self.is_class_or_module_definition(&method_name, node) {
            return vec![];
        }

        // Skip allowed methods (user config + patterns)
        if self.is_method_allowed(&method_name, &qualified_name) {
            return vec![];
        }

        let line_count = self.count_lines(&block_node, ctx);

        if line_count > self.max {
            let start = node.location().start_offset();
            let end = self.find_end_of_first_line(start, ctx.source);

            vec![ctx.offense_with_range(
                self.name(),
                &format!("Block has too many lines. [{}/{}]", line_count, self.max),
                self.severity(),
                start,
                end,
            )]
        } else {
            vec![]
        }
    }
}

/// Return the 0-indexed line number for a byte offset in source.
fn line_number_at(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
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
        let source = "foo do\n  bar\n  baz\nend\n";
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_long_block() {
        let source = "foo do\n  l1\n  l2\n  l3\n  l4\n  l5\n  l6\n  l7\nend\n";
        let offenses = check(source);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("too many lines"));
    }

    #[test]
    fn respects_custom_max() {
        let source = "foo do\n  l1\n  l2\n  l3\nend\n";
        assert_eq!(check_with_max(source, 2).len(), 1);
        assert_eq!(check_with_max(source, 10).len(), 0);
    }

    #[test]
    fn allows_brace_blocks() {
        let offenses = check("foo { bar; baz; qux }");
        assert_eq!(offenses.len(), 0);
    }
}
