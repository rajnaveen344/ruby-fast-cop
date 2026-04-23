use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Use `include?(element)` instead of `intersect?([element])`.";

#[derive(Default)]
pub struct ArrayIntersectWithSingleElement;

impl ArrayIntersectWithSingleElement {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ArrayIntersectWithSingleElement {
    fn name(&self) -> &'static str {
        "Style/ArrayIntersectWithSingleElement"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "intersect?" {
            return vec![];
        }
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }
        // Argument must be an array with exactly one element
        let array = match arg_list[0].as_array_node() {
            Some(a) => a,
            None => return vec![],
        };
        let elements: Vec<_> = array.elements().iter().collect();
        if elements.len() != 1 {
            return vec![];
        }
        // Offense from `intersect?` selector to end of call
        let sel_start = match node.message_loc() {
            Some(l) => l.start_offset(),
            None => return vec![],
        };
        let call_end = node.location().end_offset();
        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), sel_start, call_end)]
    }
}

crate::register_cop!("Style/ArrayIntersectWithSingleElement", |_cfg| {
    Some(Box::new(ArrayIntersectWithSingleElement::new()))
});
