//! Style/HashTransformValues - prefer `transform_values` over each_with_object/map.to_h/Hash[map]/to_h{}.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/hash_transform_values.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::hash_transform_method as htm;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Node};

#[derive(Default)]
pub struct HashTransformValues;

impl HashTransformValues {
    pub fn new() -> Self {
        Self
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

        // KEY must be `lvar key_arg`
        if !htm::is_lvar_ref(&key_expr, key_arg) {
            return None;
        }
        // VAL must not reference memo
        if htm::subtree_references(&val_expr, memo) {
            return None;
        }

        // noop?
        if htm::is_lvar_ref(&val_expr, val_arg) {
            return None;
        }
        // transformation_uses_both_args? (val references key)
        if htm::subtree_references(&val_expr, key_arg) {
            return None;
        }
        // use_transformed_argname?
        if !htm::subtree_references(&val_expr, val_arg) {
            return None;
        }

        let start = recv.location().start_offset();
        let end = block.location().end_offset();
        let msg = "Prefer `transform_values` over `each_with_object`.".to_string();
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
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
        if !htm::is_lvar_ref(&key_expr, &key_arg) {
            return None;
        }
        if htm::is_lvar_ref(&val_expr, &val_arg) {
            return None;
        }
        if htm::subtree_references(&val_expr, &key_arg) {
            return None;
        }
        if !htm::subtree_references(&val_expr, &val_arg) {
            return None;
        }

        let start = recv.location().start_offset();
        let end = block.location().end_offset();
        let msg = "Prefer `transform_values` over `to_h {...}`.".to_string();
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
    }

    fn check_hash_brackets_map(
        &self,
        outer: &CallNode,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        let (block, _inner_call) = htm::match_hash_brackets_map(outer)?;
        let (key_arg, val_arg) = htm::extract_simple_two_params(&block)?;
        let (key_expr, val_expr) = htm::match_array_pair(&block)?;
        if !htm::is_lvar_ref(&key_expr, &key_arg) {
            return None;
        }
        if htm::is_lvar_ref(&val_expr, &val_arg) {
            return None;
        }
        if htm::subtree_references(&val_expr, &key_arg) {
            return None;
        }
        if !htm::subtree_references(&val_expr, &val_arg) {
            return None;
        }

        let start = outer.location().start_offset();
        let end = outer.location().end_offset();
        let msg = "Prefer `transform_values` over `Hash[_.map {...}]`.".to_string();
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
    }

    fn check_map_to_h(&self, outer: &CallNode, ctx: &CheckContext) -> Option<Offense> {
        let (block, inner_call) = htm::match_map_to_h(outer)?;
        let (key_arg, val_arg) = htm::extract_simple_two_params(&block)?;
        let (key_expr, val_expr) = htm::match_array_pair(&block)?;
        if !htm::is_lvar_ref(&key_expr, &key_arg) {
            return None;
        }
        if htm::is_lvar_ref(&val_expr, &val_arg) {
            return None;
        }
        if htm::subtree_references(&val_expr, &key_arg) {
            return None;
        }
        if !htm::subtree_references(&val_expr, &val_arg) {
            return None;
        }
        let inner_recv = inner_call.receiver()?;
        let start = inner_recv.location().start_offset();
        let end = outer.message_loc().map(|l| l.end_offset()).unwrap_or(outer.location().end_offset());
        let msg = "Prefer `transform_values` over `map {...}.to_h`.".to_string();
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
    }
}

impl Cop for HashTransformValues {
    fn name(&self) -> &'static str {
        "Style/HashTransformValues"
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
                if m == "each_with_object" {
                    if !ctx.ruby_version_at_least(2, 4) {
                        return vec![];
                    }
                    if let Some(o) = self.check_each_with_object(node, &block, ctx) {
                        return vec![o];
                    }
                    return vec![];
                } else if m == "to_h" && ctx.ruby_version_at_least(2, 6) {
                    if let Some(o) = self.check_to_h_block(node, &block, ctx) {
                        return vec![o];
                    }
                    // Fall through to map-to-h check.
                }
            }
        }

        if m == "[]" {
            if let Some(o) = self.check_hash_brackets_map(node, ctx) {
                return vec![o];
            }
        } else if m == "to_h" {
            if let Some(o) = self.check_map_to_h(node, ctx) {
                return vec![o];
            }
        }
        vec![]
    }
}

crate::register_cop!("Style/HashTransformValues", |_cfg| {
    Some(Box::new(HashTransformValues::new()))
});
