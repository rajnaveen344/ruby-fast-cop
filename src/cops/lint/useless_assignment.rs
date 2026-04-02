use crate::cops::{CheckContext, Cop};
use crate::helpers::variable_force::ScopeAnalyzer;
use crate::offense::{Offense, Severity};

pub struct UselessAssignment;

impl UselessAssignment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for UselessAssignment {
    fn name(&self) -> &'static str {
        "Lint/UselessAssignment"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut analyzer = ScopeAnalyzer::new(ctx);
        analyzer.analyze_program(node);
        analyzer.offenses
    }
}
