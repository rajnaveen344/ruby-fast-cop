//! Lint/BigDecimalNew cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/big_decimal_new.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;
use ruby_prism::Node;

#[derive(Default)]
pub struct BigDecimalNew;

impl BigDecimalNew {
    pub fn new() -> Self { Self }

    fn is_big_decimal_receiver(node: &Node, ctx: &CheckContext) -> Option<usize> {
        // Returns the start offset of the receiver for correction purposes
        match node {
            Node::ConstantReadNode { .. } => {
                let name = node_name!(node.as_constant_read_node().unwrap());
                if name == "BigDecimal" {
                    Some(node.location().start_offset())
                } else {
                    None
                }
            }
            Node::ConstantPathNode { .. } => {
                // ::BigDecimal
                let cp = node.as_constant_path_node().unwrap();
                let name = cp.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string()).unwrap_or_default();
                if name == "BigDecimal" && cp.parent().is_none() {
                    Some(node.location().start_offset())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl Cop for BigDecimalNew {
    fn name(&self) -> &'static str { "Lint/BigDecimalNew" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "new" {
            return vec![];
        }
        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let recv_start = match Self::is_big_decimal_receiver(&recv, ctx) {
            Some(s) => s,
            None => return vec![],
        };

        // Offense on the method selector ("new")
        let msg_loc = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };

        // Correction: replace `BigDecimal.new(...)` with `BigDecimal(...)`
        // Remove receiver prefix (e.g. `::BigDecimal`) and `.new`
        // recv_start to dot+".new" end = msg_loc.end_offset()
        // Keep the args and closing paren intact
        let args_src = if let Some(args) = node.arguments() {
            let first = args.arguments().iter().next();
            let last = args.arguments().iter().last();
            match (first, last) {
                (Some(f), Some(l)) => ctx.src(f.location().start_offset(), l.location().end_offset()).to_string(),
                _ => String::new(),
            }
        } else {
            String::new()
        };
        // Also grab optional block
        // Build replacement: BigDecimal(args)
        let replacement = format!("BigDecimal({})", args_src);
        let call_end = if let Some(cl) = node.closing_loc() {
            cl.end_offset()
        } else if let Some(args) = node.arguments() {
            args.arguments().iter().last().map(|a| a.location().end_offset()).unwrap_or(msg_loc.end_offset())
        } else {
            msg_loc.end_offset()
        };

        let correction = Correction::replace(recv_start, call_end, replacement);

        vec![ctx.offense_with_range(
            "Lint/BigDecimalNew",
            "`BigDecimal.new()` is deprecated. Use `BigDecimal()` instead.",
            Severity::Warning,
            msg_loc.start_offset(),
            msg_loc.end_offset(),
        ).with_correction(correction)]
    }
}

crate::register_cop!("Lint/BigDecimalNew", |_cfg| {
    Some(Box::new(BigDecimalNew::new()))
});
