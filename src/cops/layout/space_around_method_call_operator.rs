//! Layout/SpaceAroundMethodCallOperator - No spaces around `.`, `&.`, `::`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_around_method_call_operator.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Avoid using spaces around a method call operator.";

#[derive(Default)]
pub struct SpaceAroundMethodCallOperator;

impl SpaceAroundMethodCallOperator {
    pub fn new() -> Self { Self }
}

impl Cop for SpaceAroundMethodCallOperator {
    fn name(&self) -> &'static str { "Layout/SpaceAroundMethodCallOperator" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Emit offense if [begin_pos, end_pos) contains only spaces/tabs on same line.
    fn check_space(&mut self, begin_pos: usize, end_pos: usize) {
        if end_pos <= begin_pos { return; }
        let slice = &self.ctx.source.as_bytes()[begin_pos..end_pos];
        if slice.is_empty() || !slice.iter().all(|&b| b == b' ' || b == b'\t') {
            return;
        }
        let loc = Location::from_offsets(self.ctx.source, begin_pos, end_pos);
        self.offenses.push(
            Offense::new(
                "Layout/SpaceAroundMethodCallOperator",
                MSG,
                Severity::Convention,
                loc,
                self.ctx.filename,
            )
            .with_correction(Correction::delete(begin_pos, end_pos)),
        );
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(dot) = node.call_operator_loc() {
            let dot_begin = dot.start_offset();
            let dot_end = dot.end_offset();

            // Space before dot: between receiver end and dot start (same line only).
            if let Some(recv) = node.receiver() {
                let recv_end = recv.location().end_offset();
                if recv_end < dot_begin && same_line(self.ctx.source, recv_end, dot_begin) {
                    self.check_space(recv_end, dot_begin);
                }
            }

            // Space after dot: between dot end and selector/begin start.
            // Proc#call shorthand `foo.()`: no selector → use opening paren.
            let selector_begin = if let Some(sel) = node.message_loc() {
                Some(sel.start_offset())
            } else {
                node.opening_loc().map(|l| l.start_offset())
            };
            if let Some(sel_begin) = selector_begin {
                if dot_end < sel_begin && same_line(self.ctx.source, dot_end, sel_begin) {
                    self.check_space(dot_end, sel_begin);
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode) {
        let delim = node.delimiter_loc();
        let delim_end = delim.end_offset();
        let name_begin = node.name_loc().start_offset();
        if delim_end < name_begin && same_line(self.ctx.source, delim_end, name_begin) {
            self.check_space(delim_end, name_begin);
        }
        ruby_prism::visit_constant_path_node(self, node);
    }
}

fn same_line(source: &str, a: usize, b: usize) -> bool {
    !source.as_bytes()[a.min(b)..a.max(b)].contains(&b'\n')
}
