//! Style/StringHashKeys — prefer symbols instead of strings as hash keys.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Prefer symbols instead of strings as hash keys.";

#[derive(Default)]
pub struct StringHashKeys;

impl StringHashKeys {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for StringHashKeys {
    fn name(&self) -> &'static str {
        "Style/StringHashKeys"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, offenses: Vec::new(), call_stack: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    call_stack: Vec<String>,
}

impl<'a> Visitor<'a> {
    fn in_exempt_call(&self) -> bool {
        for name in self.call_stack.iter().rev() {
            match name.as_str() {
                "popen" | "capture2" | "capture2e" | "capture3" | "popen2" | "popen2e"
                | "popen3" | "pipeline" | "pipeline_r" | "pipeline_rw" | "pipeline_start"
                | "pipeline_w" | "spawn" | "system" | "gsub" | "gsub!" => return true,
                _ => {}
            }
        }
        false
    }

    fn check_assoc(&mut self, assoc: &ruby_prism::AssocNode) {
        let key = assoc.key();
        let snode = match &key {
            Node::StringNode { .. } => key.as_string_node().unwrap(),
            _ => return,
        };

        let unescaped = snode.unescaped();
        let content = match std::str::from_utf8(unescaped) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        };

        let key_loc = key.location();
        let kstart = key_loc.start_offset();
        let kend = key_loc.end_offset();

        let replacement = if is_simple_symbol(&content) {
            format!(":{}", content)
        } else {
            format!(":\"{}\"", content)
        };

        let off = self
            .ctx
            .offense_with_range(
                "Style/StringHashKeys",
                MSG,
                Severity::Convention,
                kstart,
                kend,
            )
            .with_correction(Correction::replace(kstart, kend, replacement));
        self.offenses.push(off);
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
        self.call_stack.push(name);
        ruby_prism::visit_call_node(self, node);
        self.call_stack.pop();
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        if !self.in_exempt_call() {
            for el in node.elements().iter() {
                if let Some(assoc) = el.as_assoc_node() {
                    self.check_assoc(&assoc);
                }
            }
        }
        ruby_prism::visit_hash_node(self, node);
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        if !self.in_exempt_call() {
            for el in node.elements().iter() {
                if let Some(assoc) = el.as_assoc_node() {
                    self.check_assoc(&assoc);
                }
            }
        }
        ruby_prism::visit_keyword_hash_node(self, node);
    }
}

fn is_simple_symbol(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

crate::register_cop!("Style/StringHashKeys", |_cfg| Some(Box::new(StringHashKeys::new())));
