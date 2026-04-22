//! Style/RedundantSortBy cop
//!
//! Detects `sort_by { |x| x }`, `sort_by { _1 }`, `sort_by { it }` → use `sort`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{BlockNode, CallNode};

#[derive(Default)]
pub struct RedundantSortBy;

impl RedundantSortBy {
    pub fn new() -> Self {
        Self
    }

    /// Returns Some(description) if block is identity block, else None.
    fn redundant_block_pattern(block: &BlockNode) -> Option<String> {
        let body = block.body()?;
        let stmts = body.as_statements_node()?;
        let stmts_list = stmts.body();
        let mut iter = stmts_list.iter();
        let first = iter.next()?;
        if iter.next().is_some() { return None; } // more than 1 statement

        let params = block.parameters();

        // Numbered params: `sort_by { _1 }` — block has NumberedParametersNode
        if let Some(p) = params {
            if p.as_numbered_parameters_node().is_some() {
                if let Some(lv) = first.as_local_variable_read_node() {
                    let name = node_name!(lv);
                    if name == "_1" {
                        return Some("sort_by { _1 }".to_string());
                    }
                }
                return None;
            }

            // it-block (Ruby 3.4): ItParametersNode
            if p.as_it_parameters_node().is_some() {
                if first.as_it_local_variable_read_node().is_some() {
                    return Some("sort_by { it }".to_string());
                }
                return None;
            }

            // Pattern 1: `sort_by { |x| x }` — BlockParametersNode
            if let Some(bp) = p.as_block_parameters_node() {
                let inner = bp.parameters()?;

                let reqs: Vec<_> = inner.requireds().iter().collect();
                if reqs.len() != 1 { return None; }
                if inner.optionals().len() > 0 { return None; }
                if inner.rest().is_some() { return None; }
                if inner.keywords().len() > 0 { return None; }
                if inner.posts().len() > 0 { return None; }

                let param_name = {
                    let req = &reqs[0];
                    let lp = req.as_required_parameter_node()?;
                    node_name!(lp).to_string()
                };

                let body_name = {
                    let lv = first.as_local_variable_read_node()?;
                    node_name!(lv).to_string()
                };

                if param_name == body_name {
                    return Some(format!("sort_by {{ |{}| {} }}", param_name, body_name));
                }
            }

            return None;
        }

        // No params at all
        None
    }
}

impl Cop for RedundantSortBy {
    fn name(&self) -> &'static str {
        "Style/RedundantSortBy"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "sort_by" {
            return vec![];
        }

        // Must have a block
        let block = match node.block() {
            Some(b) => match b.as_block_node() {
                Some(bn) => bn,
                None => return vec![],
            },
            None => return vec![],
        };

        let block_repr = match Self::redundant_block_pattern(&block) {
            Some(s) => s,
            None => return vec![],
        };

        let msg = format!("Use `sort` instead of `{}`.", block_repr);

        // Range: from sort_by to end of block
        let start = node.message_loc().unwrap_or_else(|| node.location()).start_offset();
        let end = block.location().end_offset();

        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)]
    }
}

crate::register_cop!("Style/RedundantSortBy", |_cfg| Some(Box::new(RedundantSortBy::new())));
