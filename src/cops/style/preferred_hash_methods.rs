//! Style/PreferredHashMethods cop
//!
//! Enforces use of Hash#key?/value? (short) or Hash#has_key?/has_value? (verbose).

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::CallNode;

#[derive(Default)]
pub struct PreferredHashMethods {
    use_short: bool, // true = short style (default), false = verbose
}

impl PreferredHashMethods {
    pub fn new(use_short: bool) -> Self {
        Self { use_short }
    }
}

impl Cop for PreferredHashMethods {
    fn name(&self) -> &'static str {
        "Style/PreferredHashMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Must have receiver and exactly 1 argument
        if node.receiver().is_none() {
            return vec![];
        }
        let arg_count = node.arguments().map(|a| a.arguments().len()).unwrap_or(0);
        if arg_count != 1 {
            return vec![];
        }

        let method = node_name!(node);
        let method_loc = node.message_loc().unwrap_or_else(|| node.location());

        let (bad, good) = if self.use_short {
            // short style: flag has_key? -> key?, has_value? -> value?
            match method.as_ref() {
                "has_key?" => ("has_key?", "key?"),
                "has_value?" => ("has_value?", "value?"),
                _ => return vec![],
            }
        } else {
            // verbose style: flag key? -> has_key?, value? -> has_value?
            match method.as_ref() {
                "key?" => ("key?", "has_key?"),
                "value?" => ("value?", "has_value?"),
                _ => return vec![],
            }
        };

        let msg = format!("Use `Hash#{}` instead of `Hash#{}`.", good, bad);
        vec![ctx.offense(self.name(), &msg, self.severity(), &method_loc)]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

crate::register_cop!("Style/PreferredHashMethods", |cfg| {
    let c: Cfg = cfg.typed("Style/PreferredHashMethods");
    let use_short = c.enforced_style != "verbose";
    Some(Box::new(PreferredHashMethods::new(use_short)))
});
