//! Metrics/BlockLength cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::helpers::code_length::{count_body_lines, find_end_of_first_line, line_number_at};
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
        count_body_lines(&lines, start_line + 1, end_line, self.count_comments, &self.count_as_one, &[])
    }

    fn qualified_method_name(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> String {
        let method_name = node_name!(node).to_string();
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

    fn is_class_or_module_definition(&self, method_name: &str, node: &ruby_prism::CallNode) -> bool {
        if method_name != "new" { return false; }
        node.receiver().and_then(|r| r.as_constant_read_node()).map_or(false, |c| {
            matches!(node_name!(c).as_ref(), "Class" | "Module" | "Struct")
        })
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

        let method_name = node_name!(node).to_string();
        let qualified_name = self.qualified_method_name(node, ctx);

        if self.is_class_or_module_definition(&method_name, node) { return vec![]; }
        if is_method_allowed(&self.allowed_methods, &self.allowed_patterns, &method_name, Some(&qualified_name)) { return vec![]; }

        let line_count = self.count_lines(&block_node, ctx);
        if line_count <= self.max { return vec![]; }

        let start = node.location().start_offset();
        vec![ctx.offense_with_range(
            self.name(),
            &format!("Block has too many lines. [{}/{}]", line_count, self.max),
            self.severity(), start, find_end_of_first_line(ctx.source, start),
        )]
    }
}
