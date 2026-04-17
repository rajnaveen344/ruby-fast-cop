//! Metrics/ClassLength cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::code_length::{count_body_lines, find_end_of_first_line, line_number_at};
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
                Some((line_number_at(self.ctx.source, loc.start_offset()),
                      line_number_at(self.ctx.source, loc.end_offset()) + 1))
            } else { None }
        }).collect()
    }

    fn check_class_body(&mut self, start_offset: usize, end_offset: usize, excluded: &[(usize, usize)]) {
        let start_line = line_number_at(self.ctx.source, start_offset);
        let end_line = line_number_at(self.ctx.source, end_offset);
        if end_line <= start_line { return; }

        let lines: Vec<&str> = self.ctx.source.lines().collect();
        let line_count = count_body_lines(&lines, start_line + 1, end_line, self.cop.count_comments, &self.cop.count_as_one, excluded);
        if line_count <= self.cop.max { return; }

        self.offenses.push(self.ctx.offense_with_range(
            self.cop.name(),
            &format!("Class has too many lines. [{}/{}]", line_count, self.cop.max),
            self.cop.severity(), start_offset, find_end_of_first_line(self.ctx.source, start_offset),
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
        if node_name!(node) == "new" {
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

crate::register_cop!("Metrics/ClassLength", |cfg| {
    let cop_config = cfg.get_cop_config("Metrics/ClassLength");
    let max = cop_config.and_then(|c| c.max).unwrap_or(100);
    let count_comments = cop_config
        .and_then(|c| c.raw.get("CountComments"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let count_as_one = cop_config
        .and_then(|c| c.raw.get("CountAsOne"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(ClassLength::with_config(max, count_comments, count_as_one)))
});
