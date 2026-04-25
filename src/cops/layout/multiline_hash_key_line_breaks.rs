//! Layout/MultilineHashKeyLineBreaks
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_hash_key_line_breaks.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_line_breaks as h;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/MultilineHashKeyLineBreaks";
const MSG: &str = "Each key in a multi-line hash must start on a separate line.";

#[derive(Default)]
pub struct MultilineHashKeyLineBreaks {
    allow_multiline_final_element: bool,
}

impl MultilineHashKeyLineBreaks {
    pub fn new(allow_multiline_final_element: bool) -> Self {
        Self { allow_multiline_final_element }
    }
}

impl Cop for MultilineHashKeyLineBreaks {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only braced hashes; KeywordHashNode (implicit kwargs) is a different node type
        // and does not trigger check_hash. HashNode in Prism always has explicit braces.
        let children: Vec<ruby_prism::Node> = node.elements().iter().collect();
        h::check_multiline_breaks(ctx, COP_NAME, MSG, &children, self.allow_multiline_final_element)
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_multiline_final_element: bool,
}

crate::register_cop!("Layout/MultilineHashKeyLineBreaks", |cfg| {
    let c: Cfg = cfg.typed("Layout/MultilineHashKeyLineBreaks");
    Some(Box::new(MultilineHashKeyLineBreaks::new(c.allow_multiline_final_element)))
});
