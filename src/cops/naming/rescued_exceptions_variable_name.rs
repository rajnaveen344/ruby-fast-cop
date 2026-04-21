//! Naming/RescuedExceptionsVariableName cop
//!
//! Checks that `rescue => var` uses the preferred variable name.
//! Default preferred name: "e".
//!
//! NOTE: Nested rescues are NOT checked (only outer rescue is flagged).
//! Shadow check: if the preferred name is already used as a local variable in the rescue body,
//! skip to avoid shadowing.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/naming/rescued_exceptions_variable_name.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct RescuedExceptionsVariableName {
    preferred_name: String,
}

impl Default for RescuedExceptionsVariableName {
    fn default() -> Self {
        Self { preferred_name: "e".to_string() }
    }
}

impl RescuedExceptionsVariableName {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_preferred_name(name: String) -> Self {
        Self { preferred_name: name }
    }

    /// Given actual var name and preferred name, compute the expected form.
    /// If actual starts with `_`, preferred should also start with `_`.
    fn expected_name(&self, actual: &str) -> String {
        if actual.starts_with('_') {
            format!("_{}", self.preferred_name)
        } else {
            self.preferred_name.clone()
        }
    }

    fn is_correct(&self, actual: &str) -> bool {
        actual == self.expected_name(actual)
    }
}

/// Returns true if any local variable READ in `node`'s subtree has the given name.
/// Used for shadow check: if preferred name already appears as an lvar, skip.
fn has_lvar_named(node: &Node, name: &str) -> bool {
    struct LvarChecker {
        name: String,
        found: bool,
    }
    impl Visit<'_> for LvarChecker {
        fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
            if String::from_utf8_lossy(node.name().as_slice()) == self.name {
                self.found = true;
            }
        }
        fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
            // Don't check LHS name, but recurse into RHS
            ruby_prism::visit_local_variable_write_node(self, node);
        }
    }
    let mut checker = LvarChecker { name: name.to_string(), found: false };
    checker.visit(node);
    checker.found
}

/// Build correction edits to rename `old_name` → `new_name` in a rescue body.
/// Rules from RuboCop:
/// - Rename lvar (reads)
/// - For lvasgn/masgn (writes): rename only the RHS, then stop (break)
/// - Handle value omission: `foo:` → insert ` new_name` after `:`
fn build_body_edits(
    source: &str,
    body: &Node,
    old_name: &str,
    new_name: &str,
) -> Vec<Edit> {
    struct BodyRenamer {
        old_name: String,
        new_name: String,
        edits: Vec<Edit>,
        stopped: bool,
    }

    impl Visit<'_> for BodyRenamer {
        fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
            if self.stopped {
                return;
            }
            // Check for value omission: key is symbol, value is ImplicitNode
            let value = node.value();
            if matches!(value, Node::ImplicitNode { .. }) {
                // The key must be a symbol whose name matches old_name
                let key = node.key();
                if let Some(sym) = key.as_symbol_node() {
                    let sym_name = String::from_utf8_lossy(sym.unescaped().as_ref()).to_string();
                    if sym_name == self.old_name {
                        // Insert ` new_name` after the `:` (end of the assoc node's operator)
                        // The operator_loc for `foo:` is the `:` character
                        // We insert after the end of the key symbol (which includes the `:`)
                        let key_end = key.location().end_offset();
                        self.edits.push(Edit {
                            start_offset: key_end,
                            end_offset: key_end,
                            replacement: format!(" {}", self.new_name),
                        });
                        return; // Don't recurse further into this assoc
                    }
                }
            }
            ruby_prism::visit_assoc_node(self, node);
        }

        fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
            if self.stopped {
                return;
            }
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            if name == self.old_name {
                let loc = node.location();
                self.edits.push(Edit {
                    start_offset: loc.start_offset(),
                    end_offset: loc.end_offset(),
                    replacement: self.new_name.clone(),
                });
            }
        }

        fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
            if self.stopped {
                return;
            }
            let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            if name == self.old_name {
                // Rename only the RHS, then stop further corrections
                let rhs = node.value();
                ruby_prism::visit_local_variable_write_node(self, node);
                self.stopped = true;
                return;
            }
            ruby_prism::visit_local_variable_write_node(self, node);
        }

        fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
            if self.stopped {
                return;
            }
            // Check if any LHS target is the old_name
            let has_old_name_lhs = node.lefts().iter().any(|t| {
                if let Some(lv) = t.as_local_variable_target_node() {
                    String::from_utf8_lossy(lv.name().as_slice()) == self.old_name.as_str()
                } else {
                    false
                }
            });
            if has_old_name_lhs {
                // Only rename in the RHS
                if let Some(value) = Some(node.value()) {
                    self.visit(&value);
                }
                self.stopped = true;
                return;
            }
            ruby_prism::visit_multi_write_node(self, node);
        }
    }

    let mut renamer = BodyRenamer {
        old_name: old_name.to_string(),
        new_name: new_name.to_string(),
        edits: Vec::new(),
        stopped: false,
    };
    renamer.visit(body);
    renamer.edits
}

struct RescuedVarVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a RescuedExceptionsVariableName,
    offenses: Vec<Offense>,
    /// True when we are inside the BODY of a rescue (for nested rescue detection)
    inside_rescue_body: bool,
}

impl<'a> RescuedVarVisitor<'a> {
    fn check_rescue(&mut self, node: &ruby_prism::RescueNode) {
        // Skip nested rescues (rescues inside the body of another rescue)
        if self.inside_rescue_body {
            return;
        }

        let reference = match node.reference() {
            Some(r) => r,
            None => return, // No variable binding
        };

        // Only simple local variable (not method calls like `storage.exception`)
        let lv = match reference.as_local_variable_target_node() {
            Some(lv) => lv,
            None => return,
        };

        let actual_name = String::from_utf8_lossy(lv.name().as_slice()).to_string();
        if self.cop.is_correct(&actual_name) {
            return;
        }

        let expected = self.cop.expected_name(&actual_name);

        // Shadow check: if any lvar in the rescue body already uses the preferred name, skip
        if let Some(stmts) = node.statements() {
            let body_node = stmts.as_node();
            if has_lvar_named(&body_node, &expected) {
                return;
            }
        }

        let msg = format!("Use `{}` instead of `{}`.", expected, actual_name);
        let ref_loc = reference.location();

        let offense = self.ctx.offense_with_range(
            "Naming/RescuedExceptionsVariableName",
            &msg,
            Severity::Convention,
            ref_loc.start_offset(),
            ref_loc.end_offset(),
        );

        // Build correction
        let mut edits = vec![Edit {
            start_offset: ref_loc.start_offset(),
            end_offset: ref_loc.end_offset(),
            replacement: expected.clone(),
        }];

        // Rename in rescue body
        if let Some(stmts) = node.statements() {
            let body_node = stmts.as_node();
            let body_edits = build_body_edits(self.ctx.source, &body_node, &actual_name, &expected);
            edits.extend(body_edits);
        }

        // Rename in siblings after the rescue (the `kwbegin` right siblings pattern)
        // This handles variables referenced after the begin/end block
        // We scan siblings of the rescue's parent begin node
        // For simplicity: scan from the end of this rescue's body to the end of the
        // parent begin node for reads of the old variable.
        // The rescue's node ends at node.location().end_offset().
        // We need to find where the outer scope continues after the entire begin..end.
        // RuboCop does: kwbegin_node.right_siblings.each { correct_node }
        // This is tricky without parent tracking. For now, we skip post-rescue sibling renaming.
        // The correction tests that need post-rescue renaming:
        // - "when_the_variable_is_referenced_after_rescue_statement_handles_it" — foo(e1) after end

        // To handle the post-rescue case, we scan source from the rescue's end
        // We look for the parent begin node's end and scan any siblings after it.
        // Approximation: scan from rescue body end to end of source for this variable.
        // But we must stop at subsequent rescue clauses.
        let subsequent_start = node
            .subsequent()
            .map(|s| s.location().start_offset())
            .unwrap_or(usize::MAX);

        let body_end = node
            .statements()
            .map(|s| s.location().end_offset())
            .unwrap_or(node.location().end_offset());

        // Post-rescue reads: from body_end to source end, stopping before subsequent rescue
        // Use the same AST-based approach but with a simpler source scan for post-rescue refs
        // since we don't have parent tracking.
        // For now, use source scanning for post-rescue references.
        let post_edits = collect_post_rescue_renames(
            self.ctx.source,
            &actual_name,
            &expected,
            body_end,
            subsequent_start,
        );
        edits.extend(post_edits);

        // Sort edits descending by offset (for correct application order)
        edits.sort_by(|a, b| b.start_offset.cmp(&a.start_offset));
        edits.dedup_by_key(|e| e.start_offset);

        let correction = Correction { edits };
        self.offenses.push(offense.with_correction(correction));
    }
}

/// Source-level scan for post-rescue references of `old_name`.
/// Scans from `from` to `min(to, subsequent_start)` and collects word-boundary occurrences.
fn collect_post_rescue_renames(
    source: &str,
    old_name: &str,
    new_name: &str,
    from: usize,
    stop_before: usize,
) -> Vec<Edit> {
    let end = stop_before.min(source.len());
    if from >= end {
        return vec![];
    }
    let src = &source[from..end];
    let name_bytes = old_name.as_bytes();
    let src_bytes = src.as_bytes();
    let mut edits = Vec::new();
    let mut i = 0;
    while i + name_bytes.len() <= src_bytes.len() {
        if &src_bytes[i..i + name_bytes.len()] == name_bytes {
            let before_ok = i == 0 || !is_ident_char(src_bytes[i - 1]);
            let after_pos = i + name_bytes.len();
            let after_ok = after_pos >= src_bytes.len() || !is_ident_char(src_bytes[after_pos]);
            if before_ok && after_ok {
                // Skip if it looks like a write LHS (`name =` not `==`)
                let rest = &src_bytes[after_pos..];
                let j = rest.iter().position(|&b| b != b' ' && b != b'\t').unwrap_or(rest.len());
                let is_write = j < rest.len()
                    && rest[j] == b'='
                    && (j + 1 >= rest.len() || rest[j + 1] != b'=');
                if !is_write {
                    let abs_start = from + i;
                    let abs_end = abs_start + name_bytes.len();
                    edits.push(Edit {
                        start_offset: abs_start,
                        end_offset: abs_end,
                        replacement: new_name.to_string(),
                    });
                }
                i += name_bytes.len();
                continue;
            }
        }
        i += 1;
    }
    edits
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl Visit<'_> for RescuedVarVisitor<'_> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        self.check_rescue(node);

        // Visit rescue body with inside_rescue_body = true (to skip nested rescues)
        let prev = self.inside_rescue_body;
        self.inside_rescue_body = true;
        if let Some(stmts) = node.statements() {
            self.visit_statements_node(&stmts);
        }
        self.inside_rescue_body = prev;

        // Visit subsequent rescues (siblings) at the same nesting level
        if let Some(subsequent) = node.subsequent() {
            let n = subsequent.as_node();
            self.visit(&n);
        }
    }
}

impl Cop for RescuedExceptionsVariableName {
    fn name(&self) -> &'static str {
        "Naming/RescuedExceptionsVariableName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RescuedVarVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            inside_rescue_body: false,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    preferred_name: String,
}

crate::register_cop!("Naming/RescuedExceptionsVariableName", |cfg| {
    let c: Cfg = cfg.typed("Naming/RescuedExceptionsVariableName");
    let name = if c.preferred_name.is_empty() { "e".to_string() } else { c.preferred_name };
    Some(Box::new(RescuedExceptionsVariableName::with_preferred_name(name)))
});
