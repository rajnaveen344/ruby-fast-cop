//! Layout/MultilineMethodParameterLineBreaks
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_method_parameter_line_breaks.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_line_breaks as h;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/MultilineMethodParameterLineBreaks";
const MSG: &str = "Each parameter in a multi-line method definition must start on a separate line.";

#[derive(Default)]
pub struct MultilineMethodParameterLineBreaks {
    allow_multiline_final_element: bool,
}

impl MultilineMethodParameterLineBreaks {
    pub fn new(allow_multiline_final_element: bool) -> Self {
        Self { allow_multiline_final_element }
    }
}

impl Cop for MultilineMethodParameterLineBreaks {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![],
        };
        let children = collect_params(&params);
        if children.is_empty() {
            return vec![];
        }
        h::check_multiline_breaks(ctx, COP_NAME, MSG, &children, self.allow_multiline_final_element)
    }
}

fn collect_params<'a>(params: &ruby_prism::ParametersNode<'a>) -> Vec<ruby_prism::Node<'a>> {
    let mut all: Vec<ruby_prism::Node<'a>> = Vec::new();
    all.extend(params.requireds().iter());
    all.extend(params.optionals().iter());
    if let Some(rest) = params.rest() { all.push(rest); }
    all.extend(params.posts().iter());
    all.extend(params.keywords().iter());
    if let Some(kw_rest) = params.keyword_rest() { all.push(kw_rest); }
    if let Some(block) = params.block() { all.push(block.as_node()); }
    all.sort_by_key(|n| n.location().start_offset());
    all
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_multiline_final_element: bool,
}

crate::register_cop!("Layout/MultilineMethodParameterLineBreaks", |cfg| {
    let c: Cfg = cfg.typed("Layout/MultilineMethodParameterLineBreaks");
    Some(Box::new(MultilineMethodParameterLineBreaks::new(c.allow_multiline_final_element)))
});
