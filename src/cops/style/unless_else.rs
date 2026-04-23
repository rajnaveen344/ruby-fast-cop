use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str =
    "Do not use `unless` with `else`. Rewrite these with the positive case first.";

#[derive(Default)]
pub struct UnlessElse;

impl UnlessElse {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for UnlessElse {
    fn name(&self) -> &'static str {
        "Style/UnlessElse"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        // Flag if unless has an else clause
        if node.else_clause().is_none() {
            return vec![];
        }
        // Offense is the entire unless...end node
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)]
    }
}

crate::register_cop!("Style/UnlessElse", |_cfg| Some(Box::new(UnlessElse::new())));
