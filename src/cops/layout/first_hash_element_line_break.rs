//! Layout/FirstHashElementLineBreak
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/first_hash_element_line_break.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_line_breaks as h;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/FirstHashElementLineBreak";
const MSG: &str = "Add a line break before the first element of a multi-line hash.";

#[derive(Default)]
pub struct FirstHashElementLineBreak {
    allow_multiline_final_element: bool,
}

impl FirstHashElementLineBreak {
    pub fn new(allow_multiline_final_element: bool) -> Self {
        Self { allow_multiline_final_element }
    }
}

impl Cop for FirstHashElementLineBreak {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        // HashNode is always braced; KeywordHashNode (implicit) does not call check_hash.
        let node_start = node.location().start_offset();
        let children: Vec<ruby_prism::Node> = node.elements().iter().collect();
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

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_multiline_final_element: bool,
}

crate::register_cop!("Layout/FirstHashElementLineBreak", |cfg| {
    let c: Cfg = cfg.typed("Layout/FirstHashElementLineBreak");
    Some(Box::new(FirstHashElementLineBreak::new(c.allow_multiline_final_element)))
});
