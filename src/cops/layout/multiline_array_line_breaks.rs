//! Layout/MultilineArrayLineBreaks
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_array_line_breaks.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_line_breaks as h;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/MultilineArrayLineBreaks";
const MSG: &str = "Each item in a multi-line array must start on a separate line.";

#[derive(Default)]
pub struct MultilineArrayLineBreaks {
    allow_multiline_final_element: bool,
}

impl MultilineArrayLineBreaks {
    pub fn new(allow_multiline_final_element: bool) -> Self {
        Self { allow_multiline_final_element }
    }
}

impl Cop for MultilineArrayLineBreaks {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        let children: Vec<ruby_prism::Node> = node.elements().iter().collect();
        h::check_multiline_breaks(ctx, COP_NAME, MSG, &children, self.allow_multiline_final_element)
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_multiline_final_element: bool,
}

crate::register_cop!("Layout/MultilineArrayLineBreaks", |cfg| {
    let c: Cfg = cfg.typed("Layout/MultilineArrayLineBreaks");
    Some(Box::new(MultilineArrayLineBreaks::new(c.allow_multiline_final_element)))
});
