//! Naming/AccessorMethodName cop
//!
//! Checks that methods named `get_*` (with no args) or `set_*` (with exactly
//! one required positional arg) are renamed: use `foo` / `foo=` instead.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/naming/accessor_method_name.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct AccessorMethodName;

impl AccessorMethodName {
    pub fn new() -> Self {
        Self
    }

    fn check_def(
        &self,
        node: &ruby_prism::DefNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let raw_name = node_name!(node).to_string();
        let name_loc = node.name_loc();

        // Only plain identifiers (no !, ?, =)
        if raw_name.ends_with('!') || raw_name.ends_with('?') || raw_name.ends_with('=') {
            return vec![];
        }

        let params = node.parameters();

        if raw_name.starts_with("get_") {
            // get_* with no required args (and no parameters at all) → offense
            if params.is_none() || Self::all_params_empty(&params) {
                return vec![ctx.offense_with_range(
                    "Naming/AccessorMethodName",
                    "Do not prefix reader method names with `get_`.",
                    Severity::Convention,
                    name_loc.start_offset(),
                    name_loc.end_offset(),
                )];
            }
        }

        if raw_name.starts_with("set_") {
            // set_* with exactly one required positional arg → offense
            if Self::exactly_one_required(&params) {
                return vec![ctx.offense_with_range(
                    "Naming/AccessorMethodName",
                    "Do not prefix writer method names with `set_`.",
                    Severity::Convention,
                    name_loc.start_offset(),
                    name_loc.end_offset(),
                )];
            }
        }

        vec![]
    }

    fn all_params_empty(params: &Option<ruby_prism::ParametersNode>) -> bool {
        let p = match params {
            Some(p) => p,
            None => return true,
        };
        p.requireds().is_empty()
            && p.optionals().is_empty()
            && p.rest().is_none()
            && p.posts().is_empty()
            && p.keywords().is_empty()
            && p.keyword_rest().is_none()
            && p.block().is_none()
    }

    fn exactly_one_required(params: &Option<ruby_prism::ParametersNode>) -> bool {
        let p = match params {
            Some(p) => p,
            None => return false,
        };
        // Exactly one required param, no optional/rest/keywords/etc.
        let requireds: Vec<_> = p.requireds().iter().collect();
        if requireds.len() != 1 {
            return false;
        }
        // Must be a simple required positional (not a destructured/multi target)
        match &requireds[0] {
            Node::RequiredParameterNode { .. } => {}
            Node::MultiTargetNode { .. } => return false,
            _ => return false,
        }
        p.optionals().is_empty()
            && p.rest().is_none()
            && p.posts().is_empty()
            && p.keywords().is_empty()
            && p.keyword_rest().is_none()
            && p.block().is_none()
    }
}

impl Cop for AccessorMethodName {
    fn name(&self) -> &'static str {
        "Naming/AccessorMethodName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_def(node, ctx)
    }
}

crate::register_cop!("Naming/AccessorMethodName", |_cfg| {
    Some(Box::new(AccessorMethodName::new()))
});
