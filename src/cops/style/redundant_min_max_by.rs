//! Style/RedundantMinMaxBy cop
//!
//! Identifies `max_by { _1 }` / `min_by { |x| x }` / etc. that can be replaced
//! with `max` / `min` / `minmax`.
//!
//! Ported from `lib/rubocop/cop/style/redundant_min_max_by.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct RedundantMinMaxBy;

impl RedundantMinMaxBy {
    pub fn new() -> Self {
        Self
    }

    fn replacement(method: &str) -> Option<&'static str> {
        match method {
            "max_by" => Some("max"),
            "min_by" => Some("min"),
            "minmax_by" => Some("minmax"),
            _ => None,
        }
    }
}

impl Cop for RedundantMinMaxBy {
    fn name(&self) -> &'static str {
        "Style/RedundantMinMaxBy"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let Some(replacement) = Self::replacement(method.as_ref()) else {
            return vec![];
        };

        let Some(block) = node.block() else {
            return vec![];
        };
        let Some(block_node) = block.as_block_node() else {
            return vec![];
        };

        // Pattern: block has single-statement body that's just the parameter reference.
        // Extract param info + body.
        let params = block_node.parameters();
        let body = block_node.body();

        let (param_display, source_var_name): (String, Option<String>) = match params {
            Some(p) => match &p {
                Node::BlockParametersNode { .. } => {
                    let bp = p.as_block_parameters_node().unwrap();
                    let Some(inner) = bp.parameters() else {
                        return vec![];
                    };
                    if inner.optionals().iter().count() > 0
                        || inner.rest().is_some()
                        || inner.keywords().iter().count() > 0
                        || inner.block().is_some()
                        || inner.posts().iter().count() > 0
                    {
                        return vec![];
                    }
                    let requireds: Vec<_> = inner.requireds().iter().collect();
                    if requireds.len() != 1 {
                        return vec![];
                    }
                    match &requireds[0] {
                        Node::RequiredParameterNode { .. } => {
                            let rpn = requireds[0].as_required_parameter_node().unwrap();
                            let name = node_name!(rpn).to_string();
                            (format!("|{}| {}", name, name), Some(name))
                        }
                        _ => return vec![],
                    }
                }
                Node::NumberedParametersNode { .. } => {
                    let np = p.as_numbered_parameters_node().unwrap();
                    if np.maximum() != 1 {
                        return vec![];
                    }
                    ("_1".to_string(), Some("_1".to_string()))
                }
                Node::ItParametersNode { .. } => {
                    ("it".to_string(), Some("it".to_string()))
                }
                _ => return vec![],
            },
            None => return vec![],
        };

        // Body must be a single statement reading that variable
        let Some(body_node) = body else {
            return vec![];
        };
        let stmts = match body_node.as_statements_node() {
            Some(s) => s,
            None => return vec![],
        };
        let items: Vec<_> = stmts.body().iter().collect();
        if items.len() != 1 {
            return vec![];
        }
        let expected = source_var_name.as_deref().unwrap();
        let ok = match &items[0] {
            Node::LocalVariableReadNode { .. } => {
                let lv = items[0].as_local_variable_read_node().unwrap();
                node_name!(lv) == expected
            }
            Node::ItLocalVariableReadNode { .. } => expected == "it",
            _ => false,
        };
        if !ok {
            return vec![];
        }

        // Offense range: from selector start to block end
        let selector_start = match node.message_loc() {
            Some(l) => l.start_offset(),
            None => return vec![],
        };
        let block_end = block.location().end_offset();

        let block_src_for_msg = format!("{} {{ {} }}", method.as_ref(), param_display);
        let msg = format!(
            "Use `{}` instead of `{}`.",
            replacement, block_src_for_msg
        );

        let offense = ctx
            .offense_with_range(self.name(), &msg, self.severity(), selector_start, block_end)
            .with_correction(Correction::replace(
                selector_start,
                block_end,
                replacement,
            ));
        vec![offense]
    }
}

crate::register_cop!("Style/RedundantMinMaxBy", |_cfg| Some(Box::new(RedundantMinMaxBy::new())));
