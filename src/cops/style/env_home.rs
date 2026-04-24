//! Style/EnvHome cop
//!
//! Flags `ENV['HOME']` and `ENV.fetch('HOME')` / `ENV.fetch('HOME', nil)` in
//! favor of `Dir.home`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Use `Dir.home` instead.";

#[derive(Default)]
pub struct EnvHome;

impl EnvHome {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EnvHome {
    fn name(&self) -> &'static str {
        "Style/EnvHome"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "[]" && method != "fetch" {
            return vec![];
        }

        // Receiver must be `ENV` or `::ENV`.
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if !m::is_toplevel_constant_named(&receiver, "ENV") {
            return vec![];
        }

        // First argument must be string literal "HOME".
        let args_node = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.is_empty() {
            return vec![];
        }
        let first = &args[0];
        let s = match first.as_string_node() {
            Some(s) => s,
            None => return vec![],
        };
        if String::from_utf8_lossy(s.unescaped()) != "HOME" {
            return vec![];
        }

        // 2-arg fetch: second must be nil to flag.
        if args.len() == 2 {
            if !matches!(&args[1], Node::NilNode { .. }) {
                return vec![];
            }
        }
        if args.len() > 2 {
            return vec![];
        }

        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        vec![ctx
            .offense_with_range(self.name(), MSG, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, "Dir.home"))]
    }
}

crate::register_cop!("Style/EnvHome", |_cfg| Some(Box::new(EnvHome::new())));
