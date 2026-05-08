//! Style/KeywordParametersOrder cop
//!
//! Required keyword params must come before optional keyword params.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, ParametersNode};

#[derive(Default)]
pub struct KeywordParametersOrder;

impl KeywordParametersOrder {
    pub fn new() -> Self {
        Self
    }

    fn check_params(&self, params: ParametersNode, ctx: &CheckContext) -> Vec<Offense> {
        let keywords: Vec<Node> = params.keywords().iter().collect();
        let mut offenses = Vec::new();

        // Collect offending optional kwargs (those with required kwargs after them)
        let mut first_offending_idx: Option<usize> = None;
        for (i, kw) in keywords.iter().enumerate() {
            if kw.as_optional_keyword_parameter_node().is_none() {
                continue;
            }
            let has_required_after = keywords[i + 1..].iter().any(|k| {
                k.as_required_keyword_parameter_node().is_some()
            });
            if !has_required_after {
                continue;
            }
            if first_offending_idx.is_none() {
                first_offending_idx = Some(i);
            }
            let loc = kw.location();
            let msg = "Place optional keyword parameters at the end of the parameters list.";
            offenses.push(ctx.offense(self.name(), msg, self.severity(), &loc));
        }

        // Attach correction to first offense only (sort-block strategy)
        if let (Some(first_idx), Some(first_offense)) = (first_offending_idx, offenses.first_mut()) {
            // Check for comments in the keyword range
            let kw_start = keywords[0].location().start_offset();
            let kw_end = keywords.last().unwrap().location().end_offset();
            let has_comment = ctx.source[kw_start..kw_end].contains('#');
            if !has_comment {
                // Build sorted list: required first (preserve order), then optional (preserve order)
                let mut required: Vec<&Node> = Vec::new();
                let mut optional: Vec<&Node> = Vec::new();
                for kw in &keywords {
                    if kw.as_required_keyword_parameter_node().is_some() {
                        required.push(kw);
                    } else {
                        optional.push(kw);
                    }
                }
                let sorted: Vec<&Node> = required.into_iter().chain(optional).collect();

                // Replace each keyword node's range with sorted keyword's source
                let edits: Vec<Edit> = keywords.iter().zip(sorted.iter()).map(|(orig, sorted_kw)| {
                    let s = orig.location().start_offset();
                    let e = orig.location().end_offset();
                    let replacement = ctx.source[sorted_kw.location().start_offset()..sorted_kw.location().end_offset()].to_string();
                    Edit { start_offset: s, end_offset: e, replacement }
                }).collect();

                *first_offense = first_offense.clone().with_correction(Correction { edits });
            }
            let _ = first_idx;
        }

        offenses
    }
}

impl Cop for KeywordParametersOrder {
    fn name(&self) -> &'static str {
        "Style/KeywordParametersOrder"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        if let Some(params) = node.parameters() {
            return self.check_params(params, ctx);
        }
        vec![]
    }

    fn check_block(&self, node: &ruby_prism::BlockNode, ctx: &CheckContext) -> Vec<Offense> {
        if let Some(params_node) = node.parameters() {
            if let Some(bp) = params_node.as_block_parameters_node() {
                if let Some(inner_params) = bp.parameters() {
                    return self.check_params(inner_params, ctx);
                }
            }
        }
        vec![]
    }
}

crate::register_cop!("Style/KeywordParametersOrder", |_cfg| Some(Box::new(KeywordParametersOrder::new())));
