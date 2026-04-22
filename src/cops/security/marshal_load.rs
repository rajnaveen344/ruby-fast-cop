//! Security/MarshalLoad cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct MarshalLoad;

impl MarshalLoad {
    pub fn new() -> Self { Self }

    /// Check if node is `Marshal.dump(...)` or `::Marshal.dump(...)`
    fn is_marshal_dump(node: &ruby_prism::Node) -> bool {
        let call = match node {
            ruby_prism::Node::CallNode { .. } => node.as_call_node().unwrap(),
            _ => return false,
        };
        if node_name!(call) != "dump" {
            return false;
        }
        let recv = match call.receiver() {
            Some(r) => r,
            None => return false,
        };
        Self::is_marshal_const(&recv)
    }

    fn is_marshal_const(node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let name = node_name!(node.as_constant_read_node().unwrap());
                name == "Marshal"
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                if cp.parent().is_none() {
                    let child = String::from_utf8_lossy(cp.name_loc().as_slice());
                    child == "Marshal"
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

impl Cop for MarshalLoad {
    fn name(&self) -> &'static str { "Security/MarshalLoad" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "load" && method != "restore" {
            return vec![];
        }

        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if !Self::is_marshal_const(&recv) {
            return vec![];
        }

        // Marshal.load(Marshal.dump(...)) → ok (deep copy idiom)
        if let Some(args) = node.arguments() {
            let first = args.arguments().iter().next();
            if let Some(first_arg) = first {
                if Self::is_marshal_dump(&first_arg) {
                    return vec![];
                }
            }
        }

        let msg = format!("Avoid using `Marshal.{}`.", method);
        vec![ctx.offense(self.name(), &msg, self.severity(), &node.message_loc().unwrap())]
    }
}

crate::register_cop!("Security/MarshalLoad", |_cfg| Some(Box::new(MarshalLoad::new())));
