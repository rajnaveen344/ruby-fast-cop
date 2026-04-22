//! Style/EmptyBlockParameter cop
//!
//! Checks for empty pipes `||` in block parameters.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
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
        vec![ctx.offense_with_range(self.name(), msg, self.severity(), start, end)]
    }
}

crate::register_cop!("Style/EmptyBlockParameter", |_cfg| Some(Box::new(EmptyBlockParameter::new())));
