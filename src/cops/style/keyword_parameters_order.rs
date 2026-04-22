//! Style/KeywordParametersOrder cop
//!
//! Required keyword params must come before optional keyword params.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
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

        // For each optional keyword param, check if any required keyword param comes after it
        for (i, kw) in keywords.iter().enumerate() {
            if kw.as_optional_keyword_parameter_node().is_none() {
                continue;
            }
            // Check if any required kwarg follows this optional one
            let has_required_after = keywords[i + 1..].iter().any(|k| {
                k.as_required_keyword_parameter_node().is_some()
            });
            if !has_required_after {
                continue;
            }

            let loc = kw.location();
            let msg = "Place optional keyword parameters at the end of the parameters list.";
            offenses.push(ctx.offense(self.name(), msg, self.severity(), &loc));
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
