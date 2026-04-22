//! Style/RedundantFetchBlock cop
//!
//! Detects `fetch(:key) { literal }` replaceable with `fetch(:key, literal)`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct RedundantFetchBlock {
    safe_for_constants: bool,
}

impl RedundantFetchBlock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(safe_for_constants: bool) -> Self {
        Self { safe_for_constants }
    }

    /// Check if a block body is a simple literal safe to hoist.
    fn block_literal_src<'a>(block: &ruby_prism::BlockNode, source: &'a str, safe_for_constants: bool, frozen_strings: bool) -> Option<&'a str> {
        // Block must have no parameters
        if block.parameters().is_some() {
            return None;
        }
        let body = match block.body() {
            Some(b) => b,
            None => {
                // Empty block → nil
                return Some("nil");
            }
        };
        let stmts = body.as_statements_node()?;
        let parts: Vec<_> = stmts.body().iter().collect();
        if parts.len() != 1 {
            return None;
        }
        let node = &parts[0];
        // Check if node is a safe literal
        let ok = match node {
            Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::SymbolNode { .. }
            | Node::RationalNode { .. } | Node::ImaginaryNode { .. } => true,
            Node::StringNode { .. } => {
                // Only safe if strings are frozen (frozen_string_literal: true)
                frozen_strings
            }
            Node::ConstantReadNode { .. } => safe_for_constants,
            _ => false,
        };
        if ok {
            let loc = node.location();
            Some(&source[loc.start_offset()..loc.end_offset()])
        } else {
            None
        }
    }

    fn has_frozen_string_literal(source: &str) -> bool {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            if !trimmed.starts_with('#') { break; }
            let content = trimmed[1..].trim();
            if let Some((key, val)) = content.split_once(':') {
                let key_norm = key.trim().to_lowercase().replace(['-', '_'], "");
                if key_norm == "frozenstringliteral" {
                    return val.trim().eq_ignore_ascii_case("true");
                }
            }
        }
        false
    }
}

impl Cop for RedundantFetchBlock {
    fn name(&self) -> &'static str {
        "Style/RedundantFetchBlock"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "fetch" {
            return vec![];
        }

        // Must have a block (not block arg)
        let block = match node.block() {
            Some(b) => match b.as_block_node() {
                Some(bn) => bn,
                None => return vec![],
            },
            None => return vec![],
        };

        // Must have exactly 1 argument (the key)
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }

        // Skip Rails.cache.fetch (receiver chain)
        // RuboCop skips when receiver is a multi-level chain (Rails.cache)
        if let Some(recv) = node.receiver() {
            if let Some(recv_call) = recv.as_call_node() {
                if recv_call.receiver().is_some() {
                    return vec![];
                }
            }
        }

        let frozen = Self::has_frozen_string_literal(ctx.source);
        let lit_src = match Self::block_literal_src(&block, ctx.source, self.safe_for_constants, frozen) {
            Some(s) => s,
            None => return vec![],
        };

        // Empty block → nil; else use literal
        let replacement_val = if node.block().map_or(false, |b| {
            b.as_block_node().map_or(false, |bn| bn.body().is_none())
        }) {
            "nil"
        } else {
            lit_src
        };

        let key_src = &ctx.source[arg_list[0].location().start_offset()..arg_list[0].location().end_offset()];
        let new_call = format!("fetch({}, {})", key_src, replacement_val);

        // Offense: from method name start to block end
        let method_start = node.message_loc().map_or(node.location().start_offset(), |l| l.start_offset());
        let call_end = node.location().end_offset();
        let old_src = &ctx.source[method_start..call_end];
        let msg = format!("Use `{}` instead of `{}`.", new_call, old_src);

        let correction = Correction::replace(method_start, call_end, new_call);

        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), method_start, call_end)
            .with_correction(correction)]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    safe_for_constants: bool,
}

crate::register_cop!("Style/RedundantFetchBlock", |cfg| {
    let c: Cfg = cfg.typed("Style/RedundantFetchBlock");
    Some(Box::new(RedundantFetchBlock::with_config(c.safe_for_constants)))
});
