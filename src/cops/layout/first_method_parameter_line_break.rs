//! Layout/FirstMethodParameterLineBreak
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/first_method_parameter_line_break.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_line_breaks as h;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/FirstMethodParameterLineBreak";
const MSG: &str = "Add a line break before the first parameter of a multi-line method parameter list.";

#[derive(Default)]
pub struct FirstMethodParameterLineBreak {
    allow_multiline_final_element: bool,
}

impl FirstMethodParameterLineBreak {
    pub fn new(allow_multiline_final_element: bool) -> Self {
        Self { allow_multiline_final_element }
    }
}

impl Cop for FirstMethodParameterLineBreak {
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
        let node_start = node.location().start_offset();
        let first_start = children[0].location().start_offset();
        // method_uses_parens: line up to first child must end in `(` (whitespace-tolerant)
        if !h::method_uses_parens(ctx.source, node_start, first_start) {
            return vec![];
        }
        h::check_first_element_break(
            ctx,
            COP_NAME,
            MSG,
            node_start,
            &children,
            self.allow_multiline_final_element,
        )
    }
}

/// Collect all parameter nodes (requireds/optionals/rest/posts/keywords/keyword_rest/block)
/// in source order.
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

crate::register_cop!("Layout/FirstMethodParameterLineBreak", |cfg| {
    let c: Cfg = cfg.typed("Layout/FirstMethodParameterLineBreak");
    Some(Box::new(FirstMethodParameterLineBreak::new(c.allow_multiline_final_element)))
});
