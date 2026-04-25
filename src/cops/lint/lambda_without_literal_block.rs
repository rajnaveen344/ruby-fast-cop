//! Lint/LambdaWithoutLiteralBlock cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

const MSG: &str = "lambda without a literal block is deprecated; use the proc without lambda instead.";

#[derive(Default)]
pub struct LambdaWithoutLiteralBlock;

impl LambdaWithoutLiteralBlock {
    pub fn new() -> Self { Self }
}

impl Cop for LambdaWithoutLiteralBlock {
    fn name(&self) -> &'static str { "Lint/LambdaWithoutLiteralBlock" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node).as_ref() != "lambda" { return vec![]; }
        if node.receiver().is_some() { return vec![]; }
        let block = match node.block() { Some(b) => b, None => return vec![] };
        let ba = match block.as_block_argument_node() { Some(b) => b, None => return vec![] };
        let expr = match ba.expression() { Some(e) => e, None => return vec![] };
        if expr.as_symbol_node().is_some() { return vec![]; }

        let loc = node.location();
        let eloc = expr.location();
        let replacement = ctx.source[eloc.start_offset()..eloc.end_offset()].to_string();
        let offense = ctx.offense_with_range(self.name(), MSG, self.severity(), loc.start_offset(), loc.end_offset())
            .with_correction(Correction::replace(loc.start_offset(), loc.end_offset(), replacement));
        vec![offense]
    }
}

crate::register_cop!("Lint/LambdaWithoutLiteralBlock", |_cfg| Some(Box::new(LambdaWithoutLiteralBlock::new())));
