//! Metrics/BlockLength cop

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
        Self { max, count_comments: false, count_as_one: Vec::new(), allowed_methods: Vec::new(), allowed_patterns: Vec::new() }
    }

    pub fn with_config(max: usize, count_comments: bool, count_as_one: Vec<String>, allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { max, count_comments, count_as_one, allowed_methods, allowed_patterns }
    }

    fn count_lines(&self, node: &ruby_prism::BlockNode, ctx: &CheckContext) -> usize {
        let loc = node.location();
        let start_line = line_number_at(ctx.source, loc.start_offset());
        let end_line = line_number_at(ctx.source, loc.end_offset());
        if end_line <= start_line { return 0; }

        let lines: Vec<&str> = ctx.source.lines().collect();
        let (body_start, body_end) = (start_line + 1, end_line);

        if self.count_as_one.is_empty() {
            (body_start..body_end).filter(|&i| {
                lines.get(i).map_or(false, |line| {
                    let t = line.trim();
                    !t.is_empty() && (self.count_comments || !t.starts_with('#'))
                })
            }).count()
        } else {
            self.count_lines_with_folds(&lines, body_start, body_end)
        }
    }

    fn count_lines_with_folds(&self, lines: &[&str], body_start: usize, body_end: usize) -> usize {
        let mut count = 0;
        let mut i = body_start;
        while i < body_end {
            let trimmed = match lines.get(i) { Some(l) => l.trim(), None => break };
            if trimmed.is_empty() || (!self.count_comments && trimmed.starts_with('#')) {
                i += 1;
                continue;
            }
            if self.count_as_one.iter().any(|s| s == "array") && trimmed.contains('[') {
                if let Some(end_idx) = Self::find_closing_bracket(lines, i, body_end, '[', ']') {
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

    fn find_closing_bracket(lines: &[&str], start: usize, end: usize, open: char, close: char) -> Option<usize> {
        let mut depth = 0;
        for i in start..end {
            if let Some(line) = lines.get(i) {
                for ch in line.chars() {
                    if ch == open { depth += 1; }
                    else if ch == close {
                        depth -= 1;
                        if depth == 0 { return Some(i); }
                    }
                }
            }
        }
        None
    }

    fn qualified_method_name(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> String {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        match node.receiver() {
            Some(receiver) => {
                let loc = receiver.location();
                let recv_text = ctx.source.get(loc.start_offset()..loc.end_offset()).unwrap_or("");
                if recv_text.is_empty() { method_name }
                else { format!("{}.{}", recv_text.split_whitespace().collect::<Vec<_>>().join(""), method_name) }
            }
            None => method_name,
        }
    }

    fn matches_pattern(patterns: &[String], method_name: &str, qualified_name: &str) -> bool {
        patterns.iter().any(|pat| {
            let re_pat = pat.trim_matches('/');
            regex::Regex::new(re_pat).map_or(false, |re| re.is_match(qualified_name) || re.is_match(method_name))
        })
    }

    fn is_method_allowed(&self, method_name: &str, qualified_name: &str) -> bool {
        self.allowed_methods.iter().any(|allowed| {
            let is_regex = (allowed.starts_with('/') && allowed.ends_with('/') && allowed.len() > 2)
                || allowed.starts_with("(?");
            if is_regex {
                let pat = if allowed.starts_with('/') { &allowed[1..allowed.len() - 1] } else { allowed.as_str() };
                regex::Regex::new(pat).map_or(false, |re| re.is_match(qualified_name) || re.is_match(method_name))
            } else {
                allowed == method_name || allowed == qualified_name
            }
        }) || Self::matches_pattern(&self.allowed_patterns, method_name, qualified_name)
    }

    fn is_class_or_module_definition(&self, method_name: &str, node: &ruby_prism::CallNode) -> bool {
        if method_name != "new" { return false; }
        node.receiver().and_then(|r| r.as_constant_read_node()).map_or(false, |c| {
            matches!(String::from_utf8_lossy(c.name().as_slice()).as_ref(), "Class" | "Module" | "Struct")
        })
    }

    fn find_end_of_first_line(&self, start: usize, source: &str) -> usize {
        source.as_bytes().iter().skip(start).position(|&b| b == b'\n')
            .map_or(source.len(), |p| start + p)
    }
}

impl Default for BlockLength {
    fn default() -> Self { Self::new(25) }
}

impl Cop for BlockLength {
    fn name(&self) -> &'static str { "Metrics/BlockLength" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let block_node = match node.block() {
            Some(ruby_prism::Node::BlockNode { .. }) => node.block().unwrap().as_block_node().unwrap(),
            _ => return vec![],
        };

        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let qualified_name = self.qualified_method_name(node, ctx);

        if self.is_class_or_module_definition(&method_name, node) { return vec![]; }
        if self.is_method_allowed(&method_name, &qualified_name) { return vec![]; }

        let line_count = self.count_lines(&block_node, ctx);
        if line_count <= self.max { return vec![]; }

        let start = node.location().start_offset();
        vec![ctx.offense_with_range(
            self.name(),
            &format!("Block has too many lines. [{}/{}]", line_count, self.max),
            self.severity(), start, self.find_end_of_first_line(start, ctx.source),
        )]
    }
}

fn line_number_at(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].chars().filter(|&c| c == '\n').count()
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
