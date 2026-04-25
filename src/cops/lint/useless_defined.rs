//! Lint/UselessDefined cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG_STRING: &str = "Calling `defined?` with a string argument will always return a truthy value.";
const MSG_SYMBOL: &str = "Calling `defined?` with a symbol argument will always return a truthy value.";

#[derive(Default)]
pub struct UselessDefined;

impl UselessDefined {
    pub fn new() -> Self { Self }
}

impl Cop for UselessDefined {
    fn name(&self) -> &'static str { "Lint/UselessDefined" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    out: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_defined_node(&mut self, node: &ruby_prism::DefinedNode) {
        let val = node.value();
        let msg = if val.as_string_node().is_some() || val.as_interpolated_string_node().is_some() {
            Some(MSG_STRING)
        } else if val.as_symbol_node().is_some() || val.as_interpolated_symbol_node().is_some() {
            Some(MSG_SYMBOL)
        } else {
            None
        };
        if let Some(m) = msg {
            let loc = node.location();
            self.out.push(self.ctx.offense_with_range(
                "Lint/UselessDefined", m, Severity::Warning,
                loc.start_offset(), loc.end_offset(),
            ));
        }
        ruby_prism::visit_defined_node(self, node);
    }
}

crate::register_cop!("Lint/UselessDefined", |_cfg| Some(Box::new(UselessDefined::new())));
