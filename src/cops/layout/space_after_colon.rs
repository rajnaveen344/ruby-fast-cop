//! Layout/SpaceAfterColon - Checks for space missing after colon in hash rockets / keyword args.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_after_colon.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

pub struct SpaceAfterColon;

impl Default for SpaceAfterColon {
    fn default() -> Self {
        Self
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_colon_at(&mut self, colon_end: usize) {
        let source = self.ctx.source;
        let bytes = source.as_bytes();
        // colon_end is one past the colon byte
        if colon_end >= bytes.len() {
            return;
        }
        let next = bytes[colon_end];
        // Must be followed by whitespace
        if next != b' ' && next != b'\t' && next != b'\n' && next != b'\r' {
            let colon_start = colon_end - 1;
            let offense = Offense::new(
                "Layout/SpaceAfterColon",
                "Space missing after colon.",
                Severity::Convention,
                Location::from_offsets(source, colon_start, colon_end),
                self.ctx.filename,
            ).with_correction(Correction::insert(colon_end, " "));
            self.offenses.push(offense);
        }
    }
}

impl<'a> Visit<'a> for Visitor<'a> {
    /// Hash pair with colon syntax: {a: 3}
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode<'a>) {
        // colon? style: operator_loc is None for `key: value` style
        // Rocket style `key => value` has operator_loc = `=>`
        // For colon style (no operator_loc or operator == ":"), check
        // But we need the colon after the key.
        // In Prism, {a: 3} has AssocNode with operator_loc = None (colon is part of key symbol)
        // Actually for `{a: 3}`, the key is a SymbolNode `a:`, and operator_loc is None.
        // The colon is the last char of the key's source.
        if node.operator_loc().is_none() {
            // colon-style pair: key ends with ':'
            let key = node.key();
            let key_end = key.location().end_offset();
            let source = self.ctx.source;
            let bytes = source.as_bytes();
            // The char at key_end-1 should be ':'
            if key_end > 0 && bytes.get(key_end - 1).copied() == Some(b':') {
                // Check for value omission (Ruby 3.1+): no value or value same as key
                // value_omission: key and value are same node (same offset)
                let value_start = node.value().location().start_offset();
                let key_start = key.location().start_offset();
                if value_start == key_start {
                    // value omission, skip
                    ruby_prism::visit_assoc_node(self, node);
                    return;
                }
                self.check_colon_at(key_end);
            }
        }
        ruby_prism::visit_assoc_node(self, node);
    }

    /// Optional keyword argument: def m(var:1)
    fn visit_optional_keyword_parameter_node(&mut self, node: &ruby_prism::OptionalKeywordParameterNode<'a>) {
        // name_loc includes the trailing ':' — e.g. for `var:1`, name_loc spans `var:` (end_offset points past ':')
        let name_loc = node.name_loc();
        let source = self.ctx.source;
        let bytes = source.as_bytes();
        let name_end = name_loc.end_offset();
        // Confirm the char before name_end is ':'
        if name_end > 0 && bytes.get(name_end - 1).copied() == Some(b':') {
            self.check_colon_at(name_end);
        }
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
    }
}

impl Cop for SpaceAfterColon {
    fn name(&self) -> &'static str {
        "Layout/SpaceAfterColon"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

crate::register_cop!("Layout/SpaceAfterColon", |_cfg| {
    Some(Box::new(SpaceAfterColon))
});
