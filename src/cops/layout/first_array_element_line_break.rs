//! Layout/FirstArrayElementLineBreak
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/first_array_element_line_break.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_line_breaks as h;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/FirstArrayElementLineBreak";
const MSG: &str = "Add a line break before the first element of a multi-line array.";

#[derive(Default)]
pub struct FirstArrayElementLineBreak {
    allow_implicit_array_literals: bool,
    allow_multiline_final_element: bool,
}

impl FirstArrayElementLineBreak {
    pub fn new(allow_implicit_array_literals: bool, allow_multiline_final_element: bool) -> Self {
        Self { allow_implicit_array_literals, allow_multiline_final_element }
    }
}

impl Cop for FirstArrayElementLineBreak {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        let bracketed = node.opening_loc().is_some();
        let node_start = node.location().start_offset();

        // RuboCop: return if !node.loc.begin && !assignment_on_same_line?(node)
        if !bracketed && !h::assignment_on_same_line(ctx.source, node_start) {
            return vec![];
        }
        // RuboCop: return if allow_implicit_array_brackets? && !node.bracketed?
        if self.allow_implicit_array_literals && !bracketed {
            return vec![];
        }

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
    allow_implicit_array_literals: bool,
    allow_multiline_final_element: bool,
}

crate::register_cop!("Layout/FirstArrayElementLineBreak", |cfg| {
    let c: Cfg = cfg.typed("Layout/FirstArrayElementLineBreak");
    Some(Box::new(FirstArrayElementLineBreak::new(
        c.allow_implicit_array_literals,
        c.allow_multiline_final_element,
    )))
});
