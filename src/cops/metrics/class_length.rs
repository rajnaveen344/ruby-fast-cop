//! Metrics/ClassLength cop

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
        Self { max, count_comments: false, count_as_one: Vec::new() }
    }

    pub fn with_config(max: usize, count_comments: bool, count_as_one: Vec<String>) -> Self {
        Self { max, count_comments, count_as_one }
    }

    fn line_at(source: &str, offset: usize) -> usize {
        source[..offset.min(source.len())].chars().filter(|&c| c == '\n').count()
    }

    fn find_end_of_first_line(start: usize, source: &str) -> usize {
        source.as_bytes().iter().skip(start).position(|&b| b == b'\n')
            .map_or(source.len(), |p| start + p)
    }

    fn count_body_lines(&self, lines: &[&str], body_start: usize, body_end: usize, excluded: &[(usize, usize)]) -> usize {
        if self.count_as_one.is_empty() {
            (body_start..body_end).filter(|&i| {
                if excluded.iter().any(|&(s, e)| i >= s && i < e) { return false; }
                lines.get(i).map_or(false, |line| {
                    let t = line.trim();
                    !t.is_empty() && (self.count_comments || !t.starts_with('#'))
                })
            }).count()
        } else {
            self.count_lines_with_folds(lines, body_start, body_end, excluded)
        }
    }

    fn count_lines_with_folds(&self, lines: &[&str], body_start: usize, body_end: usize, excluded: &[(usize, usize)]) -> usize {
        let mut count = 0;
        let mut i = body_start;
        while i < body_end {
            if let Some(&(_, end)) = excluded.iter().find(|&&(s, e)| i >= s && i < e) {
                i = end;
                continue;
            }
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

    fn is_class_or_struct_receiver(source: &str, receiver: &ruby_prism::Node) -> bool {
        let loc = match receiver {
            ruby_prism::Node::ConstantReadNode { .. } => receiver.as_constant_read_node().unwrap().location(),
            ruby_prism::Node::ConstantPathNode { .. } => receiver.as_constant_path_node().unwrap().location(),
            _ => return false,
        };
        matches!(&source[loc.start_offset()..loc.end_offset()], "Class" | "::Class" | "Struct" | "::Struct")
    }
}

impl Cop for ClassLength {
    fn name(&self) -> &'static str { "Metrics/ClassLength" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = ClassLengthVisitor { cop: self, ctx, offenses: &mut offenses, inside_class: false };
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
    fn collect_inner_class_ranges(&self, body: &ruby_prism::Node) -> Vec<(usize, usize)> {
        let stmts = match body {
            ruby_prism::Node::StatementsNode { .. } => body.as_statements_node().unwrap(),
            _ => return vec![],
        };
        stmts.body().iter().filter_map(|stmt| {
            if matches!(&stmt, ruby_prism::Node::ClassNode { .. } | ruby_prism::Node::ModuleNode { .. }) {
                let loc = stmt.location();
                Some((ClassLength::line_at(self.ctx.source, loc.start_offset()),
                      ClassLength::line_at(self.ctx.source, loc.end_offset()) + 1))
            } else { None }
        }).collect()
    }

    fn check_class_body(&mut self, start_offset: usize, end_offset: usize, excluded: &[(usize, usize)]) {
        let start_line = ClassLength::line_at(self.ctx.source, start_offset);
        let end_line = ClassLength::line_at(self.ctx.source, end_offset);
        if end_line <= start_line { return; }

        let lines: Vec<&str> = self.ctx.source.lines().collect();
        let line_count = self.cop.count_body_lines(&lines, start_line + 1, end_line, excluded);
        if line_count <= self.cop.max { return; }

        let end_of_first_line = ClassLength::find_end_of_first_line(start_offset, self.ctx.source);
        self.offenses.push(self.ctx.offense_with_range(
            self.cop.name(),
            &format!("Class has too many lines. [{}/{}]", line_count, self.cop.max),
            self.cop.severity(), start_offset, end_of_first_line,
        ));
    }
}

impl Visit<'_> for ClassLengthVisitor<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let excluded = node.body().map(|b| self.collect_inner_class_ranges(&b)).unwrap_or_default();
        self.check_class_body(node.location().start_offset(), node.location().end_offset(), &excluded);
        let was_inside = self.inside_class;
        self.inside_class = true;
        ruby_prism::visit_class_node(self, node);
        self.inside_class = was_inside;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        if !self.inside_class {
            self.check_class_body(node.location().start_offset(), node.location().end_offset(), &[]);
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if String::from_utf8_lossy(node.name().as_slice()) == "new" {
            if let Some(receiver) = node.receiver() {
                if ClassLength::is_class_or_struct_receiver(self.ctx.source, &receiver) {
                    if let Some(ruby_prism::Node::BlockNode { .. }) = node.block() {
                        let block_node = node.block().unwrap().as_block_node().unwrap();
                        let recv_start = receiver.location().start_offset();
                        self.check_class_body(recv_start, block_node.location().end_offset(), &[]);
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}
