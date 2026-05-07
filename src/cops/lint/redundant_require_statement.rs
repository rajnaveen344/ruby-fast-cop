//! Lint/RedundantRequireStatement - Remove unnecessary require statements.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

const MSG: &str = "Remove unnecessary `require` statement.";

#[derive(Default)]
pub struct RedundantRequireStatement;

impl RedundantRequireStatement {
    pub fn new() -> Self { Self }
}

impl Cop for RedundantRequireStatement {
    fn name(&self) -> &'static str { "Lint/RedundantRequireStatement" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Pre-pass: build map call_start_offset → parent IfNode location
        let mut parent_map: HashMap<usize, (usize, usize)> = HashMap::new();
        {
            let mut pre = PreVisitor { parent_map: &mut parent_map };
            pre.visit_program_node(node);
        }

        let mut visitor = Visitor { ctx, offenses: Vec::new(), parent_map: &parent_map };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Pre-pass: for each IfNode whose single statement is a `require` call,
/// record call_start → (if_start, if_end).
struct PreVisitor<'a> {
    parent_map: &'a mut HashMap<usize, (usize, usize)>,
}

impl Visit<'_> for PreVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Modifier-if: condition is after `require`, predicate is before.
        // In both modifier and block form, check if there is exactly one statement
        // in the body and it's a `require` call with no receiver.
        if let Some(body) = node.statements() {
            let stmts: Vec<_> = body.body().iter().collect();
            if stmts.len() == 1 {
                if let Some(call) = stmts[0].as_call_node() {
                    let method = String::from_utf8_lossy(call.name().as_slice());
                    if method == "require" && call.receiver().is_none() {
                        let nloc = node.location();
                        self.parent_map.insert(call.location().start_offset(), (nloc.start_offset(), nloc.end_offset()));
                    }
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }
}

struct Visitor<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    offenses: Vec<Offense>,
    parent_map: &'a HashMap<usize, (usize, usize)>,
}

impl Visit<'_> for Visitor<'_, '_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method == "require" && node.receiver().is_none() {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let Some(first) = arg_list.first() {
                    if let Some(str_node) = first.as_string_node() {
                        let feature = String::from_utf8_lossy(str_node.unescaped());
                        if self.is_redundant_feature(feature.as_ref()) {
                            let loc = node.location();
                            let call_start = loc.start_offset();
                            let call_end = loc.end_offset();

                            let mut offense = self.ctx.offense_with_range(
                                "Lint/RedundantRequireStatement",
                                MSG,
                                Severity::Warning,
                                call_start,
                                call_end,
                            );

                            offense = self.build_correction(offense, call_start);
                            self.offenses.push(offense);
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a, 'b> Visitor<'a, 'b> {
    fn is_redundant_feature(&self, feature: &str) -> bool {
        let ver = self.ctx.target_ruby_version;
        feature == "enumerator"
            || (ver >= 2.1 && feature == "thread")
            || (ver >= 2.2 && (feature == "rational" || feature == "complex"))
            || (ver >= 2.7 && feature == "ruby2_keywords")
            || (ver >= 3.1 && feature == "fiber")
            || (ver >= 3.2 && feature == "set")
            || (ver >= 4.0 && feature == "pathname")
    }

    fn build_correction(&self, offense: Offense, call_start: usize) -> Offense {
        let source = self.ctx.source.as_bytes();

        if let Some(&(if_start, if_end)) = self.parent_map.get(&call_start) {
            // This require is inside an IfNode (modifier-if or block-if).
            // The correction is to strip the body, keeping `if condition\nend`.
            // We need to find the condition source and rebuild.
            // Strategy: find the `if` keyword location and condition text.
            // We know the IfNode spans if_start..if_end.
            // The corrected form is: `if <condition>\nend`
            // But we need the condition from source. The IfNode starts with `if `
            // then condition, then either `;`/newline (block) or the `require` call (modifier).

            // Find condition end: scan from `if ` to find where condition ends.
            // For modifier form: source is `require 'X' if condition`
            //   → if_start = start of `require`, condition is after `if `.
            //   But Prism parses modifier-if: the IfNode node starts at `require`.
            //   Actually for modifier-if, IfNode.keyword_loc() points to `if` keyword.
            // For block form: `if condition\n  require 'X'\nend`
            //   → IfNode starts at `if`.

            // Use keyword_loc to find `if` keyword position.
            // We'll use a text-based approach:
            // Find the condition by parsing the source around the IfNode.
            // The condition ends right before the `do`/`;`/newline in block form,
            // or before end-of-IfNode after stripping `require...` in modifier form.

            // Simpler: re-parse just the if node source to find condition.
            // Even simpler: find `if ` in if_start..if_end source and extract the condition.

            let if_src = &source[if_start..if_end];

            // Find the indentation of if_start line
            let indent = line_indent(source, if_start);

            // Find `if ` in the if source (could be modifier `require X if cond` or `if cond`)
            // We search for ` if ` to handle modifier form, or `^if ` for block form.
            let if_keyword_pos = find_if_keyword(source, if_start, if_end);

            let correction = if let Some(kw_pos) = if_keyword_pos {
                // kw_pos = offset of `i` in `if` keyword within source
                let after_if = kw_pos + 2; // skip `if`
                // Skip space after `if`
                let cond_start = skip_whitespace(source, after_if);
                // Condition ends at end-of-line (for both modifier and block forms),
                // or at `;` if same-line block form
                let cond_end = find_condition_end(source, cond_start, if_end);
                let condition = &self.ctx.source[cond_start..cond_end];
                let new_src = format!("{}if {}\n{}end", indent, condition.trim_end(), indent);
                Correction::replace(if_start, if_end, new_src)
            } else {
                // Fallback: just delete the call line
                whole_line_delete(source, call_start)
            };

            offense.with_correction(correction)
        } else {
            // Simple standalone require — delete whole line
            offense.with_correction(whole_line_delete(source, call_start))
        }
    }
}

/// Delete from start of line (after prev newline) to end of line (including newline).
fn whole_line_delete(source: &[u8], node_start: usize) -> Correction {
    // Find start of line
    let line_start = source[..node_start].iter().rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    // Find end of line (inclusive of newline)
    let line_end = source[node_start..].iter().position(|&b| b == b'\n')
        .map(|p| node_start + p + 1)
        .unwrap_or(source.len());
    Correction::delete(line_start, line_end)
}

/// Returns the indentation string of the line containing `pos`.
fn line_indent(source: &[u8], pos: usize) -> String {
    let line_start = source[..pos].iter().rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let mut end = line_start;
    while end < source.len() && (source[end] == b' ' || source[end] == b'\t') {
        end += 1;
    }
    String::from_utf8_lossy(&source[line_start..end]).into_owned()
}

/// Find the byte offset of `if` keyword within source[if_start..if_end].
/// For block form: `if` is at if_start. For modifier form: ` if ` appears inside.
fn find_if_keyword(source: &[u8], if_start: usize, if_end: usize) -> Option<usize> {
    // Check if block form: starts with `if ` or `if\n`
    if if_start + 2 < source.len()
        && source[if_start] == b'i'
        && source[if_start + 1] == b'f'
        && (source[if_start + 2] == b' ' || source[if_start + 2] == b'\t' || source[if_start + 2] == b'\n')
    {
        return Some(if_start);
    }
    // Modifier form: search for ` if ` pattern
    let mut i = if_start;
    while i + 4 <= if_end {
        if source[i] == b' '
            && source[i + 1] == b'i'
            && source[i + 2] == b'f'
            && (source[i + 3] == b' ' || source[i + 3] == b'\t')
        {
            return Some(i + 1); // return position of `i` in `if`
        }
        i += 1;
    }
    None
}

fn skip_whitespace(source: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < source.len() && (source[i] == b' ' || source[i] == b'\t') {
        i += 1;
    }
    i
}

/// Find end of condition: stop at newline, `;`, or `do` keyword, or if_end.
fn find_condition_end(source: &[u8], start: usize, if_end: usize) -> usize {
    let mut i = start;
    while i < if_end {
        if source[i] == b'\n' || source[i] == b';' { return i; }
        // Check for `do` keyword
        if i + 2 <= if_end && source[i] == b'd' && source[i+1] == b'o'
            && (i + 2 >= if_end || source[i+2] == b' ' || source[i+2] == b'\n' || source[i+2] == b'\t' || source[i+2] == b';')
        {
            return i;
        }
        i += 1;
    }
    if_end
}

crate::register_cop!("Lint/RedundantRequireStatement", |_cfg| Some(Box::new(RedundantRequireStatement::new())));
