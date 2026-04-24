//! Style/RedundantEach cop
//!
//! Checks for redundant chained `each` like `array.each.each { ... }` or
//! `array.each.each_with_index { ... }`.
//!
//! Ported from `lib/rubocop/cop/style/redundant_each.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Remove redundant `each`.";
const MSG_WITH_INDEX: &str = "Use `with_index` to remove redundant `each`.";
const MSG_WITH_OBJECT: &str = "Use `with_object` to remove redundant `each`.";

#[derive(Default)]
pub struct RedundantEach;

impl RedundantEach {
    pub fn new() -> Self {
        Self
    }

    fn last_arg_is_block_pass(call: &ruby_prism::CallNode) -> bool {
        if let Some(args) = call.arguments() {
            let args_vec: Vec<_> = args.arguments().iter().collect();
            if let Some(last) = args_vec.last() {
                return matches!(last, Node::BlockArgumentNode { .. });
            }
        }
        false
    }

    fn has_block_attached(call: &ruby_prism::CallNode) -> bool {
        call.block().is_some()
    }
}

impl Cop for RedundantEach {
    fn name(&self) -> &'static str {
        "Style/RedundantEach"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let n_method = node_name!(node);

        // Receiver must be a call
        let Some(recv) = node.receiver() else {
            return vec![];
        };
        let Some(recv_call) = recv.as_call_node() else {
            return vec![];
        };
        let r_method = node_name!(recv_call);

        let ancestor_set = |m: &str| {
            matches!(m, "each" | "each_with_index" | "each_with_object" | "reverse_each")
        };
        let restrict = |m: &str| matches!(m, "each" | "each_with_index" | "each_with_object");

        // Shared guards: inner (recv_call) must not have block attached, no block_pass last arg.
        if Self::has_block_attached(&recv_call) {
            return vec![];
        }
        if Self::last_arg_is_block_pass(&recv_call) {
            return vec![];
        }

        // Case A: inner is `each`, outer (node) method ∈ ancestor set. Flag INNER.
        if r_method.as_ref() == "each" && ancestor_set(n_method.as_ref()) {
            // Also guard: outer must not have last-arg block_pass (maps to Ruby's
            // `return if node.last_argument&.block_pass_type?`).
            if Self::last_arg_is_block_pass(node) {
                return vec![];
            }
            return emit_branch_a(node, &recv_call, ctx);
        }

        // Case B: flag OUTER (node). Only if node.method ∈ restrict set.
        if !restrict(n_method.as_ref()) {
            return vec![];
        }
        // Block-pass guard on outer as well.
        if Self::last_arg_is_block_pass(node) {
            return vec![];
        }

        let detected_each_prefix = n_method.as_ref() != "each"
            && r_method.as_ref().starts_with("each_");
        let detected_reverse = r_method.as_ref() == "reverse_each";

        if !(detected_each_prefix || detected_reverse) {
            return vec![];
        }

        emit_branch_b(node, ctx)
    }
}

fn emit_branch_a(
    outer: &ruby_prism::CallNode,
    inner: &ruby_prism::CallNode,
    ctx: &CheckContext,
) -> Vec<Offense> {
    // Inner is `each`. Range = inner.selector.join(outer.dot)
    let Some(inner_sel) = inner.message_loc() else {
        return vec![];
    };
    let Some(outer_dot) = outer.call_operator_loc() else {
        return vec![];
    };

    let range_start = inner_sel.start_offset();
    let range_end = outer_dot.end_offset();

    // Flagged node is INNER (method=="each"), so message = MSG.
    let outer_method = node_name!(outer);
    let msg = MSG;

    let mut edits = vec![Edit {
        start_offset: range_start,
        end_offset: range_end,
        replacement: String::new(),
    }];

    // Additional correction for outer=each_with_index/each_with_object:
    // replace outer selector with `each.with_index` / `each.with_object`.
    match outer_method.as_ref() {
        "each_with_index" => {
            if let Some(l) = outer.message_loc() {
                edits.push(Edit {
                    start_offset: l.start_offset(),
                    end_offset: l.end_offset(),
                    replacement: "each.with_index".to_string(),
                });
            }
        }
        "each_with_object" => {
            if let Some(l) = outer.message_loc() {
                edits.push(Edit {
                    start_offset: l.start_offset(),
                    end_offset: l.end_offset(),
                    replacement: "each.with_object".to_string(),
                });
            }
        }
        _ => {}
    }

    let offense = ctx
        .offense_with_range("Style/RedundantEach", msg, Severity::Convention, range_start, range_end)
        .with_correction(Correction { edits });
    vec![offense]
}

fn emit_branch_b(outer: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
    // Flag outer. Outer.method ∈ {each, each_with_index, each_with_object}.
    let outer_method = node_name!(outer);
    let Some(sel) = outer.message_loc() else {
        return vec![];
    };

    let (msg, correction_replacement): (&str, Option<&str>) = match outer_method.as_ref() {
        "each" => (MSG, None),
        "each_with_index" => (MSG_WITH_INDEX, Some("with_index")),
        "each_with_object" => (MSG_WITH_OBJECT, Some("with_object")),
        _ => return vec![],
    };

    let (range_start, range_end) = if outer_method.as_ref() == "each" {
        // Range = dot.join(selector) — outer's own dot + selector
        match outer.call_operator_loc() {
            Some(d) => (d.start_offset(), sel.end_offset()),
            None => (sel.start_offset(), sel.end_offset()),
        }
    } else {
        (sel.start_offset(), sel.end_offset())
    };

    let edits = if outer_method.as_ref() == "each" {
        // Remove `.each` / `&.each`
        vec![Edit {
            start_offset: range_start,
            end_offset: range_end,
            replacement: String::new(),
        }]
    } else if let Some(repl) = correction_replacement {
        vec![Edit {
            start_offset: sel.start_offset(),
            end_offset: sel.end_offset(),
            replacement: repl.to_string(),
        }]
    } else {
        vec![]
    };

    let offense = ctx
        .offense_with_range("Style/RedundantEach", msg, Severity::Convention, range_start, range_end)
        .with_correction(Correction { edits });
    vec![offense]
}

crate::register_cop!("Style/RedundantEach", |_cfg| Some(Box::new(RedundantEach::new())));
