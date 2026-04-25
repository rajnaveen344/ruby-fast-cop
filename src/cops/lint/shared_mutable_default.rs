//! Lint/SharedMutableDefault cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Do not create a Hash with a mutable default value as the default value can accidentally be changed.";

#[derive(Default)]
pub struct SharedMutableDefault;

impl SharedMutableDefault {
    pub fn new() -> Self { Self }
}

impl Cop for SharedMutableDefault {
    fn name(&self) -> &'static str { "Lint/SharedMutableDefault" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node).as_ref() != "new" { return vec![]; }
        let recv = match node.receiver() { Some(r) => r, None => return vec![] };
        let cr = match recv.as_constant_read_node() { Some(c) => c, None => return vec![] };
        if String::from_utf8_lossy(cr.name().as_slice()) != "Hash" { return vec![] }
        if node.block().is_some() { return vec![]; }
        let args = match node.arguments() { Some(a) => a, None => return vec![] };
        let first = match args.arguments().iter().next() { Some(f) => f, None => return vec![] };
        if !is_mutable_default(&first) { return vec![]; }
        let loc = node.location();
        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), loc.start_offset(), loc.end_offset())]
    }
}

fn is_mutable_default(node: &Node) -> bool {
    if node.as_array_node().is_some() { return true; }
    if node.as_hash_node().is_some() { return true; }
    if let Some(call) = node.as_call_node() {
        let name = String::from_utf8_lossy(call.name().as_slice()).to_string();
        if name == "freeze" {
            return false;
        }
        if name == "new" {
            if let Some(recv) = call.receiver() {
                if let Some(cr) = recv.as_constant_read_node() {
                    let n = String::from_utf8_lossy(cr.name().as_slice()).to_string();
                    if n == "Array" || n == "Hash" { return true; }
                }
            }
        }
    }
    false
}

crate::register_cop!("Lint/SharedMutableDefault", |_cfg| Some(Box::new(SharedMutableDefault::new())));
