//! Style/YAMLFileRead cop
//!
//! `YAML.load(File.read(x))` → `YAML.load_file(x)`. Also covers `safe_load`
//! (Ruby 3.0+) and `parse`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct YAMLFileRead;

impl YAMLFileRead {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for YAMLFileRead {
    fn name(&self) -> &'static str {
        "Style/YAMLFileRead"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node).into_owned();
        if !matches!(method.as_str(), "load" | "safe_load" | "parse") {
            return vec![];
        }
        if method == "safe_load" && ctx.target_ruby_version <= 2.7 {
            return vec![];
        }
        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if !m::is_toplevel_constant_named(&recv, "YAML") {
            return vec![];
        }
        let args_node = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.is_empty() {
            return vec![];
        }
        // First arg must be File.read(path)
        let first_call = match args[0].as_call_node() {
            Some(c) => c,
            None => return vec![],
        };
        if node_name!(first_call) != "read" {
            return vec![];
        }
        let first_recv = match first_call.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if !m::is_toplevel_constant_named(&first_recv, "File") {
            return vec![];
        }
        let first_call_args_node = match first_call.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let first_call_args: Vec<_> = first_call_args_node.arguments().iter().collect();
        if first_call_args.len() != 1 {
            return vec![];
        }
        let path_node = &first_call_args[0];
        let path_loc = path_node.location();
        let path_src = &ctx.source[path_loc.start_offset()..path_loc.end_offset()];

        // Rest arguments = args[1..]
        let rest_src: String = if args.len() > 1 {
            let mut parts = Vec::new();
            for a in &args[1..] {
                let al = a.location();
                parts.push(&ctx.source[al.start_offset()..al.end_offset()]);
            }
            format!(", {}", parts.join(", "))
        } else {
            String::new()
        };

        let prefer = format!("{}_file({}{})", method, path_src, rest_src);
        let msg = format!("Use `{}` instead.", prefer);

        // Range: from selector (method name) to end of node.
        let msg_loc = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let start = msg_loc.start_offset();
        let end = node.location().end_offset();
        vec![ctx
            .offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, prefer))]
    }
}

crate::register_cop!("Style/YAMLFileRead", |_cfg| Some(Box::new(YAMLFileRead::new())));
