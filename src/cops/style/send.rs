//! Style/Send - Prefer `Object#__send__` or `Object#public_send` to `send`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/send.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct Send;

impl Send {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for Send {
    fn name(&self) -> &'static str {
        "Style/Send"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "send" {
            return vec![];
        }
        // Need at least one argument (`Object.send` and `Object.send()` are not flagged)
        let has_args = node
            .arguments()
            .map(|a| a.arguments().iter().count() > 0)
            .unwrap_or(false);
        if !has_args {
            return vec![];
        }

        // Selector = method_name location (message_loc)
        let Some(selector) = node.message_loc() else {
            return vec![];
        };

        vec![ctx.offense_with_range(
            self.name(),
            "Prefer `Object#__send__` or `Object#public_send` to `send`.",
            self.severity(),
            selector.start_offset(),
            selector.end_offset(),
        )]
    }
}

crate::register_cop!("Style/Send", |_cfg| Some(Box::new(Send::new())));
