use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str = "Avoid the use of `BEGIN` blocks.";

#[derive(Default)]
pub struct BeginBlock;

impl BeginBlock {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for BeginBlock {
    fn name(&self) -> &'static str {
        "Style/BeginBlock"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_pre_execution(
        &self,
        node: &ruby_prism::PreExecutionNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let loc = node.keyword_loc();
        vec![ctx.offense(self.name(), MSG, self.severity(), &loc)]
    }
}

crate::register_cop!("Style/BeginBlock", |_cfg| Some(Box::new(BeginBlock::new())));
