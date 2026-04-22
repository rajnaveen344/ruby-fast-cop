//! Metrics/ModuleLength cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::code_length::{count_body_lines, find_end_of_first_line, line_number_at};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub struct ModuleLength {
    max: usize,
    count_comments: bool,
    count_as_one: Vec<String>,
}

impl ModuleLength {
    pub fn new(max: usize) -> Self {
        Self { max, count_comments: false, count_as_one: Vec::new() }
    }

    pub fn with_config(max: usize, count_comments: bool, count_as_one: Vec<String>) -> Self {
        Self { max, count_comments, count_as_one }
    }

    fn is_module_receiver(source: &str, receiver: &ruby_prism::Node) -> bool {
        let loc = match receiver {
            ruby_prism::Node::ConstantReadNode { .. } => receiver.as_constant_read_node().unwrap().location(),
            ruby_prism::Node::ConstantPathNode { .. } => receiver.as_constant_path_node().unwrap().location(),
            _ => return false,
        };
        matches!(&source[loc.start_offset()..loc.end_offset()], "Module" | "::Module")
    }
}

impl Cop for ModuleLength {
    fn name(&self) -> &'static str { "Metrics/ModuleLength" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = ModuleLengthVisitor {
            cop: self,
            ctx,
            offenses: &mut offenses,
            depth: 0,
        };
        visitor.visit(&result.node());
        offenses
    }
}

struct ModuleLengthVisitor<'a> {
    cop: &'a ModuleLength,
    ctx: &'a CheckContext<'a>,
    offenses: &'a mut Vec<Offense>,
    depth: usize, // track nesting to exclude inner class/module lines
}

impl ModuleLengthVisitor<'_> {
    fn collect_inner_ranges(&self, body: &ruby_prism::Node) -> Vec<(usize, usize)> {
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

    fn check_body(&mut self, start_offset: usize, end_offset: usize, excluded: &[(usize, usize)]) {
        let start_line = line_number_at(self.ctx.source, start_offset);
        let end_line = line_number_at(self.ctx.source, end_offset);
        if end_line <= start_line { return; }

        let lines: Vec<&str> = self.ctx.source.lines().collect();
        let line_count = count_body_lines(&lines, start_line + 1, end_line, self.cop.count_comments, &self.cop.count_as_one, excluded);
        if line_count <= self.cop.max { return; }

        self.offenses.push(self.ctx.offense_with_range(
            self.cop.name(),
            &format!("Module has too many lines. [{}/{}]", line_count, self.cop.max),
            self.cop.severity(), start_offset, find_end_of_first_line(self.ctx.source, start_offset),
        ));
    }
}

impl Visit<'_> for ModuleLengthVisitor<'_> {
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let excluded = node.body().map(|b| self.collect_inner_ranges(&b)).unwrap_or_default();
        self.check_body(node.location().start_offset(), node.location().end_offset(), &excluded);
        self.depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.depth -= 1;
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        // Don't check class nodes here (that's ClassLength's job)
        // but do recurse — inner singleton classes within a module count toward module lines
        self.depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.depth -= 1;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        // Singleton class lines count toward containing module
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        // Handle `Foo = Module.new do...end`
        if let ruby_prism::Node::CallNode { .. } = node.value() {
            let call = node.value().as_call_node().unwrap();
            if node_name!(call) == "new" {
                if let Some(receiver) = call.receiver() {
                    if ModuleLength::is_module_receiver(self.ctx.source, &receiver) {
                        if let Some(ruby_prism::Node::BlockNode { .. }) = call.block() {
                            let block_node = call.block().unwrap().as_block_node().unwrap();
                            let name_loc = node.name_loc();
                            // Check line count using the whole span but offense on just the name
                            let start_line = line_number_at(self.ctx.source, name_loc.start_offset());
                            let end_line = line_number_at(self.ctx.source, block_node.location().end_offset());
                            if end_line > start_line {
                                let lines: Vec<&str> = self.ctx.source.lines().collect();
                                let line_count = count_body_lines(&lines, start_line + 1, end_line, self.cop.count_comments, &self.cop.count_as_one, &[]);
                                if line_count > self.cop.max {
                                    self.offenses.push(self.ctx.offense_with_range(
                                        self.cop.name(),
                                        &format!("Module has too many lines. [{}/{}]", line_count, self.cop.max),
                                        self.cop.severity(),
                                        name_loc.start_offset(),
                                        name_loc.end_offset(),
                                    ));
                                }
                            }
                            ruby_prism::visit_constant_write_node(self, node);
                            return;
                        }
                    }
                }
            }
        }
        ruby_prism::visit_constant_write_node(self, node);
    }
}

// For `Foo = Module.new do ... end`, offense is on `Foo` (the casgn lhs)
// We handle this by overriding visit for constant assignment

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct ModuleLengthCfg {
    max: usize,
    count_comments: bool,
    #[serde(deserialize_with = "super::seq_or_empty")]
    count_as_one: Vec<String>,
}

impl Default for ModuleLengthCfg {
    fn default() -> Self {
        Self { max: 100, count_comments: false, count_as_one: Vec::new() }
    }
}

crate::register_cop!("Metrics/ModuleLength", |cfg| {
    let c: ModuleLengthCfg = cfg.typed("Metrics/ModuleLength");
    Some(Box::new(ModuleLength::with_config(c.max, c.count_comments, c.count_as_one)))
});
