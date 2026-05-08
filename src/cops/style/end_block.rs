use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

const MSG: &str = "Avoid the use of `END` blocks. Use `Kernel#at_exit` instead.";

#[derive(Default)]
pub struct EndBlock;

impl EndBlock {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EndBlock {
    fn name(&self) -> &'static str {
        "Style/EndBlock"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_post_execution(
        &self,
        node: &ruby_prism::PostExecutionNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let loc = node.keyword_loc();
        let offense = ctx.offense(self.name(), MSG, self.severity(), &loc);
        // Replace `END` with `at_exit`; keep the block `{ ... }` as-is
        let correction = Correction {
            edits: vec![Edit {
                start_offset: loc.start_offset(),
                end_offset: loc.end_offset(),
                replacement: "at_exit".to_string(),
            }],
        };
        vec![offense.with_correction(correction)]
    }
}

crate::register_cop!("Style/EndBlock", |_cfg| Some(Box::new(EndBlock::new())));
