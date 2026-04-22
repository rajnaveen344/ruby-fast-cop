//! Style/SingleLineMethods cop
//!
//! Checks for single-line method definitions that contain a body.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::DefNode;

pub struct SingleLineMethods {
    allow_if_method_is_empty: bool,
}

impl Default for SingleLineMethods {
    fn default() -> Self {
        Self { allow_if_method_is_empty: true }
    }
}

impl SingleLineMethods {
    pub fn new(allow_if_method_is_empty: bool) -> Self {
        Self { allow_if_method_is_empty }
    }

    fn is_single_line(node: &DefNode, source: &str) -> bool {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        !source[start..end].contains('\n')
    }

    fn is_endless(node: &DefNode) -> bool {
        // Endless method: def foo() = expr — has equals sign, no end keyword
        node.equal_loc().is_some()
    }
}

impl Cop for SingleLineMethods {
    fn name(&self) -> &'static str {
        "Style/SingleLineMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &DefNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_single_line(node, ctx.source) {
            return vec![];
        }
        if Self::is_endless(node) {
            return vec![];
        }
        // Check if body is empty
        let has_body = node.body().is_some();
        if !has_body && self.allow_if_method_is_empty {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        vec![ctx.offense_with_range(
            self.name(),
            "Avoid single-line method definitions.",
            self.severity(),
            start,
            end,
        )]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_if_method_is_empty: Option<bool>,
}

crate::register_cop!("Style/SingleLineMethods", |cfg| {
    let c: Cfg = cfg.typed("Style/SingleLineMethods");
    let allow = c.allow_if_method_is_empty.unwrap_or(true);
    Some(Box::new(SingleLineMethods::new(allow)))
});
