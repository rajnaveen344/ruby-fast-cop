//! Security/Open cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct Open;

impl Open {
    pub fn new() -> Self { Self }

    /// Mirrors RuboCop's `safe?` — returns true if argument is safe (literal non-pipe string)
    fn is_safe(node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::StringNode { .. } => {
                // simple string literal — check it doesn't start with '|'
                let s = node.as_string_node().unwrap();
                let val = String::from_utf8_lossy(s.unescaped());
                !val.is_empty() && !val.starts_with('|')
            }
            ruby_prism::Node::InterpolatedStringNode { .. } => {
                // composite: check first child
                let n = node.as_interpolated_string_node().unwrap();
                let first = n.parts().iter().next();
                match first {
                    Some(f) => Self::is_safe(&f),
                    None => true, // empty interpolated string
                }
            }
            ruby_prism::Node::CallNode { .. } => {
                // concatenated string: receiver.str_type? && method == :+
                let call = node.as_call_node().unwrap();
                if node_name!(call) == "+" {
                    if let Some(recv) = call.receiver() {
                        if matches!(recv, ruby_prism::Node::StringNode { .. }) {
                            return Self::is_safe(&recv);
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Format receiver prefix for message: "Kernel#", "URI.", "::URI."
    fn receiver_prefix(recv: &ruby_prism::Node, source: &str) -> String {
        let loc = recv.location();
        let text = &source[loc.start_offset()..loc.end_offset()];
        format!("{}.", text)
    }
}

impl Cop for Open {
    fn name(&self) -> &'static str { "Security/Open" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "open" {
            return vec![];
        }

        let receiver = node.receiver();

        // Determine if this is a flaggable call:
        // - bare open(...) with no receiver → Kernel#open
        // - URI.open(...) or ::URI.open(...)
        // - anything else (File.open, IO.popen, etc.) → skip
        let (is_flaggable, receiver_prefix) = match &receiver {
            None => (true, "Kernel#".to_string()),
            Some(recv) => {
                match recv {
                    ruby_prism::Node::ConstantReadNode { .. } => {
                        let name = node_name!(recv.as_constant_read_node().unwrap());
                        if name == "URI" {
                            (true, format!("{}.", name))
                        } else {
                            (false, String::new())
                        }
                    }
                    ruby_prism::Node::ConstantPathNode { .. } => {
                        let cp = recv.as_constant_path_node().unwrap();
                        if cp.parent().is_none() {
                            let child = String::from_utf8_lossy(cp.name_loc().as_slice());
                            if child == "URI" {
                                let loc = recv.location();
                                let text = &ctx.source[loc.start_offset()..loc.end_offset()];
                                (true, format!("{}.", text))
                            } else {
                                (false, String::new())
                            }
                        } else {
                            (false, String::new())
                        }
                    }
                    _ => (false, String::new()),
                }
            }
        };

        if !is_flaggable {
            return vec![];
        }

        // Must have at least one argument
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let first_arg = match args.arguments().iter().next() {
            Some(a) => a,
            None => return vec![],
        };

        if Self::is_safe(&first_arg) {
            return vec![];
        }

        let msg = format!("The use of `{}open` is a serious security risk.", receiver_prefix);
        vec![ctx.offense(self.name(), &msg, self.severity(), &node.message_loc().unwrap())]
    }
}

crate::register_cop!("Security/Open", |_cfg| Some(Box::new(Open::new())));
