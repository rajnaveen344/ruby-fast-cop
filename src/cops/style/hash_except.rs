use crate::cops::style::hash_slice::try_check_hash_subset;
use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct HashExcept {
    active_support: bool,
}

impl HashExcept {
    pub fn new() -> Self { Self { active_support: false } }
    pub fn with_config(active_support: bool) -> Self { Self { active_support } }
}

impl Cop for HashExcept {
    fn name(&self) -> &'static str { "Style/HashExcept" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        try_check_hash_subset(
            "Style/HashExcept", self.severity(), node, ctx,
            self.active_support, true, "except", (3, 0),
        )
    }
}

crate::register_cop!("Style/HashExcept", |cfg| {
    let cop_config = cfg.get_cop_config("Style/HashExcept");
    let active_support = cop_config
        .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
        .and_then(|v| v.as_bool())
        .or_else(|| cop_config
            .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
            .and_then(|v| v.as_bool()))
        .unwrap_or(false);
    Some(Box::new(HashExcept::with_config(active_support)))
});
