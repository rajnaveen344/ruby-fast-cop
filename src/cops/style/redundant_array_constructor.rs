//! Style/RedundantArrayConstructor - Checks for redundant `Array` constructor calls.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_array_constructor.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Remove the redundant `Array` constructor.";

#[derive(Default)]
pub struct RedundantArrayConstructor;

impl RedundantArrayConstructor {
    pub fn new() -> Self {
        Self
    }
}

/// Check if receiver is `Array` or `::Array`.
fn is_array_const(recv: &Node) -> bool {
    if let Some(c) = recv.as_constant_read_node() {
        return String::from_utf8_lossy(c.name().as_slice()) == "Array";
    }
    if let Some(cp) = recv.as_constant_path_node() {
        // name must be "Array"
        let name = match cp.name() {
            Some(n) => n,
            None => return false,
        };
        if String::from_utf8_lossy(name.as_slice()) != "Array" {
            return false;
        }
        // parent must be None (cbase) — ::Array
        return cp.parent().is_none();
    }
    false
}

impl Cop for RedundantArrayConstructor {
    fn name(&self) -> &'static str {
        "Style/RedundantArrayConstructor"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let method_str = method.as_ref();

        // Block must not be present.
        if node.block().is_some() {
            return vec![];
        }

        let arg_list: Vec<_> = node
            .arguments()
            .map(|a| a.arguments().iter().collect::<Vec<_>>())
            .unwrap_or_default();

        let call_loc = node.location();
        let call_start = call_loc.start_offset();
        let call_end = call_loc.end_offset();

        match method_str {
            "new" => {
                // Array.new(array_literal) - exactly 1 arg, array literal
                let recv = match node.receiver() {
                    Some(r) => r,
                    None => return vec![],
                };
                if !is_array_const(&recv) {
                    return vec![];
                }
                if arg_list.len() != 1 {
                    return vec![];
                }
                let array_node = match arg_list[0].as_array_node() {
                    Some(a) => a,
                    None => return vec![],
                };
                // Offense range: receiver .. selector (e.g. "Array.new")
                let sel = match node.message_loc() {
                    Some(l) => l,
                    None => return vec![],
                };
                let off_start = recv.location().start_offset();
                let off_end = sel.end_offset();
                let array_src = &ctx.source[array_node.location().start_offset()..array_node.location().end_offset()];
                vec![ctx
                    .offense_with_range(self.name(), MSG, self.severity(), off_start, off_end)
                    .with_correction(Correction::replace(call_start, call_end, array_src.to_string()))]
            }
            "[]" => {
                // Array[...] - any number of args, including zero
                let recv = match node.receiver() {
                    Some(r) => r,
                    None => return vec![],
                };
                if !is_array_const(&recv) {
                    return vec![];
                }
                // Offense range: receiver location
                let recv_loc = recv.location();
                let off_start = recv_loc.start_offset();
                let off_end = recv_loc.end_offset();
                // Replacement: brackets + contents. In `Array[1,2]`, selector is `[`
                // through `]`. We simply replace with `[contents]`.
                let sel = match node.message_loc() {
                    Some(l) => l,
                    None => return vec![],
                };
                // Replacement text: from selector.begin to call end. But selector is `[]`
                // with 0 args, or `[` when args exist. Easiest: take source from `[` (selector start)
                // to call end.
                let repl_start = sel.start_offset();
                let replacement = &ctx.source[repl_start..call_end];
                vec![ctx
                    .offense_with_range(self.name(), MSG, self.severity(), off_start, off_end)
                    .with_correction(Correction::replace(call_start, call_end, replacement.to_string()))]
            }
            "Array" => {
                // Array(array_literal) - Kernel#Array, no receiver
                if node.receiver().is_some() {
                    return vec![];
                }
                if arg_list.len() != 1 {
                    return vec![];
                }
                let array_node = match arg_list[0].as_array_node() {
                    Some(a) => a,
                    None => return vec![],
                };
                let sel = match node.message_loc() {
                    Some(l) => l,
                    None => return vec![],
                };
                let off_start = sel.start_offset();
                let off_end = sel.end_offset();
                let array_src = &ctx.source[array_node.location().start_offset()..array_node.location().end_offset()];
                vec![ctx
                    .offense_with_range(self.name(), MSG, self.severity(), off_start, off_end)
                    .with_correction(Correction::replace(call_start, call_end, array_src.to_string()))]
            }
            _ => vec![],
        }
    }
}

crate::register_cop!("Style/RedundantArrayConstructor", |_cfg| Some(Box::new(RedundantArrayConstructor::new())));
