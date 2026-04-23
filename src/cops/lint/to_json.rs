//! Lint/ToJSON cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/to_json.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;

#[derive(Default)]
pub struct ToJSON;

impl ToJSON {
    pub fn new() -> Self { Self }
}

impl Cop for ToJSON {
    fn name(&self) -> &'static str { "Lint/ToJSON" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let name = node_name!(node);
        if name != "to_json" {
            return vec![];
        }
        // Must have no parameters
        if node.parameters().is_some() {
            return vec![];
        }
        let loc = node.location();
        // Offense spans the entire def line (def to_json) — from start to end of name
        // RuboCop offenses on the node (which is def...end)
        // TOML: line 1 col 0..11 which is "def to_json"
        let name_loc = node.name_loc();
        let offense_end = name_loc.end_offset();
        let offense_start = loc.start_offset();

        // Correction: insert (*_args) after the method name
        let correction = Correction::insert(offense_end, "(*_args)".to_string());

        vec![ctx.offense_with_range(
            "Lint/ToJSON",
            "`#to_json` requires an optional argument to be parsable via JSON.generate(obj).",
            Severity::Warning,
            offense_start,
            offense_end,
        ).with_correction(correction)]
    }
}

crate::register_cop!("Lint/ToJSON", |_cfg| {
    Some(Box::new(ToJSON::new()))
});
