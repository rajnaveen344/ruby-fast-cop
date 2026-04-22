//! Style/SingleArgumentDig cop
//!
//! Detects `.dig(single_arg)` calls that can be replaced with `[single_arg]`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Use `%s` instead of `%s`.";

#[derive(Default)]
pub struct SingleArgumentDig {
    dig_chain_enabled: bool,
}

impl SingleArgumentDig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(dig_chain_enabled: bool) -> Self {
        Self { dig_chain_enabled }
    }

    /// Build replacement source: `receiver[arg]`
    fn build_replacement(node: &ruby_prism::CallNode, ctx: &CheckContext) -> Option<String> {
        let receiver = node.receiver()?;
        let args_node = node.arguments()?;
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() != 1 {
            return None;
        }
        let arg = &args[0];
        // Skip splat / forwarding args
        if matches!(
            arg,
            Node::SplatNode { .. }
                | Node::ForwardingArgumentsNode { .. }
                | Node::BlockArgumentNode { .. }
                | Node::KeywordHashNode { .. }
        ) {
            return None;
        }

        let recv_src = &ctx.source[receiver.location().start_offset()..receiver.location().end_offset()];
        let arg_src = &ctx.source[arg.location().start_offset()..arg.location().end_offset()];
        Some(format!("{}[{}]", recv_src, arg_src))
    }

    fn check_node(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "dig" {
            return vec![];
        }
        // No receiver → free-standing `dig(:key)` → skip
        if node.receiver().is_none() {
            return vec![];
        }
        // Safe nav (&.) → skip
        if node.call_operator_loc().map_or(false, |op| op.as_slice() == b"&.") {
            return vec![];
        }

        // Check args
        let args_node = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() != 1 {
            return vec![];
        }
        let arg = &args[0];
        if matches!(
            arg,
            Node::SplatNode { .. }
                | Node::ForwardingArgumentsNode { .. }
                | Node::BlockArgumentNode { .. }
                | Node::KeywordHashNode { .. }
        ) {
            return vec![];
        }

        // If DigChain enabled, skip digs that are part of a dig chain
        if self.dig_chain_enabled {
            // Skip if receiver is also a dig (inner dig in chain)
            let recv_is_dig = node.receiver().and_then(|r| r.as_call_node()).map_or(false, |c| {
                node_name!(c) == "dig"
            });
            if recv_is_dig {
                return vec![];
            }
            // Skip if this call is used as receiver of another dig (outer dig in chain)
            // Check: does the source immediately after this call contain `.dig(`?
            let call_end = node.location().end_offset();
            let after = &ctx.source[call_end..];
            let after_trimmed = after.trim_start_matches(|c: char| c == '[' || c == ']' || c.is_ascii_digit());
            // Check for `.dig(` or `[N].dig(` patterns
            if after.starts_with(".dig(") {
                return vec![];
            }
            // Also check `[n].dig(` pattern
            if after.starts_with('[') {
                // Find `].dig(`
                if let Some(end) = after.find("].dig(") {
                    // Check no nested content breaks it
                    let between = &after[1..end];
                    if !between.contains('[') {
                        return vec![];
                    }
                }
            }
        }

        let replacement = match Self::build_replacement(node, ctx) {
            Some(r) => r,
            None => return vec![],
        };

        // Offense range: from start of receiver to end of call
        let recv = node.receiver().unwrap();
        let recv_start = recv.location().start_offset();
        let call_end = node.location().end_offset();
        let call_src = &ctx.source[recv_start..call_end];
        let msg = MSG.replacen("%s", &replacement, 1).replacen("%s", call_src, 1);

        // Skip correction if this dig is a receiver of another dig (avoid overlap with outer correction)
        let is_outer_receiver = {
            let after = &ctx.source[call_end..];
            after.starts_with(".dig(") || (after.starts_with('[') && after.contains("].dig("))
        };

        if is_outer_receiver {
            // No correction — let the outer dig's correction handle it
            return vec![ctx.offense_with_range(self.name(), &msg, self.severity(), recv_start, call_end)];
        }

        // Correction: replace from receiver start to call end
        let correction = Correction::replace(recv_start, call_end, replacement);

        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), recv_start, call_end)
            .with_correction(correction)]
    }
}

impl Cop for SingleArgumentDig {
    fn name(&self) -> &'static str {
        "Style/SingleArgumentDig"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_node(node, ctx)
    }
}

crate::register_cop!("Style/SingleArgumentDig", |cfg| {
    // DigChain is pending/disabled by default. Only skip chained digs if explicitly enabled.
    let dig_chain_enabled = cfg.get_cop_config("Style/DigChain")
        .and_then(|c| c.enabled)
        .unwrap_or(false); // false = not enabled unless config says so
    Some(Box::new(SingleArgumentDig::with_config(dig_chain_enabled)))
});
