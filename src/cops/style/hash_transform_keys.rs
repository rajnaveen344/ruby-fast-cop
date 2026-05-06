//! Style/HashTransformKeys - prefer `transform_keys` over each_with_object/map.to_h/Hash[map]/to_h{}.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/hash_transform_keys.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::hash_transform_method as htm;
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{BlockNode, CallNode};

const NEW_METHOD: &str = "transform_keys";

#[derive(Default)]
pub struct HashTransformKeys;

impl HashTransformKeys {
    pub fn new() -> Self {
        Self
    }

    fn make_correction<'a>(
        source: &str,
        block_call: &CallNode<'a>,
        block: &BlockNode<'a>,
        outer_start: usize,
        outer_end: usize,
        new_arg: &str,
        key_expr: &ruby_prism::Node<'a>,
        map_to_h_outer_end: Option<usize>,
        hash_brackets_outer: bool,
    ) -> Correction {
        let edits = htm::hash_transform_edits(
            source, outer_start, outer_end,
            block_call, block, NEW_METHOD, new_arg, key_expr,
            map_to_h_outer_end, hash_brackets_outer,
        );
        Correction { edits: edits.into_iter().map(|(s, e, r)| Edit { start_offset: s, end_offset: e, replacement: r }).collect() }
    }

    fn check_block_node(
        &self,
        block_call: &CallNode,
        block: &BlockNode,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        let method = node_name!(block_call);
        let method_str: &str = method.as_ref();

        if method_str == "each_with_object" {
            if !ctx.ruby_version_at_least(2, 5) {
                return None;
            }
            return self.check_each_with_object(block_call, block, ctx);
        }

        if method_str == "to_h" && ctx.ruby_version_at_least(2, 6) {
            return self.check_to_h_block(block_call, block, ctx);
        }

        None
    }

    fn check_each_with_object(
        &self,
        block_call: &CallNode,
        block: &BlockNode,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        if !htm::is_each_with_object_empty_hash(block_call) {
            return None;
        }
        let recv = block_call.receiver()?;
        if !htm::is_hash_receiver_expr(&recv) {
            return None;
        }
        let params = htm::extract_ewo_params(block)?;
        let key_arg = &params.first;
        let val_arg = &params.second;
        let memo = &params.memo;

        let body_stmt = htm::body_single_stmt(block)?;
        let (key_expr, val_expr) = htm::match_index_assign(&body_stmt, memo)?;

        if !htm::is_lvar_ref(&val_expr, val_arg) {
            return None;
        }
        if htm::subtree_references(&key_expr, memo) {
            return None;
        }
        if htm::is_lvar_ref(&key_expr, key_arg) {
            return None;
        }
        if htm::subtree_references(&key_expr, val_arg) {
            return None;
        }
        if !htm::subtree_references(&key_expr, key_arg) {
            return None;
        }

        let start = recv.location().start_offset();
        let end = block.location().end_offset();
        let msg = "Prefer `transform_keys` over `each_with_object`.".to_string();

        let correction = Self::make_correction(ctx.source, block_call, block, start, end, key_arg, &key_expr, None, false);
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(correction))
    }

    fn check_to_h_block(
        &self,
        block_call: &CallNode,
        block: &BlockNode,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        let recv = block_call.receiver()?;
        if !htm::is_hash_receiver_expr(&recv) {
            return None;
        }
        let (key_arg, val_arg) = htm::extract_simple_two_params(block)?;
        let (key_expr, val_expr) = htm::match_array_pair(block)?;
        if !htm::is_lvar_ref(&val_expr, &val_arg) {
            return None;
        }
        if htm::is_lvar_ref(&key_expr, &key_arg) {
            return None;
        }
        if htm::subtree_references(&key_expr, &val_arg) {
            return None;
        }
        if !htm::subtree_references(&key_expr, &key_arg) {
            return None;
        }

        let start = recv.location().start_offset();
        let end = block.location().end_offset();
        let msg = "Prefer `transform_keys` over `to_h {...}`.".to_string();

        let correction = Self::make_correction(ctx.source, block_call, block, start, end, &key_arg, &key_expr, None, false);
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(correction))
    }

    fn check_hash_brackets_map(
        &self,
        outer: &CallNode,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        let (block, inner_call) = htm::match_hash_brackets_map(outer)?;
        let (key_arg, val_arg) = htm::extract_simple_two_params(&block)?;
        let (key_expr, val_expr) = htm::match_array_pair(&block)?;
        if !htm::is_lvar_ref(&val_expr, &val_arg) {
            return None;
        }
        if htm::is_lvar_ref(&key_expr, &key_arg) {
            return None;
        }
        if htm::subtree_references(&key_expr, &val_arg) {
            return None;
        }
        if !htm::subtree_references(&key_expr, &key_arg) {
            return None;
        }

        let start = outer.location().start_offset();
        let end = outer.location().end_offset();
        let msg = "Prefer `transform_keys` over `Hash[_.map {...}]`.".to_string();

        let correction = Self::make_correction(ctx.source, &inner_call, &block, start, end, &key_arg, &key_expr, None, true);
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(correction))
    }

    fn check_map_to_h(&self, outer: &CallNode, ctx: &CheckContext) -> Option<Offense> {
        let (block, inner_call) = htm::match_map_to_h(outer)?;
        let (key_arg, val_arg) = htm::extract_simple_two_params(&block)?;
        let (key_expr, val_expr) = htm::match_array_pair(&block)?;
        if !htm::is_lvar_ref(&val_expr, &val_arg) {
            return None;
        }
        if htm::is_lvar_ref(&key_expr, &key_arg) {
            return None;
        }
        if htm::subtree_references(&key_expr, &val_arg) {
            return None;
        }
        if !htm::subtree_references(&key_expr, &key_arg) {
            return None;
        }
        let inner_recv = inner_call.receiver()?;
        let start = inner_recv.location().start_offset();
        let outer_end = outer.message_loc().map(|l| l.end_offset()).unwrap_or(outer.location().end_offset());
        let msg = "Prefer `transform_keys` over `map {...}.to_h`.".to_string();

        // RuboCop: if `to_h` itself has a block, don't strip the `.to_h` trailing chars.
        let strip_trailing = if outer.block().is_none() { Some(outer_end) } else { None };

        let correction = Self::make_correction(ctx.source, &inner_call, &block, start, outer_end, &key_arg, &key_expr, strip_trailing, false);
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, outer_end)
            .with_correction(correction))
    }
}

impl Cop for HashTransformKeys {
    fn name(&self) -> &'static str {
        "Style/HashTransformKeys"
    }
    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let m: &str = method.as_ref();

        if node.block().is_some() {
            let block_node = node.block().unwrap();
            if let Some(block) = block_node.as_block_node() {
                if let Some(o) = self.check_block_node(node, &block, ctx) {
                    return vec![o];
                }
            }
        }

        if m == "[]" {
            if let Some(o) = self.check_hash_brackets_map(node, ctx) {
                return vec![o];
            }
        } else if m == "to_h" {
            if !ctx.ruby_version_at_least(2, 5) {
                return vec![];
            }
            if let Some(o) = self.check_map_to_h(node, ctx) {
                return vec![o];
            }
        }
        vec![]
    }
}

crate::register_cop!("Style/HashTransformKeys", |_cfg| {
    Some(Box::new(HashTransformKeys::new()))
});
