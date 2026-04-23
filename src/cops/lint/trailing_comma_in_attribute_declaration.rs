//! Lint/TrailingCommaInAttributeDeclaration cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/trailing_comma_in_attribute_declaration.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;

#[derive(Default)]
pub struct TrailingCommaInAttributeDeclaration;

impl TrailingCommaInAttributeDeclaration {
    pub fn new() -> Self { Self }
}

const ATTR_METHODS: &[&str] = &["attr_reader", "attr_writer", "attr_accessor", "attr"];
const MSG: &str = "Avoid leaving a trailing comma in attribute declarations.";

impl Cop for TrailingCommaInAttributeDeclaration {
    fn name(&self) -> &'static str { "Lint/TrailingCommaInAttributeDeclaration" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if !ATTR_METHODS.iter().any(|m| *m == method) {
            return vec![];
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };

        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() < 2 {
            return vec![];
        }

        // Check if last argument is a DefNode
        let last = &arg_list[arg_list.len() - 1];
        if last.as_def_node().is_none() {
            return vec![];
        }

        // Find the comma between second-to-last arg and last arg
        let prev_end = arg_list[arg_list.len() - 2].location().end_offset();
        let last_start = last.location().start_offset();

        // Scan for comma between prev_end and last_start
        let src_bytes = ctx.source.as_bytes();
        let mut comma_pos = None;
        for i in prev_end..last_start {
            if src_bytes[i] == b',' {
                comma_pos = Some(i);
                break;
            }
        }

        let comma_start = match comma_pos {
            Some(p) => p,
            None => return vec![],
        };
        let comma_end = comma_start + 1;

        // Correction: remove the comma
        let correction = Correction::delete(comma_start, comma_end);

        vec![ctx.offense_with_range(
            "Lint/TrailingCommaInAttributeDeclaration",
            MSG,
            Severity::Warning,
            comma_start,
            comma_end,
        ).with_correction(correction)]
    }
}

crate::register_cop!("Lint/TrailingCommaInAttributeDeclaration", |_cfg| {
    Some(Box::new(TrailingCommaInAttributeDeclaration::new()))
});
