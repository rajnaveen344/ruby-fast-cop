//! Lint/HashNewWithKeywordArgumentsAsDefault cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};

const MSG: &str = "Use a hash literal instead of keyword arguments.";

#[derive(Default)]
pub struct HashNewWithKeywordArgumentsAsDefault;

impl HashNewWithKeywordArgumentsAsDefault {
    pub fn new() -> Self { Self }
}

impl Cop for HashNewWithKeywordArgumentsAsDefault {
    fn name(&self) -> &'static str { "Lint/HashNewWithKeywordArgumentsAsDefault" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node).as_ref() != "new" { return vec![]; }
        let recv = match node.receiver() { Some(r) => r, None => return vec![] };
        let is_hash = if let Some(c) = recv.as_constant_read_node() {
            String::from_utf8_lossy(c.name().as_slice()) == "Hash"
        } else if let Some(cp) = recv.as_constant_path_node() {
            cp.parent().is_none()
                && cp.name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()) == "Hash")
                    .unwrap_or(false)
        } else {
            false
        };
        if !is_hash { return vec![]; }

        let args = match node.arguments() { Some(a) => a, None => return vec![] };
        let arg_vec: Vec<_> = args.arguments().iter().collect();
        if arg_vec.len() != 1 { return vec![]; }
        let kh = match arg_vec[0].as_keyword_hash_node() { Some(k) => k, None => return vec![] };
        let pairs: Vec<_> = kh.elements().iter().collect();
        if pairs.len() == 1 {
            if let Some(assoc) = pairs[0].as_assoc_node() {
                if let Some(sym) = assoc.key().as_symbol_node() {
                    if let Some(vloc) = sym.value_loc() {
                        if &ctx.source[vloc.start_offset()..vloc.end_offset()] == "capacity" {
                            return vec![];
                        }
                    }
                }
            }
        }
        let loc = kh.location();
        let s = loc.start_offset();
        let e = loc.end_offset();
        let off = ctx.offense_with_range(
            "Lint/HashNewWithKeywordArgumentsAsDefault", MSG, Severity::Warning, s, e,
        ).with_correction(Correction { edits: vec![
            Edit { start_offset: s, end_offset: s, replacement: "{".to_string() },
            Edit { start_offset: e, end_offset: e, replacement: "}".to_string() },
        ]});
        vec![off]
    }
}

crate::register_cop!("Lint/HashNewWithKeywordArgumentsAsDefault", |_cfg| Some(Box::new(HashNewWithKeywordArgumentsAsDefault::new())));
