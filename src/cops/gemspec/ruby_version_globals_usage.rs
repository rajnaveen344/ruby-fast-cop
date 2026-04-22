//! Gemspec/RubyVersionGlobalsUsage cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct RubyVersionGlobalsUsage;

impl RubyVersionGlobalsUsage {
    pub fn new() -> Self { Self }
}

impl Cop for RubyVersionGlobalsUsage {
    fn name(&self) -> &'static str { "Gemspec/RubyVersionGlobalsUsage" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = RubyVersionVisitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct RubyVersionVisitor<'a> {
    cop: &'a RubyVersionGlobalsUsage,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for RubyVersionVisitor<'_> {
    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode) {
        let name = node_name!(node);
        if name == "RUBY_VERSION" {
            let loc = node.location();
            let msg = "Do not use `RUBY_VERSION` in gemspec file.";
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(), msg, self.cop.severity(),
                loc.start_offset(), loc.end_offset(),
            ));
        }
        ruby_prism::visit_constant_read_node(self, node);
    }

    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode) {
        // Match ::RUBY_VERSION, Ruby::VERSION, ::Ruby::VERSION
        let loc = node.location();
        let text = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        if text == "::RUBY_VERSION" || text == "Ruby::VERSION" || text == "::Ruby::VERSION" {
            let msg = format!("Do not use `{text}` in gemspec file.");
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(), &msg, self.cop.severity(),
                loc.start_offset(), loc.end_offset(),
            ));
            // Don't recurse — avoids double-flagging inner RUBY_VERSION constant
            return;
        }
        ruby_prism::visit_constant_path_node(self, node);
    }
}

crate::register_cop!("Gemspec/RubyVersionGlobalsUsage", |_cfg| Some(Box::new(RubyVersionGlobalsUsage::new())));
