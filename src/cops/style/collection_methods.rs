//! Style/CollectionMethods - Enforces consistent Enumerable method names.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/collection_methods.rb
//! Mixin: lib/rubocop/cop/mixin/method_preference.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use std::collections::{HashMap, HashSet};

pub struct CollectionMethods {
    preferred_methods: HashMap<String, String>,
    methods_accepting_symbol: HashSet<String>,
}

impl Default for CollectionMethods {
    fn default() -> Self {
        let mut preferred_methods = HashMap::new();
        for (k, v) in [
            ("collect", "map"),
            ("collect!", "map!"),
            ("inject", "reduce"),
            ("detect", "find"),
            ("find_all", "select"),
            ("member?", "include?"),
        ] {
            preferred_methods.insert(k.into(), v.into());
        }
        Self {
            preferred_methods,
            methods_accepting_symbol: HashSet::new(),
        }
    }
}

impl CollectionMethods {
    pub fn new(
        preferred_methods: HashMap<String, String>,
        methods_accepting_symbol: HashSet<String>,
    ) -> Self {
        Self {
            preferred_methods,
            methods_accepting_symbol,
        }
    }

    fn implicit_block(&self, node: &ruby_prism::CallNode) -> bool {
        let Some(args) = node.arguments() else {
            return false;
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        let Some(last) = arg_list.last() else {
            return false;
        };
        // Block-pass `&:foo` always counts.
        if matches!(last, ruby_prism::Node::BlockArgumentNode { .. }) {
            return true;
        }
        // Bare symbol counts only for methods accepting symbol.
        if matches!(last, ruby_prism::Node::SymbolNode { .. }) {
            let method = node_name!(node);
            return self.methods_accepting_symbol.contains(method.as_ref());
        }
        false
    }
}

impl Cop for CollectionMethods {
    fn name(&self) -> &'static str {
        "Style/CollectionMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, _ctx: &CheckContext) -> Vec<Offense> {
        // Either has a block (literal {} / do-end), or implicit block via last arg.
        let has_block = node.block().is_some();
        if !has_block && !self.implicit_block(node) {
            return vec![];
        }

        let method_name = node_name!(node);
        let Some(preferred) = self.preferred_methods.get(method_name.as_ref()) else {
            return vec![];
        };

        let Some(selector) = node.message_loc() else {
            return vec![];
        };
        let start = selector.start_offset();
        let end = selector.end_offset();
        let message = format!("Prefer `{}` over `{}`.", preferred, method_name);
        let correction = Correction::replace(start, end, preferred.clone());
        let location = crate::offense::Location::from_offsets(_ctx.source, start, end);
        let offense = Offense::new(self.name(), &message, self.severity(), location, _ctx.filename)
            .with_correction(correction);
        vec![offense]
    }
}

crate::register_cop!("Style/CollectionMethods", |cfg| {
    let mut preferred = HashMap::new();
    for (k, v) in [
        ("collect", "map"),
        ("collect!", "map!"),
        ("inject", "reduce"),
        ("detect", "find"),
        ("find_all", "select"),
        ("member?", "include?"),
    ] {
        preferred.insert(k.to_string(), v.to_string());
    }
    let mut accept = HashSet::new();

    if let Some(cc) = cfg.get_cop_config("Style/CollectionMethods") {
        if let Some(serde_yaml::Value::Mapping(m)) = cc.raw.get("PreferredMethods") {
            preferred.clear();
            for (k, v) in m {
                if let (Some(ks), Some(vs)) = (k.as_str(), v.as_str()) {
                    preferred.insert(ks.to_string(), vs.to_string());
                }
            }
        }
        if let Some(serde_yaml::Value::Sequence(seq)) = cc.raw.get("MethodsAcceptingSymbol") {
            for v in seq {
                if let Some(s) = v.as_str() {
                    accept.insert(s.to_string());
                }
            }
        }
    }
    Some(Box::new(CollectionMethods::new(preferred, accept)))
});
