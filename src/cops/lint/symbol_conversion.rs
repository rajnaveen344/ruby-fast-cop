//! Lint/SymbolConversion - Checks for unnecessary symbol conversions.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct SymbolConversion {
    style: Style,
}

#[derive(PartialEq, Clone, Copy)]
enum Style {
    Strict,
    Consistent,
}

impl SymbolConversion {
    pub fn new(style: Style) -> Self { Self { style } }
}

impl Default for SymbolConversion {
    fn default() -> Self { Self::new(Style::Strict) }
}

fn is_simple_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let bytes = s.as_bytes();
    let first = bytes[0] as char;
    if !first.is_alphabetic() && first != '_' { return false; }
    let last = *bytes.last().unwrap() as char;
    let core = if last == '!' || last == '?' || last == '=' {
        &s[..s.len() - 1]
    } else {
        s
    };
    core.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Whether a symbol value can be used as a bare colon-style hash key (e.g. `foo:`, `foo!:`, `foo?:`)
/// Hash keys cannot end with `=` (unlike method names).
fn is_valid_hash_key_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let bytes = s.as_bytes();
    let first = bytes[0] as char;
    if !first.is_alphabetic() && first != '_' { return false; }
    let last = *bytes.last().unwrap() as char;
    // Hash keys allow ! and ? suffixes but NOT =
    let core = if last == '!' || last == '?' {
        &s[..s.len() - 1]
    } else {
        s
    };
    core.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn requires_quotes(value: &str) -> bool {
    if value.is_empty() { return false; }
    !is_simple_identifier(value)
}

/// Whether a symbol value requires quoting when used as a hash colon-style key.
fn hash_key_requires_quotes(value: &str) -> bool {
    if value.is_empty() { return false; }
    !is_valid_hash_key_identifier(value)
}

fn symbol_inspect(value: &str) -> String {
    if is_simple_identifier(value) {
        format!(":{}", value)
    } else {
        format!(":\"{}\"", value)
    }
}

impl Cop for SymbolConversion {
    fn name(&self) -> &'static str { "Lint/SymbolConversion" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { cop: self, ctx, offenses: Vec::new(), in_percent_array: false };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    cop: &'a SymbolConversion,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_percent_array: bool,
}

impl<'a> Visitor<'a> {
    fn sym_value(node: &ruby_prism::SymbolNode) -> String {
        String::from_utf8_lossy(node.unescaped().as_ref()).to_string()
    }

    fn check_to_sym_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if method.as_ref() != "to_sym" && method.as_ref() != "intern" {
            return;
        }
        let recv = match node.receiver() {
            Some(r) => r,
            None => return,
        };
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        let correction = match &recv {
            Node::SymbolNode { .. } => {
                let sym = recv.as_symbol_node().unwrap();
                let value = Self::sym_value(&sym);
                symbol_inspect(&value)
            }
            Node::StringNode { .. } => {
                let s = recv.as_string_node().unwrap();
                let value = String::from_utf8_lossy(s.unescaped().as_ref()).to_string();
                symbol_inspect(&value)
            }
            Node::InterpolatedStringNode { .. } => {
                let recv_src = &self.ctx.source[recv.location().start_offset()..recv.location().end_offset()];
                let inner = if (recv_src.starts_with('"') && recv_src.ends_with('"'))
                    || (recv_src.starts_with('\'') && recv_src.ends_with('\''))
                {
                    &recv_src[1..recv_src.len() - 1]
                } else {
                    recv_src
                };
                format!(":\"{inner}\"")
            }
            _ => return,
        };

        let msg = format!("Unnecessary symbol conversion; use `{}` instead.", correction);
        let mut offense = self.ctx.offense_with_range(
            "Lint/SymbolConversion", &msg, Severity::Warning, start, end,
        );
        offense.correction = Some(Correction::replace(start, end, correction));
        self.offenses.push(offense);
    }

    fn check_standalone_symbol(&mut self, node: &ruby_prism::SymbolNode) {
        if self.in_percent_array { return; }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let src = &self.ctx.source[start..end];

        // Must start with `:` to be a standalone symbol (not inside %i[])
        if !src.starts_with(':') { return; }

        let value = Self::sym_value(node);

        // Skip if value doesn't start with alphanumeric/underscore
        if !value.chars().next().map_or(false, |c| c.is_alphanumeric() || c == '_') {
            return;
        }

        // If the value requires quoting (e.g. `foo-bar`, `Foo/Bar`), any quoted form is acceptable.
        // Only flag symbols that DON'T require quotes but are unnecessarily quoted.
        if requires_quotes(&value) { return; }

        let inspect = symbol_inspect(&value);
        if src == inspect { return; } // already correct

        let msg = format!("Unnecessary symbol conversion; use `{}` instead.", inspect);
        let mut offense = self.ctx.offense_with_range(
            "Lint/SymbolConversion", &msg, Severity::Warning, start, end,
        );
        offense.correction = Some(Correction::replace(start, end, inspect));
        self.offenses.push(offense);
    }

    /// Check hash colon-key in strict mode: `'foo': val` → `foo: val`
    fn check_hash_colon_key_strict(&mut self, sym: &ruby_prism::SymbolNode) {
        let value = Self::sym_value(sym);

        if !value.chars().next().map_or(false, |c| c.is_alphanumeric() || c == '_') {
            return;
        }

        let start = sym.location().start_offset();
        // Prism includes the trailing `:` in SymbolNode location for colon-style hash keys.
        // Offense range excludes the colon — subtract 1.
        let prism_end = sym.location().end_offset();
        let offense_end = prism_end - 1;
        let src = &self.ctx.source[start..prism_end];

        // Only flag if the value does NOT require quotes but IS quoted (unnecessary quoting)
        if !hash_key_requires_quotes(&value) && (src.starts_with('\'') || src.starts_with('"')) {
            let correction_key = format!("{}:", value);
            let msg = format!("Unnecessary symbol conversion; use `{}` instead.", correction_key);

            // Correction replaces from key start through the colon (prism_end includes colon)
            let mut offense = self.ctx.offense_with_range(
                "Lint/SymbolConversion", &msg, Severity::Warning, start, offense_end,
            );
            offense.correction = Some(Correction::replace(start, prism_end, format!("{}:", value)));
            self.offenses.push(offense);
        }
    }

    fn check_consistent_hash(&mut self, node: &ruby_prism::HashNode) {
        let elems: Vec<_> = node.elements().iter().collect();
        let mut colon_keys: Vec<ruby_prism::SymbolNode> = Vec::new();
        let mut any_requires_quotes = false;
        let mut has_double_quoted = false;
        let mut has_single_quoted = false;

        for elem in &elems {
            if let Some(assoc) = elem.as_assoc_node() {
                if assoc.operator_loc().is_none() {
                    let key = assoc.key();
                    if let Some(sym) = key.as_symbol_node() {
                        let value = Self::sym_value(&sym);
                        let prism_end = sym.location().end_offset();
                        let key_start = sym.location().start_offset();
                        let key_src = &self.ctx.source[key_start..prism_end];
                        if hash_key_requires_quotes(&value) {
                            any_requires_quotes = true;
                            if key_src.starts_with('"') { has_double_quoted = true; }
                            if key_src.starts_with('\'') { has_single_quoted = true; }
                        }
                        colon_keys.push(sym);
                    }
                } else {
                    // Rocket-style: if there's a string key, skip consistent check
                    let key = assoc.key();
                    if matches!(key, Node::StringNode { .. } | Node::InterpolatedStringNode { .. }) {
                        return; // mixed string/symbol keys — skip
                    }
                }
            }
        }

        // Mixed quote styles: don't flag
        if has_double_quoted && has_single_quoted {
            return;
        }

        if any_requires_quotes {
            // Quote all unquoted keys that don't already have quotes
            for sym in &colon_keys {
                let value = Self::sym_value(sym);
                let prism_end = sym.location().end_offset();
                let offense_end = prism_end - 1; // exclude trailing `:` from offense range
                let start = sym.location().start_offset();
                let src = &self.ctx.source[start..prism_end];
                // Skip if already quoted or if it requires quotes (already consistent)
                if hash_key_requires_quotes(&value) { continue; }
                if src.starts_with('\'') || src.starts_with('"') { continue; }

                let correction_key = format!("\"{}\":", value);
                let msg = format!("Symbol hash key should be quoted for consistency; use `{}` instead.", correction_key);

                let mut offense = self.ctx.offense_with_range(
                    "Lint/SymbolConversion", &msg, Severity::Warning, start, offense_end,
                );
                // Correction replaces key+colon (prism_end includes trailing colon)
                offense.correction = Some(Correction::replace(start, prism_end, format!("\"{}\":", value)));
                self.offenses.push(offense);
            }
        } else {
            // No keys require quotes — treat like strict (remove unnecessary quoting)
            for sym in colon_keys {
                self.check_hash_colon_key_strict(&sym);
            }
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_to_sym_call(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode) {
        self.check_standalone_symbol(node);
        ruby_prism::visit_symbol_node(self, node);
    }

    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
        if node.operator_loc().is_none() {
            let key = node.key();
            if let Some(sym) = key.as_symbol_node() {
                if self.cop.style == Style::Strict {
                    self.check_hash_colon_key_strict(&sym);
                }
                // Visit value only — don't let visit_symbol_node re-check the key
                let val = node.value();
                self.visit(&val);
                return;
            }
        }
        ruby_prism::visit_assoc_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        if self.cop.style == Style::Consistent {
            self.check_consistent_hash(node);
            // Still recurse but skip assoc keys via visit_assoc_node
        }
        ruby_prism::visit_hash_node(self, node);
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let start = node.location().start_offset();
        let src = &self.ctx.source[start..];
        if src.starts_with("%i") || src.starts_with("%I") {
            let prev = self.in_percent_array;
            self.in_percent_array = true;
            ruby_prism::visit_array_node(self, node);
            self.in_percent_array = prev;
        } else {
            ruby_prism::visit_array_node(self, node);
        }
    }
}

crate::register_cop!("Lint/SymbolConversion", |cfg| {
    let style_str = cfg
        .get_cop_config("Lint/SymbolConversion")
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "strict".to_string());
    let style = if style_str == "consistent" { Style::Consistent } else { Style::Strict };
    Some(Box::new(SymbolConversion::new(style)))
});
