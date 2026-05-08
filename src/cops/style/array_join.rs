use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Favor `Array#join` over `Array#*`.";

#[derive(Default)]
pub struct ArrayJoin;

impl ArrayJoin {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ArrayJoin {
    fn name(&self) -> &'static str {
        "Style/ArrayJoin"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "*" {
            return vec![];
        }
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        // Receiver must be an array literal
        if !matches!(&receiver, Node::ArrayNode { .. }) {
            return vec![];
        }
        // Argument must be a string literal
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }
        if !matches!(&arg_list[0], Node::StringNode { .. }) {
            return vec![];
        }
        // Offense at the `*` selector
        let sel = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        // Get arg source for the join call
        let arg_src = {
            let arg_loc = arg_list[0].location();
            ctx.src(arg_loc.start_offset(), arg_loc.end_offset()).to_string()
        };
        // Correction: replace (optional_space)*arg → .join(arg)
        // Eat leading whitespace before `*`
        let bytes = ctx.source.as_bytes();
        let mut repl_start = sel.start_offset();
        while repl_start > 0 && bytes[repl_start - 1] == b' ' {
            repl_start -= 1;
        }
        let node_end = node.location().end_offset();
        let correction = Correction::replace(repl_start, node_end, format!(".join({})", arg_src));
        vec![ctx.offense(self.name(), MSG, self.severity(), &sel)
            .with_correction(correction)]
    }
}

crate::register_cop!("Style/ArrayJoin", |_cfg| Some(Box::new(ArrayJoin::new())));
