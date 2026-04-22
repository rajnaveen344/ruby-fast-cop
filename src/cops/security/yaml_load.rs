//! Security/YAMLLoad cop
//! Only active for Ruby < 3.1 (Psych 4 makes YAML.load safe by default)

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct YAMLLoad;

impl YAMLLoad {
    pub fn new() -> Self { Self }

    fn is_yaml_const(node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let name = node_name!(node.as_constant_read_node().unwrap());
                name == "YAML"
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                // ::YAML — parent is None
                if cp.parent().is_none() {
                    let child = String::from_utf8_lossy(cp.name_loc().as_slice());
                    child == "YAML"
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

impl Cop for YAMLLoad {
    fn name(&self) -> &'static str { "Security/YAMLLoad" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only active for Ruby <= 3.0 (Psych 3)
        if ctx.target_ruby_version >= 3.1 {
            return vec![];
        }

        if node_name!(node) != "load" {
            return vec![];
        }

        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if !Self::is_yaml_const(&recv) {
            return vec![];
        }

        vec![ctx.offense(
            self.name(),
            "Prefer using `YAML.safe_load` over `YAML.load`.",
            self.severity(),
            &node.message_loc().unwrap(),
        )]
    }
}

crate::register_cop!("Security/YAMLLoad", |_cfg| Some(Box::new(YAMLLoad::new())));
