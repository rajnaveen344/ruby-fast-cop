//! Metrics/MethodLength cop

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
        Self { max, count_comments: false, count_as_one: Vec::new(), allowed_methods: Vec::new(), allowed_patterns: Vec::new() }
    }

    pub fn with_config(max: usize, count_comments: bool, count_as_one: Vec<String>, allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { max, count_comments, count_as_one, allowed_methods, allowed_patterns }
    }

    fn count_body_lines(&self, ctx: &CheckContext, body_start: usize, body_end: usize) -> usize {
        let lines: Vec<&str> = ctx.source.lines().collect();
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
            let mut folded = false;
            for &(tag, open, close) in &[("array", '[', ']'), ("hash", '{', '}')] {
                if self.count_as_one.iter().any(|s| s == tag) && trimmed.contains(open) {
                    if let Some(end_idx) = Self::find_closing_bracket(lines, i, body_end, open, close) {
                        count += 1;
                        i = end_idx + 1;
                        folded = true;
                        break;
                    }
                }
            }
            if folded { continue; }
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

    fn is_method_allowed(&self, method_name: &str) -> bool {
        self.allowed_methods.iter().any(|a| a == method_name)
            || self.allowed_patterns.iter().any(|p|
                Regex::new(p.trim_matches('/')).map_or(false, |re| re.is_match(method_name)))
    }

    fn line_at(source: &str, offset: usize) -> usize {
        source[..offset.min(source.len())].chars().filter(|&c| c == '\n').count()
    }

    fn find_end_of_first_line(&self, start: usize, source: &str) -> usize {
        source.as_bytes().iter().skip(start).position(|&b| b == b'\n')
            .map_or(source.len(), |p| start + p)
    }

    fn check_def_body(&self, ctx: &CheckContext, method_name: &str, start_offset: usize, end_offset: usize, body_start_offset: Option<usize>) -> Option<Offense> {
        if self.is_method_allowed(method_name) { return None; }
        let start_line = Self::line_at(ctx.source, start_offset);
        let end_line = Self::line_at(ctx.source, end_offset);
        if end_line <= start_line { return None; }

        let body_start = body_start_offset.map_or(start_line + 1, |o| Self::line_at(ctx.source, o));
        let line_count = self.count_body_lines(ctx, body_start, end_line);
        if line_count <= self.max { return None; }

        Some(ctx.offense_with_range("Metrics/MethodLength",
            &format!("Method has too many lines. [{}/{}]", line_count, self.max),
            self.severity(), start_offset, self.find_end_of_first_line(start_offset, ctx.source)))
    }
}

impl Cop for MethodLength {
    fn name(&self) -> &'static str { "Metrics/MethodLength" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let loc = node.location();
        let body_start = node.body().map(|b| b.location().start_offset()).unwrap_or(loc.start_offset());
        self.check_def_body(ctx, &String::from_utf8_lossy(node.name().as_slice()),
            loc.start_offset(), loc.end_offset(), Some(body_start)).into_iter().collect()
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if String::from_utf8_lossy(node.name().as_slice()) != "define_method" { return vec![]; }
        let block_node = match node.block() {
            Some(ruby_prism::Node::BlockNode { .. }) => node.block().unwrap().as_block_node().unwrap(),
            _ => return vec![],
        };

        let defined_name = node.arguments().and_then(|args| {
            args.arguments().iter().next().and_then(|a| a.as_symbol_node())
                .map(|sym| String::from_utf8_lossy(sym.unescaped().as_ref()).to_string())
        }).unwrap_or_default();

        if !defined_name.is_empty() && self.is_method_allowed(&defined_name) { return vec![]; }

        let block_loc = block_node.location();
        let start_line = Self::line_at(ctx.source, block_loc.start_offset());
        let end_line = Self::line_at(ctx.source, block_loc.end_offset());
        if end_line <= start_line { return vec![]; }

        let line_count = self.count_body_lines(ctx, start_line + 1, end_line);
        if line_count <= self.max { return vec![]; }

        let start = node.location().start_offset();
        vec![ctx.offense_with_range(self.name(),
            &format!("Method has too many lines. [{}/{}]", line_count, self.max),
            self.severity(), start, self.find_end_of_first_line(start, ctx.source))]
    }
}
