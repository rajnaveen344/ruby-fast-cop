//! Style/OptionalBooleanParameter cop
//!
//! Checks for optional boolean arguments; suggests keyword arguments instead.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{DefNode, Node, Visit};

pub struct OptionalBooleanParameter {
    allowed_methods: Vec<String>,
}

impl OptionalBooleanParameter {
    pub fn new(allowed_methods: Vec<String>) -> Self {
        Self { allowed_methods }
    }
}

impl Default for OptionalBooleanParameter {
    fn default() -> Self {
        Self::new(vec!["respond_to_missing?".to_string()])
    }
}

impl Cop for OptionalBooleanParameter {
    fn name(&self) -> &'static str {
        "Style/OptionalBooleanParameter"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = OptBoolVisitor {
            ctx,
            allowed_methods: &self.allowed_methods,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct OptBoolVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    allowed_methods: &'a [String],
    offenses: Vec<Offense>,
}

impl<'a> OptBoolVisitor<'a> {
    fn check_def(&mut self, node: &DefNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if self.allowed_methods.iter().any(|m| m == &method_name) {
            return;
        }

        let params = match node.parameters() {
            Some(p) => p,
            None => return,
        };

        for opt in params.optionals().iter() {
            // Each optional is an OptionalParameterNode
            let opt_node = match opt.as_optional_parameter_node() {
                Some(n) => n,
                None => continue,
            };

            let value = opt_node.value();
            let is_bool = matches!(value, Node::TrueNode { .. } | Node::FalseNode { .. });
            if !is_bool {
                continue;
            }

            // Build message
            let param_name = String::from_utf8_lossy(opt_node.name().as_slice()).to_string();
            let value_src = &self.ctx.source[value.location().start_offset()..value.location().end_offset()];
            let original = &self.ctx.source[opt_node.location().start_offset()..opt_node.location().end_offset()];
            let replacement = format!("{}: {}", param_name, value_src);
            let msg = format!(
                "Prefer keyword arguments for arguments with a boolean default value; use `{}` instead of `{}`.",
                replacement, original
            );

            self.offenses.push(self.ctx.offense_with_range(
                "Style/OptionalBooleanParameter",
                &msg,
                Severity::Convention,
                opt_node.location().start_offset(),
                opt_node.location().end_offset(),
            ));
        }
    }
}

impl Visit<'_> for OptBoolVisitor<'_> {
    fn visit_def_node(&mut self, node: &DefNode) {
        self.check_def(node);
        ruby_prism::visit_def_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_methods: Vec<String>,
}

crate::register_cop!("Style/OptionalBooleanParameter", |cfg| {
    let c: Cfg = cfg.typed("Style/OptionalBooleanParameter");
    Some(Box::new(OptionalBooleanParameter::new(c.allowed_methods)))
});
