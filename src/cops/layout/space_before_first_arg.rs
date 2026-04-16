//! Layout/SpaceBeforeFirstArg — requires exactly one space between a method
//! name and its first argument for method calls without parentheses.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_before_first_arg.rb
//!
//! Uses the shared `preceding_following_alignment` helper (RuboCop's
//! `PrecedingFollowingAlignment` mixin) for the `AllowForAlignment` check.

use crate::cops::{CheckContext, Cop};
use crate::helpers::preceding_following_alignment::{
    aligned_with_something, AlignRange, AlignmentIndex,
};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct SpaceBeforeFirstArg {
    allow_for_alignment: bool,
}

impl Default for SpaceBeforeFirstArg {
    fn default() -> Self {
        Self { allow_for_alignment: true }
    }
}

impl SpaceBeforeFirstArg {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(allow_for_alignment: bool) -> Self {
        Self { allow_for_alignment }
    }
}

impl Cop for SpaceBeforeFirstArg {
    fn name(&self) -> &'static str { "Layout/SpaceBeforeFirstArg" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let idx = AlignmentIndex::build(ctx.source);
        let mut v = Visitor {
            ctx,
            idx: &idx,
            allow_for_alignment: self.allow_for_alignment,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a, 'b> {
    ctx: &'a CheckContext<'a>,
    idx: &'b AlignmentIndex<'a>,
    allow_for_alignment: bool,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for Visitor<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a, 'b> Visitor<'a, 'b> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        // `regular_method_call_with_arguments?`: has arguments, not operator, not setter.
        let Some(args) = node.arguments() else { return };
        let arg_vec = args.arguments();
        let Some(first_arg) = arg_vec.iter().next() else { return };
        // `node.parenthesized?`: has `(` opener.
        if node.opening_loc().is_some() {
            return;
        }
        let Some(sel) = node.message_loc() else { return };
        let sel_text = &self.ctx.source[sel.start_offset()..sel.end_offset()];
        // Operator method (`+`, `-`, etc.): any non-alnum-underscore selector. Skip.
        if !sel_text.is_empty()
            && sel_text.chars().all(|c| !c.is_alphanumeric() && c != '_')
        {
            return;
        }
        // Setter: name ends with `=` (and selector isn't `==`/`!=`/etc).
        if sel_text.ends_with('=') && sel_text != "==" && sel_text != "!=" && sel_text != "<=" && sel_text != ">=" && sel_text != "===" {
            return;
        }

        let method_end = sel.end_offset();
        let arg_start = first_arg.location().start_offset();
        if method_end > arg_start {
            return;
        }
        // Gap between selector end and first argument start.
        let gap = &self.ctx.source[method_end..arg_start];
        // Must all be spaces/tabs and on same line (no newline).
        if gap.bytes().any(|b| b == b'\n') {
            return;
        }

        // Trim to just the whitespace immediately before the argument (gap is
        // already that whitespace; but for escaped-newline case (`\`) the gap
        // contains `\\` which has no newline but we also should skip).
        if gap.contains('\\') {
            return;
        }
        // `space.length == 1` → OK.
        if gap.len() == 1 {
            return;
        }
        // No space at all: must register.
        let no_space = gap.is_empty();

        // `expect_params_after_method_name?`:
        //   return true if no_space;
        //   return same_line?(first_arg, node) && !(allow_for_alignment && aligned_with_something)
        if !no_space {
            let node_loc = node.location();
            let same_line = self.ctx.same_line(node_loc.start_offset(), arg_start);
            if !same_line {
                return;
            }
            if self.allow_for_alignment {
                let arg_line = self.ctx.line_of(arg_start) as u32;
                let arg_col = self.ctx.col_of(arg_start) as u32;
                let arg_end_col = arg_col + self.arg_token_length(&first_arg);
                let range = AlignRange {
                    line: arg_line,
                    column: arg_col,
                    last_column: arg_end_col,
                    source: &self.ctx.source[arg_start..arg_start + self.arg_token_length(&first_arg) as usize],
                };
                if aligned_with_something(self.idx, range) {
                    return;
                }
            }
        }

        // Offense range = the whitespace gap.
        // Zero-width case: start == end; Location::from_offsets will widen.
        let start = method_end;
        let end = arg_start;
        let correction = Correction::replace(start, end, " ");
        let loc = Location::from_offsets(self.ctx.source, start, end);
        self.offenses.push(
            Offense::new(
                "Layout/SpaceBeforeFirstArg",
                "Put one space between the method name and the first argument.",
                Severity::Convention,
                loc,
                self.ctx.filename,
            )
            .with_correction(correction),
        );
    }

    /// Approximate "length of the first token of the argument" for alignment.
    /// Matches `first_arg.source_range.size` in RuboCop's check.
    fn arg_token_length(&self, arg: &Node) -> u32 {
        let loc = arg.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        // Clamp to end of line.
        let bytes = self.ctx.source.as_bytes();
        let mut e = end;
        let mut k = start;
        while k < end && k < bytes.len() && bytes[k] != b'\n' {
            k += 1;
        }
        if k < end {
            e = k;
        }
        (e - start) as u32
    }
}
