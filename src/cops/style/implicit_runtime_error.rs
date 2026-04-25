//! Style/ImplicitRuntimeError — flags `raise`/`fail` with only a message
//! (which raises an implicit `RuntimeError`).

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct ImplicitRuntimeError;

impl ImplicitRuntimeError {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ImplicitRuntimeError {
    fn name(&self) -> &'static str {
        "Style/ImplicitRuntimeError"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "raise" && method != "fail" {
            return vec![];
        }
        // Must be implicit-self call (no receiver)
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

        // The single argument must be a string (str or dstr)
        let is_str = matches!(
            arg_list[0],
            ruby_prism::Node::StringNode { .. } | ruby_prism::Node::InterpolatedStringNode { .. }
        );
        if !is_str {
            return vec![];
        }

        let msg = format!(
            "Use `{}` with an explicit exception class and message, rather than just a message.",
            method
        );
        let nloc = node.location();
        let start = nloc.start_offset();
        let end = nloc.end_offset();

        // Truncate end to end of first physical line (RuboCop highlights only line 1).
        let bytes = ctx.source.as_bytes();
        let mut line_end = start;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }
        let final_end = end.min(line_end);

        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), start, final_end)]
    }
}

crate::register_cop!("Style/ImplicitRuntimeError", |_cfg| Some(Box::new(ImplicitRuntimeError::new())));
