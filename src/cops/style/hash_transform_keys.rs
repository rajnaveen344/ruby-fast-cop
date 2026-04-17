//! Style/HashTransformKeys - prefer `transform_keys` over each_with_object/map.to_h/Hash[map]/to_h{}.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/hash_transform_keys.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::hash_transform_method as htm;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Node};

#[derive(Default)]
pub struct HashTransformKeys;

impl HashTransformKeys {
    pub fn new() -> Self {
        Self
    }

    /// Check a BlockNode: handles each_with_object (ruby >= 2.5) and to_h {...} (ruby >= 2.6) patterns.
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
        // Receiver must look like a hash
        let recv = block_call.receiver()?;
        if !htm::is_hash_receiver_expr(&recv) {
            return None;
        }
        // Block params: |(k, v), memo|
        let params = htm::extract_ewo_params(block)?;
        let key_arg = &params.first;
        let val_arg = &params.second;
        let memo = &params.memo;

        // Body: single stmt `memo[KEY] = VAL` where VAL is `lvar val_arg`
        let body_stmt = htm::body_single_stmt(block)?;
        let (key_expr, val_expr) = htm::match_index_assign(&body_stmt, memo)?;

        // VAL must be `lvar val_arg`
        if !htm::is_lvar_ref(&val_expr, val_arg) {
            return None;
        }
        // KEY must not reference memo
        if htm::subtree_references(&key_expr, memo) {
            return None;
        }

        // Checks from Captures
        // noop?
        if htm::is_lvar_ref(&key_expr, key_arg) {
            return None;
        }
        // transformation_uses_both_args? (key references value)
        if htm::subtree_references(&key_expr, val_arg) {
            return None;
        }
        // use_transformed_argname? (key references key_arg)
        if !htm::subtree_references(&key_expr, key_arg) {
            return None;
        }

        // The block outer range: block_call.location() spans recv...block end
        // Prism's CallNode with a block: node.location spans whole thing including the block.
        let start = recv.location().start_offset();
        // End = end of block
        let end = block.location().end_offset();
        let msg = "Prefer `transform_keys` over `each_with_object`.".to_string();
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
    }

    fn check_to_h_block(
        &self,
        block_call: &CallNode,
        block: &BlockNode,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        // Receiver must be a hash
        let recv = block_call.receiver()?;
        if !htm::is_hash_receiver_expr(&recv) {
            return None;
        }
        // Block params: simple |k, v|
        let (key_arg, val_arg) = htm::extract_simple_two_params(block)?;
        // Body: single [K_EXPR, V_EXPR]
        let (key_expr, val_expr) = htm::match_array_pair(block)?;
        // V must be lvar val_arg
        if !htm::is_lvar_ref(&val_expr, &val_arg) {
            return None;
        }
        // noop
        if htm::is_lvar_ref(&key_expr, &key_arg) {
            return None;
        }
        // transformation_uses_both_args
        if htm::subtree_references(&key_expr, &val_arg) {
            return None;
        }
        // use_transformed_argname
        if !htm::subtree_references(&key_expr, &key_arg) {
            return None;
        }

        let start = recv.location().start_offset();
        let end = block.location().end_offset();
        let msg = "Prefer `transform_keys` over `to_h {...}`.".to_string();
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
    }

    fn check_hash_brackets_map(
        &self,
        outer: &CallNode,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        let (block, _inner_call) = htm::match_hash_brackets_map(outer)?;
        // Simple |k, v|
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
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
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
        // Offense spans from inner call receiver start to outer end.
        let inner_recv = inner_call.receiver()?;
        let start = inner_recv.location().start_offset();
        let end = outer.message_loc().map(|l| l.end_offset()).unwrap_or(outer.location().end_offset());
        let msg = "Prefer `transform_keys` over `map {...}.to_h`.".to_string();
        Some(ctx.offense_with_range(self.name(), &msg, self.severity(), start, end))
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

        // Dispatch: on_send handles `Hash[...]` (method `:[]`) and `map {...}.to_h` (method `:to_h` no block).
        // on_block is a CallNode with a block (method either each_with_object, to_h {...}, etc.)

        if node.block().is_some() {
            let block_node = node.block().unwrap();
            if let Some(block) = block_node.as_block_node() {
                if let Some(o) = self.check_block_node(node, &block, ctx) {
                    return vec![o];
                }
            }
            // Fall through: a `.to_h { ... }` call can also be an outer wrapper
            // of `map {...}.to_h` (where the attached block belongs to the outer to_h).
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
