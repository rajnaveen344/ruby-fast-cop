//! Lint/ConstantResolution - require fully qualified constant references.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/lint/constant_resolution.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Fully qualify this constant to avoid possibly ambiguous resolution.";
const COP: &str = "Lint/ConstantResolution";

pub struct ConstantResolution {
    only: Vec<String>,
    ignore: Vec<String>,
}

impl ConstantResolution {
    pub fn new(only: Vec<String>, ignore: Vec<String>) -> Self {
        Self { only, ignore }
    }
}

impl Cop for ConstantResolution {
    fn name(&self) -> &'static str { COP }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V {
            ctx,
            only: &self.only,
            ignore: &self.ignore,
            skip_ranges: Vec::new(),
            out: Vec::new(),
        };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    only: &'a [String],
    ignore: &'a [String],
    skip_ranges: Vec<(usize, usize)>,
    out: Vec<Offense>,
}

impl<'a, 'b> V<'a, 'b> {
    fn in_skip_range(&self, start: usize, end: usize) -> bool {
        self.skip_ranges.iter().any(|(s, e)| *s <= start && end <= *e)
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let cp = node.constant_path();
        let loc = cp.location();
        self.skip_ranges.push((loc.start_offset(), loc.end_offset()));
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let cp = node.constant_path();
        let loc = cp.location();
        self.skip_ranges.push((loc.start_offset(), loc.end_offset()));
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode) {
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        if self.in_skip_range(start, end) {
            return;
        }
        let name_bytes = node.name().as_slice();
        let name = std::str::from_utf8(name_bytes).unwrap_or("");

        if !self.only.is_empty() && !self.only.iter().any(|s| s == name) {
            return;
        }
        if self.ignore.iter().any(|s| s == name) {
            return;
        }

        self.out.push(self.ctx.offense_with_range(
            COP, MSG, Severity::Warning, start, end,
        ));
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    only: Option<Vec<String>>,
    ignore: Option<Vec<String>>,
}

crate::register_cop!("Lint/ConstantResolution", |cfg| {
    let c: Cfg = cfg.typed("Lint/ConstantResolution");
    Some(Box::new(ConstantResolution::new(
        c.only.unwrap_or_default(),
        c.ignore.unwrap_or_default(),
    )))
});
