//! Style/StabbyLambdaParentheses cop
//!
//! Checks for parentheses around stabby lambda arguments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{LambdaNode, ProgramNode, Visit};

pub struct StabbyLambdaParentheses {
    require_parens: bool, // true = require_parentheses (default), false = require_no_parentheses
}

impl Default for StabbyLambdaParentheses {
    fn default() -> Self {
        Self { require_parens: true }
    }
}

impl StabbyLambdaParentheses {
    pub fn new(require_parens: bool) -> Self {
        Self { require_parens }
    }

    fn check_lambda_node(&self, node: &LambdaNode, ctx: &CheckContext) -> Option<Offense> {
        // Must have parameters
        let params_node = node.parameters()?;
        let bp = params_node.as_block_parameters_node()?;

        // Must have at least one parameter (inner ParametersNode present)
        let has_params = bp.parameters().is_some();
        if !has_params {
            return None;
        }

        // Detect if parenthesized: opening_loc on BlockParametersNode
        let has_parens = bp.opening_loc().is_some();

        let bp_loc = bp.location();

        if self.require_parens && !has_parens {
            Some(ctx.offense(
                "Style/StabbyLambdaParentheses",
                "Wrap stabby lambda arguments with parentheses.",
                Severity::Convention,
                &bp_loc,
            ))
        } else if !self.require_parens && has_parens {
            Some(ctx.offense(
                "Style/StabbyLambdaParentheses",
                "Do not wrap stabby lambda arguments with parentheses.",
                Severity::Convention,
                &bp_loc,
            ))
        } else {
            None
        }
    }
}

impl Cop for StabbyLambdaParentheses {
    fn name(&self) -> &'static str {
        "Style/StabbyLambdaParentheses"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = LambdaVisitor {
            cop: self,
            ctx,
            offenses: &mut offenses,
        };
        visitor.visit(&result.node());
        offenses
    }
}

struct LambdaVisitor<'a, 'b> {
    cop: &'a StabbyLambdaParentheses,
    ctx: &'b CheckContext<'b>,
    offenses: &'b mut Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for LambdaVisitor<'a, 'b> {
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        if let Some(offense) = self.cop.check_lambda_node(node, self.ctx) {
            self.offenses.push(offense);
        }
        ruby_prism::visit_lambda_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

crate::register_cop!("Style/StabbyLambdaParentheses", |cfg| {
    let c: Cfg = cfg.typed("Style/StabbyLambdaParentheses");
    let require_parens = c.enforced_style != "require_no_parentheses";
    Some(Box::new(StabbyLambdaParentheses::new(require_parens)))
});
