//! Security/JSONLoad cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct JSONLoad;

impl JSONLoad {
    pub fn new() -> Self { Self }

    fn is_json_const(node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let name = node_name!(node.as_constant_read_node().unwrap());
                name == "JSON"
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                // ::JSON — no parent
                if cp.parent().is_none() {
                    let child = String::from_utf8_lossy(cp.name_loc().as_slice());
                    child == "JSON"
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Check if `create_additions:` kwarg is present anywhere in arguments
    fn has_create_additions(node: &ruby_prism::CallNode) -> bool {
        let args = match node.arguments() {
            Some(a) => a,
            None => return false,
        };
        for arg in args.arguments().iter() {
            if Self::kwarg_has_create_additions(&arg) {
                return true;
            }
        }
        false
    }

    fn kwarg_has_create_additions(node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::KeywordHashNode { .. } => {
                let kh = node.as_keyword_hash_node().unwrap();
                for elem in kh.elements().iter() {
                    if let ruby_prism::Node::AssocNode { .. } = elem {
                        let assoc = elem.as_assoc_node().unwrap();
                        if let ruby_prism::Node::SymbolNode { .. } = assoc.key() {
                            let sym = assoc.key().as_symbol_node().unwrap();
                            let key = String::from_utf8_lossy(sym.unescaped());
                            if key == "create_additions" {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            ruby_prism::Node::HashNode { .. } => {
                let h = node.as_hash_node().unwrap();
                for elem in h.elements().iter() {
                    if let ruby_prism::Node::AssocNode { .. } = elem {
                        let assoc = elem.as_assoc_node().unwrap();
                        if let ruby_prism::Node::SymbolNode { .. } = assoc.key() {
                            let sym = assoc.key().as_symbol_node().unwrap();
                            let key = String::from_utf8_lossy(sym.unescaped());
                            if key == "create_additions" {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }
}

impl Cop for JSONLoad {
    fn name(&self) -> &'static str { "Security/JSONLoad" }
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
        if !Self::is_json_const(&recv) {
            return vec![];
        }

        // If create_additions kwarg present → safe
        if Self::has_create_additions(node) {
            return vec![];
        }

        let msg = format!("Prefer `JSON.parse` over `JSON.{}`.", method);
        vec![ctx.offense(self.name(), &msg, self.severity(), &node.message_loc().unwrap())]
    }
}

crate::register_cop!("Security/JSONLoad", |_cfg| Some(Box::new(JSONLoad::new())));
