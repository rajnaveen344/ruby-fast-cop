//! Style/NumberedParametersLimit
//!
//! Flags blocks using more than `Max` distinct numbered parameters.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Visit};
use std::collections::HashSet;

pub struct NumberedParametersLimit {
    max: usize,
}

impl NumberedParametersLimit {
    pub fn new() -> Self { Self { max: 1 } }
    pub fn with_max(max: usize) -> Self { Self { max: max.min(9) } }
}

impl Default for NumberedParametersLimit {
    fn default() -> Self { Self::new() }
}

fn count_numbered_params(block: &BlockNode, source: &str) -> usize {
    struct V<'a> {
        source: &'a str,
        set: HashSet<String>,
    }
    impl<'a> Visit<'_> for V<'a> {
        fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
            let name = String::from_utf8_lossy(node.name().as_slice());
            let bytes = name.as_bytes();
            if bytes.len() == 2 && bytes[0] == b'_' && (b'1'..=b'9').contains(&bytes[1]) {
                self.set.insert(name.into_owned());
            }
        }
    }
    let mut v = V { source, set: HashSet::new() };
    if let Some(body) = block.body() {
        v.visit(&body);
    }
    v.set.len()
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    max: usize,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &CallNode) {
        if let Some(block) = node.block() {
            if let Some(block_node) = block.as_block_node() {
                if let Some(params) = block_node.parameters() {
                    if params.as_numbered_parameters_node().is_some() {
                        let count = count_numbered_params(&block_node, self.ctx.source);
                        if count > self.max {
                            let word = if self.max > 1 { "parameters" } else { "parameter" };
                            let msg = format!(
                                "Avoid using more than {} numbered {}; {} detected.",
                                self.max, word, count
                            );
                            let start = node.location().start_offset();
                            let end = block_node.location().end_offset();
                            self.offenses.push(self.ctx.offense_with_range(
                                "Style/NumberedParametersLimit", &msg,
                                Severity::Convention, start, end,
                            ));
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for NumberedParametersLimit {
    fn name(&self) -> &'static str { "Style/NumberedParametersLimit" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, max: self.max, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/NumberedParametersLimit", |cfg| {
    let max = cfg.get_cop_config("Style/NumberedParametersLimit")
        .and_then(|c| c.raw.get("Max"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(1);
    Some(Box::new(NumberedParametersLimit::with_max(max)))
});
