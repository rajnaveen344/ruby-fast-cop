//! Metrics/MethodLength - Checks if the length of a method exceeds some maximum value.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/metrics/method_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;

pub struct MethodLength {
    max: usize,
    count_comments: bool,
    count_as_one: Vec<String>,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl MethodLength {
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

    /// Count lines in a method body between start_line and end_line (0-indexed, exclusive).
    fn count_body_lines(&self, ctx: &CheckContext, body_start: usize, body_end: usize) -> usize {
        let lines: Vec<&str> = ctx.source.lines().collect();

        if self.count_as_one.is_empty() {
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
            self.count_lines_with_folds(&lines, body_start, body_end)
        }
    }

    /// Count lines, folding multi-line constructs into one line each.
    fn count_lines_with_folds(
        &self,
        lines: &[&str],
        body_start: usize,
        body_end: usize,
    ) -> usize {
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

            // Check for foldable constructs
            if self.count_as_one.contains(&"array".to_string()) && trimmed.contains('[') {
                if let Some(end_idx) = Self::find_closing_bracket(lines, i, body_end, '[', ']') {
                    count += 1;
                    i = end_idx + 1;
                    continue;
                }
            }

            if self.count_as_one.contains(&"hash".to_string()) && trimmed.contains('{') {
                if let Some(end_idx) = Self::find_closing_bracket(lines, i, body_end, '{', '}') {
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

    fn find_closing_bracket(
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

    /// Check if a method name is allowed.
    fn is_method_allowed(&self, method_name: &str) -> bool {
        for allowed in &self.allowed_methods {
            if allowed == method_name {
                return true;
            }
        }

        for pattern in &self.allowed_patterns {
            let pat = pattern.trim_matches('/');
            // Handle (?-mix:...) style patterns
            let pat = if pat.starts_with("(?") {
                pat.to_string()
            } else {
                pat.to_string()
            };
            if let Ok(re) = Regex::new(&pat) {
                if re.is_match(method_name) {
                    return true;
                }
            }
        }

        false
    }

    /// Get the 0-indexed line number for a byte offset.
    fn line_at(source: &str, offset: usize) -> usize {
        source[..offset.min(source.len())]
            .chars()
            .filter(|&c| c == '\n')
            .count()
    }

    /// Check a def node and return an offense if too long.
    fn check_def_body(
        &self,
        ctx: &CheckContext,
        method_name: &str,
        start_offset: usize,
        end_offset: usize,
        body_start_offset: Option<usize>,
    ) -> Option<Offense> {
        // Skip allowed methods
        if self.is_method_allowed(method_name) {
            return None;
        }

        let start_line = Self::line_at(ctx.source, start_offset);
        let end_line = Self::line_at(ctx.source, end_offset);

        // One-liner: def on same line as end
        if end_line <= start_line {
            return None;
        }

        // Body starts at the body node's line (skipping multiline params)
        let body_start = match body_start_offset {
            Some(offset) => Self::line_at(ctx.source, offset),
            None => start_line + 1,
        };
        let body_end = end_line;

        let line_count = self.count_body_lines(ctx, body_start, body_end);

        if line_count > self.max {
            let end_of_first_line = self.find_end_of_first_line(start_offset, ctx.source);
            Some(ctx.offense_with_range(
                "Metrics/MethodLength",
                &format!("Method has too many lines. [{}/{}]", line_count, self.max),
                self.severity(),
                start_offset,
                end_of_first_line,
            ))
        } else {
            None
        }
    }

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

impl Cop for MethodLength {
    fn name(&self) -> &'static str {
        "Metrics/MethodLength"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();

        // Use the body node's location to skip multiline parameter lines
        let body_start_offset = node
            .body()
            .map(|b| b.location().start_offset())
            .unwrap_or_else(|| loc.start_offset());

        if let Some(offense) =
            self.check_def_body(ctx, &method_name, loc.start_offset(), loc.end_offset(), Some(body_start_offset))
        {
            vec![offense]
        } else {
            vec![]
        }
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Check for define_method(:name) do ... end
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if method_name != "define_method" {
            return vec![];
        }

        // Must have a block
        let block = match node.block() {
            Some(block) => block,
            None => return vec![],
        };

        let block_node = match block {
            ruby_prism::Node::BlockNode { .. } => block.as_block_node().unwrap(),
            _ => return vec![],
        };

        // Get the defined method name from the first argument
        let defined_name = if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let Some(first_arg) = arg_list.first() {
                match first_arg {
                    ruby_prism::Node::SymbolNode { .. } => {
                        let sym = first_arg.as_symbol_node().unwrap();
                        String::from_utf8_lossy(sym.unescaped().as_ref()).to_string()
                    }
                    _ => String::new(),
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Skip allowed methods
        if !defined_name.is_empty() && self.is_method_allowed(&defined_name) {
            return vec![];
        }

        let block_loc = block_node.location();
        let start_line = Self::line_at(ctx.source, block_loc.start_offset());
        let end_line = Self::line_at(ctx.source, block_loc.end_offset());

        if end_line <= start_line {
            return vec![];
        }

        let body_start = start_line + 1;
        let body_end = end_line;
        let line_count = self.count_body_lines(ctx, body_start, body_end);

        if line_count > self.max {
            let call_loc = node.location();
            let end_of_first_line = self.find_end_of_first_line(call_loc.start_offset(), ctx.source);
            vec![ctx.offense_with_range(
                self.name(),
                &format!("Method has too many lines. [{}/{}]", line_count, self.max),
                self.severity(),
                call_loc.start_offset(),
                end_of_first_line,
            )]
        } else {
            vec![]
        }
    }
}
