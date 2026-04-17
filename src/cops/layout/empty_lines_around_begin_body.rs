//! Layout/EmptyLinesAroundBeginBody
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_around_begin_body.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::empty_lines_around_body::{check, Style};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct EmptyLinesAroundBeginBody;

impl EmptyLinesAroundBeginBody {
    pub fn new() -> Self {
        Self
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source.as_bytes()[..offset].iter().filter(|&&b| b == b'\n').count()
}

fn last_line_of(source: &str, end: usize) -> usize {
    let last_byte = if end > 0 { end - 1 } else { 0 };
    1 + source.as_bytes()[..=last_byte].iter().filter(|&&b| b == b'\n').count()
}

struct Visitor<'a> {
    source: &'a str,
    severity: Severity,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'a>) {
        // Only fire on real `begin...end` (kwbegin), not implicit rescue wrappers
        // around def/block bodies.
        if node.begin_keyword_loc().is_some() {
            let first_line = line_of(self.source, node.location().start_offset());
            let last_line = last_line_of(self.source, node.location().end_offset());
            self.offenses.extend(check(
                "Layout/EmptyLinesAroundBeginBody",
                self.severity,
                "`begin`",
                Style::NoEmptyLines,
                first_line,
                last_line,
                None,
                self.source,
                self.ctx,
            ));
        }
        ruby_prism::visit_begin_node(self, node);
    }
}

impl Cop for EmptyLinesAroundBeginBody {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAroundBeginBody"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            source: ctx.source,
            severity: self.severity(),
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Layout/EmptyLinesAroundBeginBody", |_cfg| {
    Some(Box::new(EmptyLinesAroundBeginBody::new()))
});
