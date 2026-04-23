use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str = "Do not use `::` for defining class methods.";

#[derive(Default)]
pub struct ColonMethodDefinition;

impl ColonMethodDefinition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ColonMethodDefinition {
    fn name(&self) -> &'static str {
        "Style/ColonMethodDefinition"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only singleton methods (def receiver.method or def receiver::method)
        if node.receiver().is_none() {
            return vec![];
        }
        // Check the operator location between receiver and method name
        let op_loc = match node.operator_loc() {
            Some(l) => l,
            None => return vec![],
        };
        if op_loc.as_slice() != b"::" {
            return vec![];
        }
        vec![ctx.offense(self.name(), MSG, self.severity(), &op_loc)]
    }
}

crate::register_cop!("Style/ColonMethodDefinition", |_cfg| {
    Some(Box::new(ColonMethodDefinition::new()))
});
