//! Naming/AsciiIdentifiers cop
//! Identifiers should use only ASCII characters.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/naming/ascii_identifiers.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Cfg {
    #[serde(default)]
    ascii_constants: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg { ascii_constants: false }
    }
}

pub struct AsciiIdentifiers {
    ascii_constants: bool,
}

impl AsciiIdentifiers {
    pub fn new(ascii_constants: bool) -> Self {
        Self { ascii_constants }
    }
}

impl Cop for AsciiIdentifiers {
    fn name(&self) -> &'static str { "Naming/AsciiIdentifiers" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = AsciiIdentifiersVisitor {
            source: ctx.source,
            ascii_constants: self.ascii_constants,
            offenses: Vec::new(),
            ctx,
        };
        let result = ruby_prism::parse(ctx.source.as_bytes());
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct AsciiIdentifiersVisitor<'a> {
    source: &'a str,
    ascii_constants: bool,
    offenses: Vec<Offense>,
    ctx: &'a CheckContext<'a>,
}

impl AsciiIdentifiersVisitor<'_> {
    /// Check if a name has non-ASCII chars and report the first non-ASCII range
    fn check_name(&mut self, name: &str, name_offset: usize, is_constant: bool) {
        if is_constant && !self.ascii_constants {
            return;
        }
        let msg = if is_constant {
            "Use only ascii symbols in constants."
        } else {
            "Use only ascii symbols in identifiers."
        };

        // Find the first non-ASCII character and report its byte range
        let mut char_offset = 0usize;
        let name_bytes = name.as_bytes();
        for (i, ch) in name.char_indices() {
            if !ch.is_ascii() {
                // Find the end of the non-ASCII sequence (contiguous non-ASCII chars)
                let start = i;
                let mut end = i + ch.len_utf8();
                // Continue while next char is also non-ASCII
                for (j, c2) in name[end..].char_indices() {
                    if c2.is_ascii() { break; }
                    end += c2.len_utf8();
                }
                let _ = char_offset;
                let _ = name_bytes;

                let abs_start = name_offset + start;
                let abs_end = name_offset + end;
                self.offenses.push(self.ctx.offense_with_range(
                    "Naming/AsciiIdentifiers", msg, Severity::Convention,
                    abs_start,
                    abs_end,
                ));
                return; // Only report first non-ASCII segment
            }
            char_offset += ch.len_utf8();
        }
    }
}

impl Visit<'_> for AsciiIdentifiersVisitor<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name_loc = node.name_loc();
        let name = &self.source[name_loc.start_offset()..name_loc.end_offset()];
        self.check_name(name, name_loc.start_offset(), false);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let loc = node.location();
        let name = &self.source[loc.start_offset()..loc.end_offset()];
        self.check_name(name, loc.start_offset(), false);
        ruby_prism::visit_local_variable_read_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let name_loc = node.name_loc();
        let name = &self.source[name_loc.start_offset()..name_loc.end_offset()];
        self.check_name(name, name_loc.start_offset(), false);
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let name_loc = node.name_loc();
        let name = &self.source[name_loc.start_offset()..name_loc.end_offset()];
        self.check_name(name, name_loc.start_offset(), false);
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode) {
        let loc = node.location();
        let name = &self.source[loc.start_offset()..loc.end_offset()];
        self.check_name(name, loc.start_offset(), true);
        ruby_prism::visit_constant_read_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let name_loc = node.name_loc();
        let name = &self.source[name_loc.start_offset()..name_loc.end_offset()];
        self.check_name(name, name_loc.start_offset(), true);
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        // Class name: use constant_path location (covers the last identifier)
        let cp_loc = node.constant_path().location();
        let name = &self.source[cp_loc.start_offset()..cp_loc.end_offset()];
        // Only check the last part of a namespaced constant (e.g. Foo::Bör → "Bör")
        let last = name.rsplit("::").next().unwrap_or(name);
        let last_start = cp_loc.end_offset() - last.len();
        self.check_name(last, last_start, true);
        // Manually visit body and superclass but NOT constant_path (already checked above)
        if let Some(superclass) = node.superclass() {
            self.visit(&superclass);
        }
        if let Some(body) = node.body() {
            self.visit(&body);
        }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let cp_loc = node.constant_path().location();
        let name = &self.source[cp_loc.start_offset()..cp_loc.end_offset()];
        let last = name.rsplit("::").next().unwrap_or(name);
        let last_start = cp_loc.end_offset() - last.len();
        self.check_name(last, last_start, true);
        // Visit body but NOT constant_path (already checked above)
        if let Some(body) = node.body() {
            self.visit(&body);
        }
    }
}

crate::register_cop!("Naming/AsciiIdentifiers", |cfg| {
    let c: Cfg = cfg.typed("Naming/AsciiIdentifiers");
    Some(Box::new(AsciiIdentifiers::new(c.ascii_constants)))
});
