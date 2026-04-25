//! Style/OptionHash — discourages options hashes in favor of keyword arguments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Prefer keyword arguments to options hashes.";

#[derive(Default)]
pub struct OptionHash {
    allowlist: Vec<String>,
    suspicious_param_names: Vec<String>,
}

impl OptionHash {
    pub fn new() -> Self {
        Self {
            allowlist: Vec::new(),
            suspicious_param_names: vec!["options".to_string()],
        }
    }

    pub fn with_config(allowlist: Vec<String>, suspicious_param_names: Vec<String>) -> Self {
        Self { allowlist, suspicious_param_names }
    }
}

impl Cop for OptionHash {
    fn name(&self) -> &'static str {
        "Style/OptionHash"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
        if self.allowlist.iter().any(|m| m == &method_name) {
            return vec![];
        }

        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![],
        };
        let optionals: Vec<_> = params.optionals().iter().collect();
        if optionals.is_empty() {
            return vec![];
        }
        // Last optional only
        let last = optionals.last().unwrap();
        let opt = match last.as_optional_parameter_node() {
            Some(o) => o,
            None => return vec![],
        };
        let pname = String::from_utf8_lossy(opt.name().as_slice()).into_owned();
        if !self.suspicious_param_names.iter().any(|n| n == &pname) {
            return vec![];
        }
        // Default value must be empty hash literal
        let value = opt.value();
        let hash = match value.as_hash_node() {
            Some(h) => h,
            None => return vec![],
        };
        if hash.elements().iter().count() != 0 {
            return vec![];
        }

        // Skip if body uses bare `super` (forwarding_super / zsuper).
        if let Some(body) = node.body() {
            if contains_forwarding_super(&body) {
                return vec![];
            }
        }

        let oloc = last.location();
        vec![ctx.offense_with_range(
            self.name(),
            MSG,
            self.severity(),
            oloc.start_offset(),
            oloc.end_offset(),
        )]
    }
}

fn contains_forwarding_super(node: &Node) -> bool {
    struct V {
        found: bool,
    }
    impl<'a> Visit<'a> for V {
        fn visit_forwarding_super_node(&mut self, _n: &ruby_prism::ForwardingSuperNode) {
            self.found = true;
        }
    }
    let mut v = V { found: false };
    v.visit(node);
    v.found
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowlist: Vec<String>,
    suspicious_param_names: Option<Vec<String>>,
}

crate::register_cop!("Style/OptionHash", |cfg| {
    let c: Cfg = cfg.typed("Style/OptionHash");
    let suspicious = c.suspicious_param_names.unwrap_or_else(|| vec!["options".to_string()]);
    Some(Box::new(OptionHash::with_config(c.allowlist, suspicious)))
});
