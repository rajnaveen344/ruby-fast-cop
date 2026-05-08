//! Style/ArrayFirstLast cop
//!
//! Identifies `arr[0]` / `arr[-1]` and suggests `arr.first` / `arr.last`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct ArrayFirstLast;

impl ArrayFirstLast {
    pub fn new() -> Self {
        Self
    }

    fn is_int_arg(node: &ruby_prism::CallNode, source: &str) -> Option<i64> {
        let args = node.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return None;
        }
        match &arg_list[0] {
            Node::IntegerNode { .. } => {
                let loc = arg_list[0].location();
                source[loc.start_offset()..loc.end_offset()].parse::<i64>().ok()
            }
            _ => None,
        }
    }
}

impl Cop for ArrayFirstLast {
    fn name(&self) -> &'static str {
        "Style/ArrayFirstLast"
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "[]" {
            return vec![];
        }
        let value = match Self::is_int_arg(node, ctx.source) {
            Some(v) if v == 0 || v == -1 => v,
            _ => return vec![],
        };

        // Skip outer `[]` calls that wrap another `[]` (chained `arr[0][-1]`).
        // From the outer side: receiver is itself a `[]` call.
        if let Some(recv) = node.receiver() {
            if let Some(recv_call) = recv.as_call_node() {
                if node_name!(recv_call) == "[]" {
                    return vec![];
                }
            }
        }
        // From inner side: parent is `[]`. No parent ref available in visitor — detect
        // by source: char immediately after this call's end is `[`.
        let after = node.location().end_offset();
        let bytes = ctx.bytes();
        if after < bytes.len() && bytes[after] == b'[' {
            return vec![];
        }

        let preferred = if value == 0 { "first" } else { "last" };

        // Range:
        //   - if call_operator_loc present (dot/safe-nav): start = message_loc start, end = call end
        //   - else: message_loc
        let msg_loc = match node.message_loc() {
            Some(m) => m,
            None => return vec![],
        };
        let (start, end) = if node.call_operator_loc().is_some() {
            (msg_loc.start_offset(), node.location().end_offset())
        } else {
            (msg_loc.start_offset(), msg_loc.end_offset())
        };

        let message = format!("Use `{}`.", preferred);
        // Correction: replace from call_operator (or '[') through node end with .first/.last
        let node_end = node.location().end_offset();
        let correction = if let Some(op_loc) = node.call_operator_loc() {
            let op = &ctx.source[op_loc.start_offset()..op_loc.end_offset()];
            let replacement = format!("{}{}", op, preferred);
            Correction::replace(op_loc.start_offset(), node_end, replacement)
        } else {
            // arr[0] → arr.first  — replace from '[' through end
            Correction::replace(msg_loc.start_offset(), node_end, format!(".{}", preferred))
        };
        vec![ctx.offense_with_range(self.name(), &message, self.severity(), start, end)
            .with_correction(correction)]
    }
}

crate::register_cop!("Style/ArrayFirstLast", |_cfg| Some(Box::new(ArrayFirstLast::new())));
