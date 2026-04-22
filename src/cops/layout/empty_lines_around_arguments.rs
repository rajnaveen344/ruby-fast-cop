//! Layout/EmptyLinesAroundArguments
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/empty_lines_around_arguments.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Layout/EmptyLinesAroundArguments";
const MSG: &str = "Empty line detected around arguments.";

#[derive(Default)]
pub struct EmptyLinesAroundArguments;

impl EmptyLinesAroundArguments {
    pub fn new() -> Self {
        Self
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source[..offset.min(source.len())].bytes().filter(|&b| b == b'\n').count()
}

struct ArgVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    offenses: Vec<Offense>,
}

impl<'a> ArgVisitor<'a> {
    fn is_single_line(&self, node_start: usize, node_end: usize) -> bool {
        line_of(self.source, node_start) == line_of(self.source, node_end.saturating_sub(1))
    }

    /// Find empty line immediately before `offset` (going backward past whitespace on same and previous lines)
    /// Returns the line number of the empty line if found.
    fn empty_line_before(&self, offset: usize) -> Option<usize> {
        // We look backward: from offset, skip whitespace/spaces on current line,
        // if we hit a newline, check if the line before it was empty
        let source = self.source;
        let bytes = source.as_bytes();
        let mut i = offset;
        // Skip backward past spaces/tabs on current portion
        while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
            i -= 1;
        }
        // Now i is at start of line or a newline
        if i == 0 {
            return None;
        }
        // If we're at a newline, look at the line before
        if bytes[i - 1] == b'\n' {
            // Check if the line before that newline is blank
            let prev_line_end = i - 1; // position of the \n
            // Find start of that previous line
            let prev_line_start = if prev_line_end == 0 {
                0
            } else {
                source[..prev_line_end].rfind('\n').map_or(0, |p| p + 1)
            };
            let prev_line = &source[prev_line_start..prev_line_end];
            if prev_line.trim().is_empty() {
                return Some(line_of(source, prev_line_start));
            }
        }
        None
    }

    /// Correct by removing the empty line at the given 1-based line number
    fn remove_empty_line(&self, empty_line: usize) -> Correction {
        let start = line_byte_offset(self.source, empty_line);
        let end = line_byte_offset(self.source, empty_line + 1);
        Correction::delete(start, end)
    }

    fn check_call(&mut self, node: &ruby_prism::CallNode<'a>) {
        // Skip single line
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        if self.is_single_line(node_start, node_end) {
            return;
        }

        // Must have args
        let Some(args_node) = node.arguments() else {
            return;
        };

        // Skip if receiver and method call on different lines
        // (receiver.last_line != selector.line)
        if let Some(receiver) = node.receiver() {
            let recv_last_line = line_of(self.source, receiver.location().end_offset().saturating_sub(1));
            let selector_line = node.message_loc()
                .map(|s| line_of(self.source, s.start_offset()))
                .unwrap_or(recv_last_line);
            if recv_last_line != selector_line {
                return;
            }
        }

        let args: Vec<ruby_prism::Node> = args_node.arguments().iter().collect();
        if args.is_empty() {
            return;
        }

        // Check each arg: look for empty line before arg start
        for arg in &args {
            let arg_start = arg.location().start_offset();
            if let Some(empty_line) = self.empty_line_before(arg_start) {
                let line_start = line_byte_offset(self.source, empty_line);
                let line_end = line_byte_offset(self.source, empty_line + 1);
                let loc = Location::from_offsets(self.source, line_start, line_start + 1);
                let correction = self.remove_empty_line(empty_line);
                self.offenses.push(
                    Offense::new(COP_NAME, MSG, Severity::Convention, loc, self.filename)
                        .with_correction(correction),
                );
            }
        }

        // Check before closing paren
        if let Some(close) = node.closing_loc() {
            let close_start = close.start_offset();
            if let Some(empty_line) = self.empty_line_before(close_start) {
                let line_start = line_byte_offset(self.source, empty_line);
                let loc = Location::from_offsets(self.source, line_start, line_start + 1);
                let correction = self.remove_empty_line(empty_line);
                self.offenses.push(
                    Offense::new(COP_NAME, MSG, Severity::Convention, loc, self.filename)
                        .with_correction(correction),
                );
            }
        }
    }
}

impl<'a> Visit<'a> for ArgVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for EmptyLinesAroundArguments {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = ArgVisitor {
            source: ctx.source,
            filename: ctx.filename,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Layout/EmptyLinesAroundArguments", |_cfg| {
    Some(Box::new(EmptyLinesAroundArguments::new()))
});
