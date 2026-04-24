//! Style/RedundantFilterChain cop
//!
//! Identifies `select/filter/find_all` followed by `any?/empty?/none?/one?`
//! (or `many?`/`present?` under ActiveSupport) and rewrites using the predicate.
//!
//! Ported from `lib/rubocop/cop/style/redundant_filter_chain.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct RedundantFilterChain {
    active_support: bool,
}

impl RedundantFilterChain {
    pub fn new() -> Self {
        Self { active_support: false }
    }

    pub fn with_config(active_support: bool) -> Self {
        Self { active_support }
    }

    fn replacement(predicate: &str) -> Option<&'static str> {
        match predicate {
            "any?" => Some("any?"),
            "empty?" => Some("none?"),
            "none?" => Some("none?"),
            "one?" => Some("one?"),
            "many?" => Some("many?"),
            "present?" => Some("any?"),
            _ => None,
        }
    }

    fn is_rails_method(m: &str) -> bool {
        m == "many?" || m == "present?"
    }

    fn is_filter_method(m: &str) -> bool {
        m == "select" || m == "filter" || m == "find_all"
    }
}

impl Cop for RedundantFilterChain {
    fn name(&self) -> &'static str {
        "Style/RedundantFilterChain"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let predicate = node_name!(node);
        let Some(replacement) = Self::replacement(predicate.as_ref()) else {
            return vec![];
        };

        if Self::is_rails_method(predicate.as_ref()) && !self.active_support {
            return vec![];
        }

        // Outer call must have no arguments, no block
        if node.arguments().is_some() {
            return vec![];
        }
        if node.block().is_some() {
            return vec![];
        }

        // Receiver must be either:
        //   - a CallNode with a BlockNode attached, whose method is select/filter/find_all
        //   - a CallNode whose only arg is a block_pass and method is select/filter/find_all
        let Some(recv) = node.receiver() else {
            return vec![];
        };
        let Some(recv_call) = recv.as_call_node() else {
            return vec![];
        };
        let recv_method = node_name!(recv_call);
        if !Self::is_filter_method(recv_method.as_ref()) {
            return vec![];
        }

        let has_block = recv_call.block().is_some();
        let mut has_block_pass_only = false;
        if !has_block {
            // must have args containing only a single block_pass
            let Some(args_node) = recv_call.arguments() else {
                return vec![];
            };
            let args: Vec<_> = args_node.arguments().iter().collect();
            if args.len() != 1 {
                return vec![];
            }
            if !matches!(&args[0], Node::BlockArgumentNode { .. }) {
                return vec![];
            }
            has_block_pass_only = true;
        }
        let _ = has_block_pass_only;

        // Offense range: filter selector start → predicate selector end
        let filter_sel = match recv_call.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let pred_sel = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };

        let msg = format!(
            "Use `{}` instead of `{}.{}`.",
            replacement,
            recv_method.as_ref(),
            predicate.as_ref()
        );

        // Correction:
        //   1. Replace filter selector with replacement
        //   2. Remove from filter receiver end → predicate selector end,
        //      i.e. delete `.predicate?` / `&.predicate?` at the end of the chain.
        // But we also need to handle the case where predicate is after a block/args
        // (e.g. `.any?` chained on `.select { ... }`). The range to remove is:
        //   from receiver of predicate call's source_range end → predicate selector end
        let recv_end = recv.location().end_offset();
        let pred_end = pred_sel.end_offset();

        let filter_sel_start = filter_sel.start_offset();
        let filter_sel_end = filter_sel.end_offset();

        let edits = vec![
            Edit {
                start_offset: filter_sel_start,
                end_offset: filter_sel_end,
                replacement: replacement.to_string(),
            },
            Edit {
                start_offset: recv_end,
                end_offset: pred_end,
                replacement: String::new(),
            },
        ];

        let offense = ctx
            .offense_with_range(self.name(), &msg, self.severity(), filter_sel_start, pred_end)
            .with_correction(Correction { edits });
        vec![offense]
    }
}

crate::register_cop!("Style/RedundantFilterChain", |cfg| {
    let cop_config = cfg.get_cop_config("Style/RedundantFilterChain");
    let active_support = cop_config
        .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
        .and_then(|v| v.as_bool())
        .or_else(|| cop_config.and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled")).and_then(|v| v.as_bool()))
        .unwrap_or(false);
    Some(Box::new(RedundantFilterChain::with_config(active_support)))
});
