//! Naming/MethodParameterName
//!
//! Checks method parameter names for descriptiveness (length, case, digits,
//! and a forbidden/allowed list). Mirrors RuboCop's UncommunicativeName mixin.

use crate::cops::{CheckContext, Cop};
use crate::helpers::uncommunicative_name::{
    check_params, extract_params, UncommunicativeConfig,
};
use crate::offense::{Offense, Severity};

pub struct MethodParameterName {
    config: UncommunicativeConfig,
}

impl MethodParameterName {
    pub fn new() -> Self {
        Self {
            config: UncommunicativeConfig::new(
                3,
                true,
                default_allowed(),
                vec![],
            ),
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

impl Default for MethodParameterName {
    fn default() -> Self {
        Self::new()
    }
}

fn default_allowed() -> Vec<String> {
    ["as", "at", "by", "cc", "db", "id", "if", "in", "io", "ip", "of", "on", "os", "pp", "to"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

impl Cop for MethodParameterName {
    fn name(&self) -> &'static str {
        "Naming/MethodParameterName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(params_node) = node.parameters() else {
            return vec![];
        };
        let params = extract_params(ctx.source, &params_node);
        check_params(
            &params,
            "method parameter",
            "Naming/MethodParameterName",
            &self.config,
            ctx,
        )
    }
}
