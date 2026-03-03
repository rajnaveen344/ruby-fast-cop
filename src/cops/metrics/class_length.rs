//! Metrics/ClassLength - Checks if the length of a class exceeds some maximum value.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/metrics/class_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct ClassLength {
    max: usize,
    count_comments: bool,
    count_as_one: Vec<String>,
}

impl ClassLength {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            count_comments: false,
            count_as_one: Vec::new(),
        }
    }

    pub fn with_config(max: usize, count_comments: bool, count_as_one: Vec<String>) -> Self {
        Self {
            max,
            count_comments,
            count_as_one,
        }
    }

    /// Get 0-indexed line number for a byte offset.
    fn line_at(source: &str, offset: usize) -> usize {
        source[..offset.min(source.len())]
            .chars()
            .filter(|&c| c == '\n')
            .count()
    }

    fn find_end_of_first_line(start: usize, source: &str) -> usize {
        let bytes = source.as_bytes();
        for i in start..bytes.len() {
            if bytes[i] == b'\n' {
                return i;
            }
        }
        source.len()
    }

    fn count_body_lines(
        &self,
        lines: &[&str],
        body_start: usize,
        body_end: usize,
        excluded_ranges: &[(usize, usize)],
    ) -> usize {
        if self.count_as_one.is_empty() {
            let mut count = 0;
            for i in body_start..body_end {
                if excluded_ranges.iter().any(|&(s, e)| i >= s && i < e) {
                    continue;
                }
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
            self.count_lines_with_folds(lines, body_start, body_end, excluded_ranges)
        }
    }

    fn count_lines_with_folds(
        &self,
        lines: &[&str],
        body_start: usize,
        body_end: usize,
        excluded_ranges: &[(usize, usize)],
    ) -> usize {
        let mut count = 0;
        let mut i = body_start;
        while i < body_end {
            if let Some(&(_, end)) = excluded_ranges.iter().find(|&&(s, e)| i >= s && i < e) {
                i = end;
                continue;
            }
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

    /// Check if a receiver node is Class, ::Class, Struct, or ::Struct
    fn is_class_or_struct_receiver(source: &str, receiver: &ruby_prism::Node) -> bool {
        let loc = match receiver {
            ruby_prism::Node::ConstantReadNode { .. } => {
                receiver.as_constant_read_node().unwrap().location()
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                receiver.as_constant_path_node().unwrap().location()
            }
            _ => return false,
        };
        let text = &source[loc.start_offset()..loc.end_offset()];
        text == "Class" || text == "::Class" || text == "Struct" || text == "::Struct"
    }
}

impl Cop for ClassLength {
    fn name(&self) -> &'static str {
        "Metrics/ClassLength"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = ClassLengthVisitor {
            cop: self,
            ctx,
            offenses: &mut offenses,
            inside_class: false,
        };
        visitor.visit(&result.node());
        offenses
    }
}

struct ClassLengthVisitor<'a> {
    cop: &'a ClassLength,
    ctx: &'a CheckContext<'a>,
    offenses: &'a mut Vec<Offense>,
    inside_class: bool,
}

impl ClassLengthVisitor<'_> {
    /// Collect line ranges of inner ClassNode and ModuleNode within a body node.
    fn collect_inner_class_ranges(&self, body: &ruby_prism::Node) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        // The body is typically a StatementsNode. Check its direct children
        // for inner class/module definitions.
        if let ruby_prism::Node::StatementsNode { .. } = body {
            let stmts = body.as_statements_node().unwrap();
            for stmt in stmts.body().iter() {
                match &stmt {
                    ruby_prism::Node::ClassNode { .. } => {
                        let loc = stmt.location();
                        let start_line =
                            ClassLength::line_at(self.ctx.source, loc.start_offset());
                        let end_line =
                            ClassLength::line_at(self.ctx.source, loc.end_offset());
                        ranges.push((start_line, end_line + 1));
                    }
                    ruby_prism::Node::ModuleNode { .. } => {
                        let loc = stmt.location();
                        let start_line =
                            ClassLength::line_at(self.ctx.source, loc.start_offset());
                        let end_line =
                            ClassLength::line_at(self.ctx.source, loc.end_offset());
                        ranges.push((start_line, end_line + 1));
                    }
                    _ => {}
                }
            }
        }
        ranges
    }
}

impl Visit<'_> for ClassLengthVisitor<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let loc = node.location();
        let start_line = ClassLength::line_at(self.ctx.source, loc.start_offset());
        let end_line = ClassLength::line_at(self.ctx.source, loc.end_offset());

        if end_line > start_line {
            let body_start = start_line + 1;
            let body_end = end_line;

            // Find inner class/module ranges to exclude
            let excluded_ranges = if let Some(body) = node.body() {
                self.collect_inner_class_ranges(&body)
            } else {
                vec![]
            };

            let lines: Vec<&str> = self.ctx.source.lines().collect();
            let line_count =
                self.cop
                    .count_body_lines(&lines, body_start, body_end, &excluded_ranges);

            if line_count > self.cop.max {
                let end_of_first_line =
                    ClassLength::find_end_of_first_line(loc.start_offset(), self.ctx.source);
                self.offenses.push(self.ctx.offense_with_range(
                    self.cop.name(),
                    &format!(
                        "Class has too many lines. [{}/{}]",
                        line_count, self.cop.max
                    ),
                    self.cop.severity(),
                    loc.start_offset(),
                    end_of_first_line,
                ));
            }
        }

        let was_inside = self.inside_class;
        self.inside_class = true;
        ruby_prism::visit_class_node(self, node);
        self.inside_class = was_inside;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        // Only check standalone singleton classes (not nested inside a regular class)
        if !self.inside_class {
            let loc = node.location();
            let start_line = ClassLength::line_at(self.ctx.source, loc.start_offset());
            let end_line = ClassLength::line_at(self.ctx.source, loc.end_offset());

            if end_line > start_line {
                let body_start = start_line + 1;
                let body_end = end_line;
                let lines: Vec<&str> = self.ctx.source.lines().collect();
                let line_count = self.cop.count_body_lines(&lines, body_start, body_end, &[]);

                if line_count > self.cop.max {
                    let end_of_first_line =
                        ClassLength::find_end_of_first_line(loc.start_offset(), self.ctx.source);
                    self.offenses.push(self.ctx.offense_with_range(
                        self.cop.name(),
                        &format!(
                            "Class has too many lines. [{}/{}]",
                            line_count, self.cop.max
                        ),
                        self.cop.severity(),
                        loc.start_offset(),
                        end_of_first_line,
                    ));
                }
            }
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if method_name == "new" {
            if let Some(receiver) = node.receiver() {
                if ClassLength::is_class_or_struct_receiver(self.ctx.source, &receiver) {
                    if let Some(block) = node.block() {
                        if let ruby_prism::Node::BlockNode { .. } = block {
                            let block_node = block.as_block_node().unwrap();
                            let block_loc = block_node.location();
                            let start_line =
                                ClassLength::line_at(self.ctx.source, block_loc.start_offset());
                            let end_line =
                                ClassLength::line_at(self.ctx.source, block_loc.end_offset());

                            if end_line > start_line {
                                let body_start = start_line + 1;
                                let body_end = end_line;
                                let lines: Vec<&str> = self.ctx.source.lines().collect();
                                let line_count = self.cop.count_body_lines(
                                    &lines,
                                    body_start,
                                    body_end,
                                    &[],
                                );

                                if line_count > self.cop.max {
                                    // Offense location: from receiver start to end of first line
                                    let receiver_start = match &receiver {
                                        ruby_prism::Node::ConstantReadNode { .. } => {
                                            receiver
                                                .as_constant_read_node()
                                                .unwrap()
                                                .location()
                                                .start_offset()
                                        }
                                        ruby_prism::Node::ConstantPathNode { .. } => {
                                            receiver
                                                .as_constant_path_node()
                                                .unwrap()
                                                .location()
                                                .start_offset()
                                        }
                                        _ => node.location().start_offset(),
                                    };
                                    let end_of_first_line = ClassLength::find_end_of_first_line(
                                        receiver_start,
                                        self.ctx.source,
                                    );
                                    self.offenses.push(self.ctx.offense_with_range(
                                        self.cop.name(),
                                        &format!(
                                            "Class has too many lines. [{}/{}]",
                                            line_count, self.cop.max
                                        ),
                                        self.cop.severity(),
                                        receiver_start,
                                        end_of_first_line,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}
