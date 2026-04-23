//! Layout/SpaceInsideRangeLiteral cop
//! Checks for spaces inside range literals.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct SpaceInsideRangeLiteral;

impl SpaceInsideRangeLiteral {
    pub fn new() -> Self { Self }
}

impl Cop for SpaceInsideRangeLiteral {
    fn name(&self) -> &'static str { "Layout/SpaceInsideRangeLiteral" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RangeLiteralVisitor {
            source: ctx.source,
            offenses: Vec::new(),
            ctx,
        };
        let result = ruby_prism::parse(ctx.source.as_bytes());
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct RangeLiteralVisitor<'a> {
    source: &'a str,
    offenses: Vec<Offense>,
    ctx: &'a CheckContext<'a>,
}

impl RangeLiteralVisitor<'_> {
    fn check_range(&mut self, loc: ruby_prism::Location, op_loc: ruby_prism::Location,
                   left_end: Option<usize>, right_start: Option<usize>) {
        let src = self.source;
        let range_start = loc.start_offset();
        let range_end = loc.end_offset();
        let op_start = op_loc.start_offset();
        let op_end = op_loc.end_offset();

        // Check space before operator (between left end and op start)
        // Check space after operator (between op end and right start)
        let has_space_before = left_end.map(|le| le < op_start).unwrap_or(false);
        let has_space_after = right_start.map(|rs| op_end < rs).unwrap_or(false);

        // Only flag if on same line (multi-line intentional ranges are ok)
        // Exception: multiline where there's a space on the same line before newline is flagged
        let op_line = self.ctx.line_of(op_start);
        let range_line_start = self.ctx.line_of(range_start);
        let range_line_end = self.ctx.line_of(range_end);

        // For multiline ranges: only flag if the space is on the same line as the range start
        // and the operator
        let flag = if range_line_start != range_line_end {
            // Multi-line range: flag if left has space before op on same line
            has_space_before && self.ctx.line_of(left_end.unwrap_or(op_start)) == op_line
        } else {
            has_space_before || has_space_after
        };

        if !flag {
            return;
        }

        let msg = "Space inside range literal.";

        // For multi-line ranges, the correction collapses to single line
        let correction_end = if range_line_start != range_line_end {
            // Find right operand start (after newlines)
            right_start.unwrap_or(range_end)
        } else {
            range_end
        };

        // Build corrected: left + op + right (no spaces)
        let left_src = left_end.map(|le| &src[range_start..le]).unwrap_or("");
        let op_src = &src[op_start..op_end];
        let right_src = right_start.map(|rs| &src[rs..range_end]).unwrap_or("");
        let replacement = format!("{}{}{}", left_src, op_src, right_src);

        let offense = self.ctx.offense_with_range(
            "Layout/SpaceInsideRangeLiteral", msg, Severity::Convention,
            range_start,
            if range_line_start != range_line_end { range_end } else { range_end },
        ).with_correction(Correction::replace(range_start, range_end, replacement));

        self.offenses.push(offense);
    }
}

impl Visit<'_> for RangeLiteralVisitor<'_> {
    fn visit_range_node(&mut self, node: &ruby_prism::RangeNode) {
        let loc = node.location();
        let op_loc = node.operator_loc();

        let left_end = node.left().map(|l| l.location().end_offset());
        let right_start = node.right().map(|r| r.location().start_offset());

        self.check_range(loc, op_loc, left_end, right_start);
        ruby_prism::visit_range_node(self, node);
    }
}

crate::register_cop!("Layout/SpaceInsideRangeLiteral", |_cfg| Some(Box::new(SpaceInsideRangeLiteral::new())));
