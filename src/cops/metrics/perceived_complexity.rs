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

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct PerceivedCfg {
    max: usize,
    allowed_methods: Vec<String>,
    ignored_methods: Vec<String>,
    excluded_methods: Vec<String>,
    allowed_patterns: Vec<String>,
    ignored_patterns: Vec<String>,
}

impl Default for PerceivedCfg {
    fn default() -> Self {
        Self {
            max: 8,
            allowed_methods: Vec::new(),
            ignored_methods: Vec::new(),
            excluded_methods: Vec::new(),
            allowed_patterns: Vec::new(),
            ignored_patterns: Vec::new(),
        }
    }
}

crate::register_cop!("Metrics/PerceivedComplexity", |cfg| {
    let c: PerceivedCfg = cfg.typed("Metrics/PerceivedComplexity");
    let mut allowed_methods = c.allowed_methods;
    allowed_methods.extend(c.ignored_methods);
    allowed_methods.extend(c.excluded_methods);
    let mut allowed_patterns = c.allowed_patterns;
    allowed_patterns.extend(c.ignored_patterns);
    Some(Box::new(PerceivedComplexity::with_config(c.max, allowed_methods, allowed_patterns)))
});
