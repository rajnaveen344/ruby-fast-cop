//! Style/RedundantInterpolationUnfreeze - Flags unfreezing interpolated strings in Ruby 3.0+.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_interpolation_unfreeze.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Don't unfreeze interpolated strings as they are already unfrozen.";

#[derive(Default)]
pub struct RedundantInterpolationUnfreeze;

impl RedundantInterpolationUnfreeze {
    pub fn new() -> Self {
        Self
    }
}

/// Check if node is an interpolated string with actual interpolation (not just
/// concatenated string parts / uninterpolated heredoc).
fn is_dstr_with_interpolation(node: &Node) -> bool {
    let interp = match node.as_interpolated_string_node() {
        Some(i) => i,
        None => return false,
    };
    // Must have at least one EmbeddedStatements / EmbeddedVariable part.
    interp.parts().iter().any(|p| {
        matches!(
            p,
            Node::EmbeddedStatementsNode { .. } | Node::EmbeddedVariableNode { .. }
        )
    })
}

fn is_string_const(recv: &Node) -> bool {
    if let Some(c) = recv.as_constant_read_node() {
        return String::from_utf8_lossy(c.name().as_slice()) == "String";
    }
    false
}

impl Cop for RedundantInterpolationUnfreeze {
    fn name(&self) -> &'static str {
        "Style/RedundantInterpolationUnfreeze"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only Ruby >= 3.0
        if !ctx.ruby_version_at_least(3, 0) {
            return vec![];
        }

        let method = node_name!(node);
        let method_str = method.as_ref();

        let call_loc = node.location();
        let call_start = call_loc.start_offset();
        let call_end = call_loc.end_offset();

        // Case 1: dstr.+@ or dstr.dup (no args, has dstr receiver).
        if method_str == "+@" || method_str == "dup" {
            // Must have no arguments.
            if node.arguments().is_some() {
                return vec![];
            }
            if node.block().is_some() {
                return vec![];
            }
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            if !is_dstr_with_interpolation(&recv) {
                return vec![];
            }
            let recv_loc = recv.location();
            let recv_src = ctx.source[recv_loc.start_offset()..recv_loc.end_offset()].to_string();

            // Offense range: for `+"..."` unary plus, selector is `+` at call start.
            // For `"...".+@` normal call, selector is `+@` or `.dup`.
            // Ruby: `node.method?(:new) ? source_range.begin.join(selector) : selector`
            // For +@/dup (not :new), it's just selector.
            let sel = match node.message_loc() {
                Some(l) => l,
                None => return vec![],
            };
            let off_start = sel.start_offset();
            let off_end = sel.end_offset();

            return vec![ctx
                .offense_with_range(self.name(), MSG, self.severity(), off_start, off_end)
                .with_correction(Correction::replace(call_start, call_end, recv_src))];
        }

        // Case 2: String.new(dstr)
        if method_str == "new" {
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            if !is_string_const(&recv) {
                return vec![];
            }
            let arg_list: Vec<_> = match node.arguments() {
                Some(a) => a.arguments().iter().collect(),
                None => return vec![],
            };
            if arg_list.len() != 1 {
                return vec![];
            }
            if !is_dstr_with_interpolation(&arg_list[0]) {
                return vec![];
            }
            let dstr = &arg_list[0];
            let dstr_loc = dstr.location();
            let dstr_src = ctx.source[dstr_loc.start_offset()..dstr_loc.end_offset()].to_string();

            // Offense range: source_range.begin .. selector (end)
            let sel = match node.message_loc() {
                Some(l) => l,
                None => return vec![],
            };
            let off_start = call_start;
            let off_end = sel.end_offset();

            return vec![ctx
                .offense_with_range(self.name(), MSG, self.severity(), off_start, off_end)
                .with_correction(Correction::replace(call_start, call_end, dstr_src))];
        }

        vec![]
    }
}

crate::register_cop!("Style/RedundantInterpolationUnfreeze", |_cfg| Some(Box::new(RedundantInterpolationUnfreeze::new())));
