//! Lint/HashCompareByIdentity cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/hash_compare_by_identity.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct HashCompareByIdentity;

impl HashCompareByIdentity {
    pub fn new() -> Self { Self }
}

const HASH_METHODS: &[&str] = &["key?", "has_key?", "fetch", "[]", "[]="];
const MSG: &str = "Use `Hash#compare_by_identity` instead of using `object_id` for keys.";

impl Cop for HashCompareByIdentity {
    fn name(&self) -> &'static str { "Lint/HashCompareByIdentity" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if !HASH_METHODS.iter().any(|m| *m == method) {
            return vec![];
        }

        // Check if any argument ends with .object_id or is itself `object_id`
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };

        let has_object_id_arg = args.arguments().iter().any(|arg| {
            // arg is: foo.object_id (CallNode with method object_id)
            // or just: object_id (CallNode with no receiver, method object_id)
            if let Some(call) = arg.as_call_node() {
                node_name!(call) == "object_id"
            } else {
                false
            }
        });

        if !has_object_id_arg {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        vec![ctx.offense_with_range(
            "Lint/HashCompareByIdentity",
            MSG,
            Severity::Warning,
            start,
            end,
        )]
    }
}

crate::register_cop!("Lint/HashCompareByIdentity", |_cfg| {
    Some(Box::new(HashCompareByIdentity::new()))
});
