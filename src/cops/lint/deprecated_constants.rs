//! Lint/DeprecatedConstants cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;
use std::collections::HashMap;

#[derive(Default, Clone, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Entry {
    alternative: Option<String>,
    deprecated_version: Option<String>,
}

pub struct DeprecatedConstants {
    map: HashMap<String, Entry>,
}

impl DeprecatedConstants {
    pub fn new(map: HashMap<String, Entry>) -> Self { Self { map } }
}

impl Cop for DeprecatedConstants {
    fn name(&self) -> &'static str { "Lint/DeprecatedConstants" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, map: &self.map, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    map: &'a HashMap<String, Entry>,
    out: Vec<Offense>,
}

fn node_source<'s>(loc_start: usize, loc_end: usize, src: &'s str) -> &'s str {
    &src[loc_start..loc_end]
}

fn lookup_key(source: &str) -> &str {
    source.strip_prefix("::").unwrap_or(source)
}

impl<'a, 'b> V<'a, 'b> {
    fn check(&mut self, start: usize, end: usize) {
        let src = node_source(start, end, self.ctx.source);
        let key = lookup_key(src);
        let Some(entry) = self.map.get(key) else { return };

        if let Some(ver) = &entry.deprecated_version {
            if let Ok(v) = ver.parse::<f64>() {
                if self.ctx.target_ruby_version < v { return; }
            }
        }

        let msg = match (&entry.alternative, &entry.deprecated_version) {
            (Some(alt), Some(ver)) => format!("Use `{}` instead of `{}`, deprecated since Ruby {}.", alt, src, ver),
            (Some(alt), None) => format!("Use `{}` instead of `{}`.", alt, src),
            (None, Some(ver)) => format!("Do not use `{}`, deprecated since Ruby {}.", src, ver),
            (None, None) => format!("Do not use `{}`.", src),
        };

        let mut off = self.ctx.offense_with_range(
            "Lint/DeprecatedConstants", &msg, Severity::Warning, start, end,
        );
        if let Some(alt) = &entry.alternative {
            off = off.with_correction(Correction::replace(start, end, alt));
        }
        self.out.push(off);
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode) {
        let loc = node.location();
        self.check(loc.start_offset(), loc.end_offset());
    }

    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode) {
        let loc = node.location();
        self.check(loc.start_offset(), loc.end_offset());
        // Do not recurse: nested ConstantPathNode/ConstantReadNode children represent
        // sub-paths of the same constant expression and should not be re-checked.
    }
}

crate::register_cop!("Lint/DeprecatedConstants", |cfg| {
    #[derive(Default, serde::Deserialize)]
    #[serde(default, rename_all = "PascalCase")]
    struct Cfg {
        deprecated_constants: HashMap<String, Entry>,
    }
    let c: Cfg = cfg.typed("Lint/DeprecatedConstants");
    Some(Box::new(DeprecatedConstants::new(c.deprecated_constants)))
});
