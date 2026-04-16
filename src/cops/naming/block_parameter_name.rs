//! Naming/BlockParameterName
//!
//! Checks block parameter names for descriptiveness. Mirrors RuboCop's
//! UncommunicativeName mixin.

use crate::cops::{CheckContext, Cop};
use crate::helpers::uncommunicative_name::{
    check_params, extract_params, UncommunicativeConfig,
};
use crate::offense::{Offense, Severity};

pub struct BlockParameterName {
    config: UncommunicativeConfig,
}

impl BlockParameterName {
    pub fn new() -> Self {
        Self {
            config: UncommunicativeConfig::new(1, true, vec![], vec![]),
        }
    }

    pub fn with_config(
        min_name_length: usize,
        allow_names_ending_in_numbers: bool,
        allowed_names: Vec<String>,
        forbidden_names: Vec<String>,
    ) -> Self {
        Self {
            config: UncommunicativeConfig::new(
                min_name_length,
                allow_names_ending_in_numbers,
                allowed_names,
                forbidden_names,
            ),
        }
    }
}

impl Default for BlockParameterName {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for BlockParameterName {
    fn name(&self) -> &'static str {
        "Naming/BlockParameterName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_block(&self, node: &ruby_prism::BlockNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(block_params_raw) = node.parameters() else {
            return vec![];
        };
        let Some(block_params) = block_params_raw.as_block_parameters_node() else {
            return vec![];
        };
        let Some(params_node) = block_params.parameters() else {
            return vec![];
        };
        let params = extract_params(ctx.source, &params_node);
        check_params(
            &params,
            "block parameter",
            "Naming/BlockParameterName",
            &self.config,
            ctx,
        )
    }
}
