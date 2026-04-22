//! Style/PerlBackrefs cop
//!
//! Flags Perl-style regex backrefs ($1, $&, $`, $', $+, $MATCH, etc.)
//! and suggests `Regexp.last_match(n)` equivalents.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct PerlBackrefs;

impl PerlBackrefs {
    pub fn new() -> Self {
        Self
    }

    /// Map backreference source text → (preferred, correction)
    fn map_backref(name: &str, has_regexp_in_scope: bool) -> Option<(&'static str, String)> {
        let prefix = if has_regexp_in_scope { "::Regexp" } else { "Regexp" };
        match name {
            "$&" | "$MATCH" => Some(("last_match(0)", format!("{}.last_match(0)", prefix))),
            "$`" | "$PREMATCH" => Some(("last_match.pre_match", format!("{}.last_match.pre_match", prefix))),
            "$'" | "$POSTMATCH" => Some(("last_match.post_match", format!("{}.last_match.post_match", prefix))),
            "$+" | "$LAST_PAREN_MATCH" => Some(("last_match(-1)", format!("{}.last_match(-1)", prefix))),
            _ => None,
        }
    }

    fn map_numbered(n: u32, has_regexp_in_scope: bool) -> String {
        let prefix = if has_regexp_in_scope { "::Regexp" } else { "Regexp" };
        format!("{}.last_match({})", prefix, n)
    }

    /// Check if source has a local `class Regexp` or `module Regexp` definition.
    fn has_local_regexp(source: &str) -> bool {
        // Simple heuristic: scan for `class Regexp` not at top level
        let result = ruby_prism::parse(source.as_bytes());
        let mut checker = RegexpScopeChecker { depth: 0, found: false };
        ruby_prism::visit_program_node(&mut checker, &result.node().as_program_node().unwrap());
        checker.found
    }
}

struct RegexpScopeChecker {
    depth: usize,
    found: bool,
}

impl Visit<'_> for RegexpScopeChecker {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if self.depth > 0 {
            // Check if class name is Regexp
            let name_src = node.name().as_slice();
            if name_src == b"Regexp" {
                self.found = true;
            }
        }
        self.depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.depth -= 1;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.depth -= 1;
    }
}

impl Cop for PerlBackrefs {
    fn name(&self) -> &'static str {
        "Style/PerlBackrefs"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let has_regexp_scope = Self::has_local_regexp(ctx.source);
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = PerlBackrefsVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            has_regexp_scope,
            inside_embedded_var: false,
        };
        ruby_prism::visit_program_node(&mut visitor, &result.node().as_program_node().unwrap());
        visitor.offenses
    }
}

struct PerlBackrefsVisitor<'a> {
    cop: &'a PerlBackrefs,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    has_regexp_scope: bool,
    inside_embedded_var: bool,
}

impl PerlBackrefsVisitor<'_> {
    fn emit_offense(&mut self, start: usize, end: usize, name_display: &str, replacement: String) {
        let old_src = &self.ctx.source[start..end];
        let msg = format!("Prefer `{}` over `{}`.", replacement, name_display);
        let correction = Correction::replace(start, end, replacement);
        self.offenses.push(
            self.ctx.offense_with_range(self.cop.name(), &msg, self.cop.severity(), start, end)
                .with_correction(correction)
        );
    }
}

impl Visit<'_> for PerlBackrefsVisitor<'_> {
    fn visit_back_reference_read_node(&mut self, node: &ruby_prism::BackReferenceReadNode) {
        if !self.inside_embedded_var {
            let loc = node.location();
            let name = &self.ctx.source[loc.start_offset()..loc.end_offset()];
            if let Some((_label, replacement)) = PerlBackrefs::map_backref(name, self.has_regexp_scope) {
                self.emit_offense(loc.start_offset(), loc.end_offset(), name, replacement);
            }
        }
        ruby_prism::visit_back_reference_read_node(self, node);
    }

    fn visit_numbered_reference_read_node(&mut self, node: &ruby_prism::NumberedReferenceReadNode) {
        if !self.inside_embedded_var {
            let n = node.number();
            let loc = node.location();
            let name = &self.ctx.source[loc.start_offset()..loc.end_offset()];
            let replacement = PerlBackrefs::map_numbered(n, self.has_regexp_scope);
            self.emit_offense(loc.start_offset(), loc.end_offset(), name, replacement);
        }
        ruby_prism::visit_numbered_reference_read_node(self, node);
    }

    fn visit_global_variable_read_node(&mut self, node: &ruby_prism::GlobalVariableReadNode) {
        if !self.inside_embedded_var {
            let loc = node.location();
            let name_bytes = node.name().as_slice();
            let name = match std::str::from_utf8(name_bytes) {
                Ok(s) => s,
                Err(_) => return,
            };
            if let Some((_label, replacement)) = PerlBackrefs::map_backref(name, self.has_regexp_scope) {
                self.emit_offense(loc.start_offset(), loc.end_offset(), name, replacement);
            }
        }
        ruby_prism::visit_global_variable_read_node(self, node);
    }

    // Handle embedded vars in strings: "#$1" → "#{Regexp.last_match(1)}"
    fn visit_embedded_variable_node(&mut self, node: &ruby_prism::EmbeddedVariableNode) {
        self.inside_embedded_var = true;
        let var = node.variable();
        match var {
            ruby_prism::Node::NumberedReferenceReadNode { .. } => {
                let nr = var.as_numbered_reference_read_node().unwrap();
                let n = nr.number();
                let var_loc = var.location();
                let name = &self.ctx.source[var_loc.start_offset()..var_loc.end_offset()];
                let replacement = format!("{{Regexp.last_match({})}}", n);
                // Replace just the var portion (not the #)
                let msg = format!("Prefer `Regexp.last_match({})` over `{}`.", n, name);
                let correction = Correction::replace(
                    var_loc.start_offset(), var_loc.end_offset(),
                    format!("{{Regexp.last_match({})}}", n),
                );
                self.offenses.push(
                    self.ctx.offense_with_range(self.cop.name(), &msg, self.cop.severity(),
                        var_loc.start_offset(), var_loc.end_offset())
                        .with_correction(correction)
                );
            }
            ruby_prism::Node::BackReferenceReadNode { .. } => {
                let br = var.as_back_reference_read_node().unwrap();
                let var_loc = var.location();
                let name = &self.ctx.source[var_loc.start_offset()..var_loc.end_offset()];
                if let Some((_label, repl)) = PerlBackrefs::map_backref(name, self.has_regexp_scope) {
                    let msg = format!("Prefer `{}` over `{}`.", repl, name);
                    let correction = Correction::replace(
                        var_loc.start_offset(), var_loc.end_offset(),
                        format!("{{{}}}", repl),
                    );
                    self.offenses.push(
                        self.ctx.offense_with_range(self.cop.name(), &msg, self.cop.severity(),
                            var_loc.start_offset(), var_loc.end_offset())
                            .with_correction(correction)
                    );
                }
            }
            ruby_prism::Node::GlobalVariableReadNode { .. } => {
                let gv = var.as_global_variable_read_node().unwrap();
                let var_loc = var.location();
                let name_bytes = gv.name().as_slice();
                let name = match std::str::from_utf8(name_bytes) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let src_text = &self.ctx.source[var_loc.start_offset()..var_loc.end_offset()];
                if let Some((_label, repl)) = PerlBackrefs::map_backref(name, self.has_regexp_scope) {
                    let msg = format!("Prefer `{}` over `{}`.", repl, src_text);
                    let correction = Correction::replace(
                        var_loc.start_offset(), var_loc.end_offset(),
                        format!("{{{}}}", repl),
                    );
                    self.offenses.push(
                        self.ctx.offense_with_range(self.cop.name(), &msg, self.cop.severity(),
                            var_loc.start_offset(), var_loc.end_offset())
                            .with_correction(correction)
                    );
                }
            }
            _ => {}
        }
        self.inside_embedded_var = false;
        // Do NOT recurse — we handle the inner var here directly to avoid double-visiting.
        // The inner node would be visited again by its own visit_* method.
    }
}

crate::register_cop!("Style/PerlBackrefs", |_cfg| {
    Some(Box::new(PerlBackrefs::new()))
});
