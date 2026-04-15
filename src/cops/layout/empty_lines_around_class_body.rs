//! Layout/EmptyLinesAroundClassBody
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_around_class_body.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::empty_lines_around_body::{check, Style};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub use crate::helpers::empty_lines_around_body::Style as EmptyLinesAroundClassBodyStyle;

pub struct EmptyLinesAroundClassBody {
    style: Style,
}

impl EmptyLinesAroundClassBody {
    pub fn new(style: Style) -> Self {
        Self { style }
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
    style: Style,
    severity: Severity,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'a>) {
        let first_line = line_of(self.source, node.location().start_offset());
        let last_line = last_line_of(self.source, node.location().end_offset());
        let effective_first = if let Some(sc) = node.superclass() {
            last_line_of(self.source, sc.location().end_offset())
        } else {
            first_line
        };
        let body = node.body();
        self.offenses.extend(check(
            "Layout/EmptyLinesAroundClassBody",
            self.severity,
            "class",
            self.style,
            effective_first,
            last_line,
            body.as_ref(),
            self.source,
            self.ctx,
        ));
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'a>) {
        let first_line = line_of(self.source, node.location().start_offset());
        let last_line = last_line_of(self.source, node.location().end_offset());
        let body = node.body();
        self.offenses.extend(check(
            "Layout/EmptyLinesAroundClassBody",
            self.severity,
            "class",
            self.style,
            first_line,
            last_line,
            body.as_ref(),
            self.source,
            self.ctx,
        ));
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

impl Cop for EmptyLinesAroundClassBody {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAroundClassBody"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            source: ctx.source,
            style: self.style,
            severity: self.severity(),
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}
