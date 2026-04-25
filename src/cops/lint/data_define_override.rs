//! Lint/DataDefineOverride cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

const METHOD_NAMES: &[&str] = &[
    "!", "!=", "!~", "<=>", "==", "===", "__id__", "__send__", "class", "clone",
    "deconstruct", "deconstruct_keys", "define_singleton_method", "display", "dup",
    "enum_for", "eql?", "equal?", "extend", "freeze", "frozen?", "hash", "inspect",
    "instance_eval", "instance_exec", "instance_of?", "instance_variable_defined?",
    "instance_variable_get", "instance_variable_set", "instance_variables", "is_a?",
    "itself", "kind_of?", "members", "method", "methods", "nil?", "object_id",
    "private_methods", "protected_methods", "public_method", "public_methods",
    "public_send", "remove_instance_variable", "respond_to?", "send", "singleton_class",
    "singleton_method", "singleton_methods", "tap", "then", "to_enum", "to_h", "to_s",
    "with", "yield_self",
];

#[derive(Default)]
pub struct DataDefineOverride;

impl DataDefineOverride {
    pub fn new() -> Self { Self }
}

impl Cop for DataDefineOverride {
    fn name(&self) -> &'static str { "Lint/DataDefineOverride" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node).as_ref() != "define" { return vec![]; }
        let recv = match node.receiver() { Some(r) => r, None => return vec![] };
        let is_data = if let Some(c) = recv.as_constant_read_node() {
            String::from_utf8_lossy(c.name().as_slice()) == "Data"
        } else if let Some(cp) = recv.as_constant_path_node() {
            cp.parent().is_none()
                && cp.name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()) == "Data")
                    .unwrap_or(false)
        } else {
            false
        };
        if !is_data { return vec![]; }

        let args = match node.arguments() { Some(a) => a, None => return vec![] };
        let mut out = vec![];
        for arg in args.arguments().iter() {
            let (text, name_str, loc) = if let Some(s) = arg.as_symbol_node() {
                let loc = s.location();
                let vloc = match s.value_loc() { Some(v) => v, None => continue };
                let name = ctx.source[vloc.start_offset()..vloc.end_offset()].to_string();
                let text = format!(":{}", name);
                (text, name, loc)
            } else if let Some(st) = arg.as_string_node() {
                let loc = st.location();
                let vloc = st.content_loc();
                let name = ctx.source[vloc.start_offset()..vloc.end_offset()].to_string();
                let text = format!("\"{}\"", name);
                (text, name, loc)
            } else {
                continue;
            };
            if METHOD_NAMES.iter().any(|&m| m == name_str) {
                let msg = format!(
                    "`{}` member overrides `Data#{}` and it may be unexpected.",
                    text, name_str
                );
                out.push(ctx.offense_with_range(
                    "Lint/DataDefineOverride", &msg, Severity::Warning,
                    loc.start_offset(), loc.end_offset(),
                ));
            }
        }
        out
    }
}

crate::register_cop!("Lint/DataDefineOverride", |_cfg| Some(Box::new(DataDefineOverride::new())));
