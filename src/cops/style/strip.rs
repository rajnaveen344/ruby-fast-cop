//! Style/Strip cop
//!
//! Checks for `lstrip.rstrip` or `rstrip.lstrip` chains; suggests `strip`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Visit};

#[derive(Default)]
pub struct Strip;

impl Strip {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for Strip {
    fn name(&self) -> &'static str {
        "Style/Strip"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = StripVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct StripVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> StripVisitor<'a> {
    fn check_call(&mut self, node: &CallNode) {
        // Outer call must be `lstrip` or `rstrip` with no args
        let outer_method = node_name!(node);
        let (outer, inner_expected) = match outer_method.as_ref() {
            "lstrip" => ("lstrip", "rstrip"),
            "rstrip" => ("rstrip", "lstrip"),
            _ => return,
        };
        if node.arguments().is_some() {
            return;
        }

        // Receiver must be a call node (lstrip/rstrip or &.lstrip/&.rstrip)
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        let inner_call = match receiver.as_call_node() {
            Some(c) => c,
            None => return,
        };

        let inner_method = node_name!(inner_call);
        if inner_method.as_ref() != inner_expected {
            return;
        }
        if inner_call.arguments().is_some() {
            return;
        }

        // Build the dot notation string for the message
        // inner: is it safe nav? outer: is it safe nav?
        let inner_safe = inner_call.call_operator_loc()
            .map(|l| &self.ctx.source[l.start_offset()..l.end_offset()] == "&.")
            .unwrap_or(false);
        let outer_safe = node.call_operator_loc()
            .map(|l| &self.ctx.source[l.start_offset()..l.end_offset()] == "&.")
            .unwrap_or(false);

        let outer_dot = if outer_safe { "&." } else { "." };
        let methods_str = format!("{}{}{}{}", inner_expected, outer_dot, outer, "");
        // e.g. "lstrip.rstrip" or "lstrip&.rstrip"

        let msg = format!("Use `strip` instead of `{}`.", methods_str);

        // Range: from inner method loc to end of outer
        let start = inner_call.message_loc()
            .unwrap_or_else(|| inner_call.location())
            .start_offset();
        let end = node.message_loc()
            .unwrap_or_else(|| node.location())
            .end_offset();

        self.offenses.push(self.ctx.offense_with_range(
            "Style/Strip",
            &msg,
            Severity::Convention,
            start,
            end,
        ));
    }
}

impl Visit<'_> for StripVisitor<'_> {
    fn visit_call_node(&mut self, node: &CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/Strip", |_cfg| {
    Some(Box::new(Strip::new()))
});
