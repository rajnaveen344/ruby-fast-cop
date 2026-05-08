//! Style/EmptyBlockParameter cop
//!
//! Checks for empty pipes `||` in block parameters.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::BlockNode;

#[derive(Default)]
pub struct EmptyBlockParameter;

impl EmptyBlockParameter {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EmptyBlockParameter {
    fn name(&self) -> &'static str {
        "Style/EmptyBlockParameter"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_block(&self, node: &BlockNode, ctx: &CheckContext) -> Vec<Offense> {
        // Must have parameters node
        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![],
        };

        // Parameters must be a BlockParametersNode
        let bp = match params.as_block_parameters_node() {
            Some(bp) => bp,
            None => return vec![],
        };

        // Must be empty: no inner ParametersNode and no locals
        if bp.parameters().is_some() {
            return vec![];
        }
        if bp.locals().len() > 0 {
            return vec![];
        }

        // Must have opening/closing pipe locs (i.e., `||` present)
        let opening = match bp.opening_loc() {
            Some(loc) => loc,
            None => return vec![],
        };
        let closing = match bp.closing_loc() {
            Some(loc) => loc,
            None => return vec![],
        };

        let msg = "Omit pipes for the empty block parameters.";
        let start = opening.start_offset();
        let end = closing.end_offset();
        let offense = ctx.offense_with_range(self.name(), msg, self.severity(), start, end);
        // Delete `||` and surrounding whitespace context.
        // For `do ||` style: eat space before `||` (leaves `do\n` or `do body`)
        // For `{ || }` style: eat trailing space after `||` (preserves `{ body }`)
        let source_bytes = ctx.source.as_bytes();
        let mut delete_start = start;
        let mut delete_end = end;

        // Check if preceded by a space AND the char before the space is not `{`
        // (for brace blocks, eat trailing space instead)
        if delete_start > 0 && source_bytes[delete_start - 1] == b' '
            && delete_start >= 2 && source_bytes[delete_start - 2] != b'{'
        {
            delete_start -= 1;
        } else {
            // Eat trailing space after `||`
            while delete_end < source_bytes.len() && source_bytes[delete_end] == b' ' {
                delete_end += 1;
            }
        }
        vec![offense.with_correction(Correction::delete(delete_start, delete_end))]
    }
}

crate::register_cop!("Style/EmptyBlockParameter", |_cfg| Some(Box::new(EmptyBlockParameter::new())));
