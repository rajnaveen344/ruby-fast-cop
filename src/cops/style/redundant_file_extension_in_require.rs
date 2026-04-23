use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Redundant `.rb` file extension detected.";

#[derive(Default)]
pub struct RedundantFileExtensionInRequire;

impl RedundantFileExtensionInRequire {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantFileExtensionInRequire {
    fn name(&self) -> &'static str {
        "Style/RedundantFileExtensionInRequire"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "require" && method != "require_relative" {
            return vec![];
        }
        // Must have no receiver (top-level call)
        if node.receiver().is_some() {
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
        // Argument must be a string literal
        let arg = &arg_list[0];
        if !matches!(arg, Node::StringNode { .. }) {
            return vec![];
        }
        let str_node = arg.as_string_node().unwrap();
        let content = str_node.unescaped();
        if !content.ends_with(b".rb") {
            return vec![];
        }
        // Offense is on the `.rb` part within the string
        // String value offset: string starts at arg.location().start_offset()
        // The `.rb` is at the end of the string content, before the closing quote
        let str_end = arg.location().end_offset();
        // `.rb` = 3 chars, plus the closing quote = 4
        let rb_end = str_end - 1; // before closing quote
        let rb_start = rb_end - 3; // `.rb`
        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), rb_start, rb_end)]
    }
}

crate::register_cop!("Style/RedundantFileExtensionInRequire", |_cfg| {
    Some(Box::new(RedundantFileExtensionInRequire::new()))
});
