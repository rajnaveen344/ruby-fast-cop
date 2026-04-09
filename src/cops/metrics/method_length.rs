//! Metrics/MethodLength cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::helpers::code_length::{count_body_lines, find_end_of_first_line, line_number_at};
use crate::offense::{Offense, Severity};

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

    fn count_body(&self, ctx: &CheckContext, body_start: usize, body_end: usize) -> usize {
        let lines: Vec<&str> = ctx.source.lines().collect();
        count_body_lines(&lines, body_start, body_end, self.count_comments, &self.count_as_one, &[])
    }

    fn check_def_body(&self, ctx: &CheckContext, method_name: &str, start_offset: usize, end_offset: usize, body_start_offset: Option<usize>) -> Option<Offense> {
        if is_method_allowed(&self.allowed_methods, &self.allowed_patterns, method_name, None) { return None; }
        let start_line = line_number_at(ctx.source, start_offset);
        let end_line = line_number_at(ctx.source, end_offset);
        if end_line <= start_line { return None; }

        let body_start = body_start_offset.map_or(start_line + 1, |o| line_number_at(ctx.source, o));
        let line_count = self.count_body(ctx, body_start, end_line);
        if line_count <= self.max { return None; }

        Some(ctx.offense_with_range("Metrics/MethodLength",
            &format!("Method has too many lines. [{}/{}]", line_count, self.max),
            self.severity(), start_offset, find_end_of_first_line(ctx.source, start_offset)))
    }
}

impl Cop for MethodLength {
    fn name(&self) -> &'static str { "Metrics/MethodLength" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let loc = node.location();
        let body_start = node.body().map(|b| b.location().start_offset()).unwrap_or(loc.start_offset());
        self.check_def_body(ctx, &node_name!(node),
            loc.start_offset(), loc.end_offset(), Some(body_start)).into_iter().collect()
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "define_method" { return vec![]; }
        let block_node = match node.block() {
            Some(ruby_prism::Node::BlockNode { .. }) => node.block().unwrap().as_block_node().unwrap(),
            _ => return vec![],
        };

        let defined_name = node.arguments().and_then(|args| {
            args.arguments().iter().next().and_then(|a| a.as_symbol_node())
                .map(|sym| String::from_utf8_lossy(sym.unescaped().as_ref()).to_string())
        }).unwrap_or_default();

        if !defined_name.is_empty() && is_method_allowed(&self.allowed_methods, &self.allowed_patterns, &defined_name, None) { return vec![]; }

        let block_loc = block_node.location();
        let start_line = line_number_at(ctx.source, block_loc.start_offset());
        let end_line = line_number_at(ctx.source, block_loc.end_offset());
        if end_line <= start_line { return vec![]; }

        let line_count = self.count_body(ctx, start_line + 1, end_line);
        if line_count <= self.max { return vec![]; }

        let start = node.location().start_offset();
        vec![ctx.offense_with_range(self.name(),
            &format!("Method has too many lines. [{}/{}]", line_count, self.max),
            self.severity(), start, find_end_of_first_line(ctx.source, start))]
    }
}
