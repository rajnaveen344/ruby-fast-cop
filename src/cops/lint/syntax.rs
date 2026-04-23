//! Lint/Syntax - Reports parse errors from Prism as offenses.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/syntax.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

#[derive(Default)]
pub struct Syntax;

impl Syntax {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for Syntax {
    fn name(&self) -> &'static str {
        "Lint/Syntax"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Prism is error-tolerant; parse errors are surfaced via parse_result.errors().
        // We receive a ProgramNode even on parse errors, so we can't access errors here
        // without re-parsing. For now this is a stub — Prism recovers from most errors
        // and the parsed tree is still usable. If the runner surfaces parse diagnostics,
        // they would be emitted here.
        //
        // No test cases in TOML; this cop is registered so it appears in cop lists.
        let _ = ctx;
        vec![]
    }
}

crate::register_cop!("Lint/Syntax", |_cfg| {
    Some(Box::new(Syntax::new()))
});
