//! Lint/UnderscorePrefixedVariableName - Checks for underscore-prefixed variables that are actually used.

use crate::cops::{CheckContext, Cop};
use crate::helpers::variable_force::{Scope, Variable, VariableForceDispatcher, VariableForceHook};
use crate::offense::{Offense, Severity};

const MSG: &str = "Do not use prefix `_` for a variable that is used.";

pub struct UnderscorePrefixedVariableName {
    allow_keyword_block_arguments: bool,
}

impl UnderscorePrefixedVariableName {
    pub fn new(allow_keyword_block_arguments: bool) -> Self {
        Self { allow_keyword_block_arguments }
    }
}

impl Default for UnderscorePrefixedVariableName {
    fn default() -> Self { Self::new(false) }
}

impl Cop for UnderscorePrefixedVariableName {
    fn name(&self) -> &'static str { "Lint/UnderscorePrefixedVariableName" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut hook = Hook {
            ctx,
            offenses: Vec::new(),
            allow_keyword_block_arguments: self.allow_keyword_block_arguments,
        };
        let mut dispatcher = VariableForceDispatcher::new(&mut hook, ctx.source);
        dispatcher.investigate(node);
        hook.offenses
    }
}

struct Hook<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    allow_keyword_block_arguments: bool,
}

impl<'a> Hook<'a> {
    fn check_variable(&mut self, variable: &Variable) {
        // Must start with _
        if !variable.should_be_unused() {
            return;
        }

        // Must have explicit references (not just via super/binding)
        if variable.explicit_reference_count == 0 {
            return;
        }

        // Skip if it's a keyword block argument and AllowKeywordBlockArguments is set
        if self.allow_keyword_block_arguments
            && variable.is_block_argument()
            && variable.is_keyword_argument
        {
            return;
        }

        // The offense is at the declaration location
        let start = variable.declaration_start;
        let end = variable.declaration_end;

        if start >= end {
            return; // no location info
        }

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/UnderscorePrefixedVariableName",
            MSG,
            Severity::Warning,
            start,
            end,
        ));
    }
}

impl<'a> VariableForceHook for Hook<'a> {
    fn after_leaving_scope(&mut self, scope: &Scope, _source: &str) {
        for variable in scope.variables.values() {
            self.check_variable(variable);
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_keyword_block_arguments: bool,
}

impl Default for Cfg {
    fn default() -> Self { Self { allow_keyword_block_arguments: false } }
}

crate::register_cop!("Lint/UnderscorePrefixedVariableName", |cfg| {
    let c: Cfg = cfg.typed("Lint/UnderscorePrefixedVariableName");
    Some(Box::new(UnderscorePrefixedVariableName::new(c.allow_keyword_block_arguments)))
});
