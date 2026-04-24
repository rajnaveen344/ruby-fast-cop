//! Style/ItAssignment
//!
//! Flags using `it` as a local variable or parameter name (Ruby 3.4 default
//! block parameter).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{
    BlockParameterNode, KeywordRestParameterNode, LocalVariableWriteNode,
    OptionalKeywordParameterNode, OptionalParameterNode, RequiredKeywordParameterNode,
    RequiredParameterNode, RestParameterNode, Visit,
};

const MSG: &str = "`it` is the default block parameter; consider another name.";

#[derive(Default)]
pub struct ItAssignment;

impl ItAssignment {
    pub fn new() -> Self { Self }
}

fn is_it(bytes: &[u8]) -> bool { bytes == b"it" }

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> V<'a> {
    fn push(&mut self, start: usize, end: usize) {
        self.offenses.push(self.ctx.offense_with_range(
            "Style/ItAssignment", MSG, Severity::Convention, start, end,
        ));
    }
}

impl<'a> Visit<'_> for V<'a> {
    fn visit_local_variable_write_node(&mut self, node: &LocalVariableWriteNode) {
        if is_it(node.name().as_slice()) {
            let loc = node.name_loc();
            self.push(loc.start_offset(), loc.end_offset());
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_required_parameter_node(&mut self, node: &RequiredParameterNode) {
        if is_it(node.name().as_slice()) {
            let loc = node.location();
            self.push(loc.start_offset(), loc.end_offset());
        }
    }
    fn visit_optional_parameter_node(&mut self, node: &OptionalParameterNode) {
        if is_it(node.name().as_slice()) {
            let loc = node.name_loc();
            self.push(loc.start_offset(), loc.end_offset());
        }
        ruby_prism::visit_optional_parameter_node(self, node);
    }
    fn visit_required_keyword_parameter_node(&mut self, node: &RequiredKeywordParameterNode) {
        let bytes = node.name().as_slice();
        let trimmed: &[u8] = if bytes.last() == Some(&b':') { &bytes[..bytes.len() - 1] } else { bytes };
        if trimmed == b"it" {
            let loc = node.name_loc();
            self.push(loc.start_offset(), loc.end_offset() - 1);
        }
    }
    fn visit_optional_keyword_parameter_node(&mut self, node: &OptionalKeywordParameterNode) {
        let bytes = node.name().as_slice();
        let trimmed: &[u8] = if bytes.last() == Some(&b':') { &bytes[..bytes.len() - 1] } else { bytes };
        if trimmed == b"it" {
            let loc = node.name_loc();
            self.push(loc.start_offset(), loc.end_offset() - 1);
        }
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
    }
    fn visit_rest_parameter_node(&mut self, node: &RestParameterNode) {
        if let Some(name_loc) = node.name_loc() {
            if name_loc.as_slice() == b"it" {
                self.push(name_loc.start_offset(), name_loc.end_offset());
            }
        }
    }
    fn visit_keyword_rest_parameter_node(&mut self, node: &KeywordRestParameterNode) {
        if let Some(name_loc) = node.name_loc() {
            if name_loc.as_slice() == b"it" {
                self.push(name_loc.start_offset(), name_loc.end_offset());
            }
        }
    }
    fn visit_block_parameter_node(&mut self, node: &BlockParameterNode) {
        if let Some(name_loc) = node.name_loc() {
            if name_loc.as_slice() == b"it" {
                self.push(name_loc.start_offset(), name_loc.end_offset());
            }
        }
    }
}

impl Cop for ItAssignment {
    fn name(&self) -> &'static str { "Style/ItAssignment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/ItAssignment", |_cfg| {
    Some(Box::new(ItAssignment::new()))
});
