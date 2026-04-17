//! Metrics/PerceivedComplexity cop.
//!
//! Ported from https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/metrics/perceived_complexity.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::method_complexity::{check_program, ComplexityKind, MethodComplexityConfig};
use crate::offense::{Offense, Severity};

pub struct PerceivedComplexity {
    max: usize,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl PerceivedComplexity {
    pub fn new(max: usize) -> Self {
        Self { max, allowed_methods: Vec::new(), allowed_patterns: Vec::new() }
    }

    pub fn with_config(max: usize, allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { max, allowed_methods, allowed_patterns }
    }
}

impl Cop for PerceivedComplexity {
    fn name(&self) -> &'static str { "Metrics/PerceivedComplexity" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let cfg = MethodComplexityConfig {
            kind: ComplexityKind::Perceived,
            cop_name: "Metrics/PerceivedComplexity",
            msg_template: "Perceived complexity for `{method}` is too high. [{complexity}/{max}]",
            max: self.max,
            allowed_methods: self.allowed_methods.clone(),
            allowed_patterns: self.allowed_patterns.clone(),
        };
        check_program(ctx, &cfg, &mut offenses);
        offenses
    }
}

crate::register_cop!("Metrics/PerceivedComplexity", |cfg| {
    let cop_config = cfg.get_cop_config("Metrics/PerceivedComplexity");
    let max = cop_config.and_then(|c| c.max).unwrap_or(8);
    let mut allowed_methods: Vec<String> = Vec::new();
    for key in &["AllowedMethods", "IgnoredMethods", "ExcludedMethods"] {
        if let Some(seq) = cop_config.and_then(|c| c.raw.get(*key)).and_then(|v| v.as_sequence()) {
            for v in seq { if let Some(s) = v.as_str() { allowed_methods.push(s.to_string()); } }
        }
    }
    let mut allowed_patterns: Vec<String> = Vec::new();
    for key in &["AllowedPatterns", "IgnoredPatterns"] {
        if let Some(seq) = cop_config.and_then(|c| c.raw.get(*key)).and_then(|v| v.as_sequence()) {
            for v in seq { if let Some(s) = v.as_str() { allowed_patterns.push(s.to_string()); } }
        }
    }
    Some(Box::new(PerceivedComplexity::with_config(max, allowed_methods, allowed_patterns)))
});
