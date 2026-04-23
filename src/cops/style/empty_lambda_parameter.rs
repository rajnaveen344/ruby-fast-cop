use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str = "Omit parentheses for the empty lambda parameters.";

#[derive(Default)]
pub struct EmptyLambdaParameter;

impl EmptyLambdaParameter {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EmptyLambdaParameter {
    fn name(&self) -> &'static str {
        "Style/EmptyLambdaParameter"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_lambda(&self, node: &ruby_prism::LambdaNode, ctx: &CheckContext) -> Vec<Offense> {
        // LambdaNode: check if it has parameters that are empty (just parentheses)
        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![], // no parens at all — good
        };
        // Check if block parameters are empty
        // BlockParametersNode: check if it has no parameters
        let block_params = match params.as_block_parameters_node() {
            Some(bp) => bp,
            None => return vec![],
        };

        // Check all parameter lists are empty
        let has_params = block_params.parameters().map_or(false, |p| {
            !p.requireds().is_empty()
                || !p.optionals().is_empty()
                || !p.posts().is_empty()
                || p.rest().is_some()
                || !p.keywords().is_empty()
                || p.keyword_rest().is_some()
                || p.block().is_some()
        });

        if has_params {
            return vec![];
        }

        // Empty params with parens — flag the parens range
        let params_loc = params.location();
        vec![ctx.offense(self.name(), MSG, self.severity(), &params_loc)]
    }
}

crate::register_cop!("Style/EmptyLambdaParameter", |_cfg| {
    Some(Box::new(EmptyLambdaParameter::new()))
});
