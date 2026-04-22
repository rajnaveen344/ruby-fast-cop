//! Lint/BooleanSymbol - Checks for `:true` / `:false` symbol literals.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct BooleanSymbol;

impl BooleanSymbol {
    pub fn new() -> Self { Self }
}

fn is_boolean_sym(name: &[u8]) -> bool {
    name == b"true" || name == b"false"
}

impl Cop for BooleanSymbol {
    fn name(&self) -> &'static str { "Lint/BooleanSymbol" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new(), skip_percent_array: false };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    skip_percent_array: bool,
}

impl<'a> Visitor<'a> {
    fn check_sym(&mut self, node: &ruby_prism::SymbolNode) {
        if self.skip_percent_array { return; }

        let unescaped = node.unescaped();
        let name_bytes = unescaped.as_ref();
        if !is_boolean_sym(name_bytes) { return; }

        let name = std::str::from_utf8(name_bytes).unwrap_or("false");
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let src = &self.ctx.source[start..end];

        // Skip if no leading colon (inside %i[] etc.)
        if !src.starts_with(':') { return; }

        let msg = format!("Symbol with a boolean name - you probably meant to use `{}`.", name);
        let correction = Correction::replace(start, end, name.to_string());
        let mut offense = self.ctx.offense_with_range(
            "Lint/BooleanSymbol", &msg, Severity::Warning, start, end,
        );
        offense.correction = Some(correction);
        self.offenses.push(offense);
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode) {
        self.check_sym(node);
        ruby_prism::visit_symbol_node(self, node);
    }

    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
        // Colon-style hash key: `true: val` or `false: val`
        if node.operator_loc().is_none() {
            let key = node.key();
            if let Some(sym) = key.as_symbol_node() {
                let unescaped = sym.unescaped();
                let name_bytes = unescaped.as_ref();
                if is_boolean_sym(name_bytes) {
                    let name = std::str::from_utf8(name_bytes).unwrap_or("false");
                    let key_start = sym.location().start_offset();
                    let key_end = sym.location().end_offset();
                    let src = &self.ctx.source[key_start..key_end];

                    let msg = format!("Symbol with a boolean name - you probably meant to use `{}`.", name);

                    // Find the colon after the key — it's the separator in `foo: val`
                    // The key source is just `true` (no colon). The assoc key ends before colon.
                    // In Prism, SymbolNode for colon-style hash key includes trailing `:`
                    // e.g. `true:` → end_offset covers the colon too.
                    // RuboCop offense range is just the keyword part (not the colon).
                    let offense_end = key_end - 1; // exclude trailing colon

                    // Correction: replace `true: val` portion → `true => val`
                    // We replace from key_start to start of value with `name =>`
                    let val_start = node.value().location().start_offset();
                    let correction = Correction::replace(
                        key_start,
                        val_start,
                        format!("{} => ", name),
                    );

                    let mut offense = self.ctx.offense_with_range(
                        "Lint/BooleanSymbol", &msg, Severity::Warning, key_start, offense_end,
                    );
                    offense.correction = Some(correction);
                    self.offenses.push(offense);

                    // Visit value only (avoid double-offense on key)
                    let val = node.value();
                    self.visit(&val);
                    return;
                }
            }
        }
        ruby_prism::visit_assoc_node(self, node);
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let start = node.location().start_offset();
        let src = &self.ctx.source[start..];
        if src.starts_with("%i") || src.starts_with("%I") {
            // Inside %i/%I — skip children for boolean_symbol check
            let prev = self.skip_percent_array;
            self.skip_percent_array = true;
            ruby_prism::visit_array_node(self, node);
            self.skip_percent_array = prev;
        } else {
            ruby_prism::visit_array_node(self, node);
        }
    }
}

crate::register_cop!("Lint/BooleanSymbol", |_cfg| Some(Box::new(BooleanSymbol::new())));
