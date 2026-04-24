//! Gemspec/AddRuntimeDependency cop
//!
//! `add_runtime_dependency` is deprecated; prefer `add_dependency`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

const MSG: &str = "Use `add_dependency` instead of `add_runtime_dependency`.";

#[derive(Default)]
pub struct AddRuntimeDependency;

impl AddRuntimeDependency {
    pub fn new() -> Self { Self }
}

impl Cop for AddRuntimeDependency {
    fn name(&self) -> &'static str { "Gemspec/AddRuntimeDependency" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "add_runtime_dependency" { return vec![]; }
        if node.receiver().is_none() { return vec![]; }
        if node.arguments().is_none() { return vec![]; }

        let msg_loc = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let start = msg_loc.start_offset();
        let end = msg_loc.end_offset();
        // Sanity: source must read "add_runtime_dependency"
        if &ctx.source[start..end] != "add_runtime_dependency" { return vec![]; }

        let offense = ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, "add_dependency".to_string()));
        vec![offense]
    }
}

crate::register_cop!("Gemspec/AddRuntimeDependency", |_cfg| Some(Box::new(AddRuntimeDependency::new())));
