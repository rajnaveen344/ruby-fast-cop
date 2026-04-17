//! Layout/EmptyLinesAroundMethodBody
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_around_method_body.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::empty_lines_around_body::{check, Style};
use crate::helpers::source::{line_byte_offset, line_end_byte_offset};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct EmptyLinesAroundMethodBody;

impl EmptyLinesAroundMethodBody {
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

fn line_is_blank(source: &str, line_1idx: usize) -> bool {
    let start = line_byte_offset(source, line_1idx);
    let end = line_end_byte_offset(source, line_1idx);
    let line = &source[start..end];
    line.trim_end_matches('\n').trim_end_matches('\r').is_empty()
}

struct Visitor<'a> {
    source: &'a str,
    severity: Severity,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_endless(&mut self, node: &ruby_prism::DefNode<'a>) {
        // Endless methods: `def foo = body`. Only offending when body is not on the
        // same or next line after `=` AND the line immediately after `=` is blank.
        let Some(assign_loc) = node.equal_loc() else { return };
        let Some(body) = node.body() else { return };
        let assign_line = line_of(self.source, assign_loc.start_offset());
        let body_first_line = line_of(self.source, body.location().start_offset());

        if body_first_line <= assign_line + 1 {
            return;
        }
        // Check if the line after `=` is blank (i.e. the first body line beginning).
        if !line_is_blank(self.source, assign_line + 1) {
            return;
        }

        let target_line = assign_line + 1;
        let byte_offset = line_byte_offset(self.source, target_line);
        // `line_end_byte_offset` already returns the position after the trailing '\n',
        // so use it directly to consume just the blank line.
        let line_end = line_end_byte_offset(self.source, target_line);

        let msg = "Extra empty line detected at method body beginning.";
        let loc = Location::from_offsets(self.source, byte_offset, byte_offset);

        let correction = Correction {
            edits: vec![Edit {
                start_offset: byte_offset,
                end_offset: line_end,
                replacement: String::new(),
            }],
        };

        self.offenses.push(
            Offense::new(
                "Layout/EmptyLinesAroundMethodBody",
                msg,
                self.severity,
                loc,
                self.ctx.filename,
            )
            .with_correction(correction),
        );
    }
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'a>) {
        if node.equal_loc().is_some() {
            self.check_endless(node);
            ruby_prism::visit_def_node(self, node);
            return;
        }

        let def_first_line = line_of(self.source, node.location().start_offset());
        let last_line = last_line_of(self.source, node.location().end_offset());

        // Adjusted first line: end of parameters (if present), else def line.
        let adjusted_first = if let Some(rparen) = node.rparen_loc() {
            line_of(self.source, rparen.start_offset())
        } else if let Some(params) = node.parameters() {
            last_line_of(self.source, params.location().end_offset())
        } else {
            def_first_line
        };

        let body = node.body();
        self.offenses.extend(check(
            "Layout/EmptyLinesAroundMethodBody",
            self.severity,
            "method",
            Style::NoEmptyLines,
            adjusted_first,
            last_line,
            body.as_ref(),
            self.source,
            self.ctx,
        ));

        ruby_prism::visit_def_node(self, node);
    }
}

impl Cop for EmptyLinesAroundMethodBody {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAroundMethodBody"
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

crate::register_cop!("Layout/EmptyLinesAroundMethodBody", |_cfg| {
    Some(Box::new(EmptyLinesAroundMethodBody::new()))
});
