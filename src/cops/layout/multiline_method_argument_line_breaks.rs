//! Layout/MultilineMethodArgumentLineBreaks
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_method_argument_line_breaks.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_line_breaks as h;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Layout/MultilineMethodArgumentLineBreaks";
const MSG: &str = "Each argument in a multi-line method call must start on a separate line.";

#[derive(Default)]
pub struct MultilineMethodArgumentLineBreaks {
    allow_multiline_final_element: bool,
}

impl MultilineMethodArgumentLineBreaks {
    pub fn new(allow_multiline_final_element: bool) -> Self {
        Self { allow_multiline_final_element }
    }

    fn check_args<'a>(&self, ctx: &CheckContext, args: Vec<Node<'a>>) -> Vec<Offense> {
        // Expand trailing implicit hash (KeywordHashNode) into assoc children.
        let mut expanded: Vec<Node<'a>> = args;
        if let Some(last) = expanded.last() {
            if let Node::KeywordHashNode { .. } = last {
                let kh = last.as_keyword_hash_node().unwrap();
                let pairs: Vec<Node> = kh.elements().iter().collect();
                expanded.pop();
                expanded.extend(pairs);
            }
        }
        h::check_multiline_breaks(ctx, COP_NAME, MSG, &expanded, self.allow_multiline_final_element)
    }
}

impl Cop for MultilineMethodArgumentLineBreaks {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip `[]=` per RuboCop.
        let method = node_name!(node);
        if method == "[]=" {
            return vec![];
        }
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        self.check_args(ctx, arg_list)
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_multiline_final_element: bool,
}

crate::register_cop!("Layout/MultilineMethodArgumentLineBreaks", |cfg| {
    let c: Cfg = cfg.typed("Layout/MultilineMethodArgumentLineBreaks");
    Some(Box::new(MultilineMethodArgumentLineBreaks::new(c.allow_multiline_final_element)))
});
