//! Style/SoleNestedConditional - Detect if/unless nested inside another if/unless
//! that could be combined with `&&`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/sole_nested_conditional.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/SoleNestedConditional";

pub struct SoleNestedConditional {
    allow_modifier: bool,
}

impl SoleNestedConditional {
    pub fn new() -> Self {
        Self {
            allow_modifier: false,
        }
    }

    pub fn with_config(allow_modifier: bool) -> Self {
        Self { allow_modifier }
    }
}

impl Default for SoleNestedConditional {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for SoleNestedConditional {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = SoleNestedConditionalVisitor {
            ctx,
            allow_modifier: self.allow_modifier,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SoleNestedConditionalVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    allow_modifier: bool,
    offenses: Vec<Offense>,
}

impl<'a> SoleNestedConditionalVisitor<'a> {
    /// Check an if/unless node for sole nested conditional pattern
    fn check_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Skip ternaries, elsif, or nodes with else branches
        if is_ternary_if(node, self.ctx.source) {
            return;
        }
        if is_elsif(node, self.ctx.source) {
            return;
        }
        if node.subsequent().is_some() {
            return;
        }

        let if_branch = match get_if_branch_if(node, self.ctx.source) {
            Some(b) => b,
            None => return,
        };

        // Check for variable assignment in condition that's used in inner condition
        if use_variable_assignment_in_condition_if(node, &if_branch, self.ctx.source) {
            return;
        }

        if !self.offending_branch_from_if(node, &if_branch) {
            return;
        }

        // Determine the keyword for the message based on outer node
        let keyword = if_keyword_text_if(node, self.ctx.source);
        let message = format!(
            "Consider merging nested conditions into outer `{}` conditions.",
            keyword
        );

        // Offense location is the keyword of the inner if
        let inner_keyword_loc = inner_keyword_loc(&if_branch);
        if let Some((start, end)) = inner_keyword_loc {
            let outer_is_modifier = is_modifier_form_if(node, self.ctx.source);
            let correction = if outer_is_modifier {
                // Outer is modifier form: `inner_block if outer_cond`
                // -> `if chainable(outer) && chainable(inner)\n  body\nend`
                build_correction_outer_modifier_if(node, &if_branch, self.ctx.source)
            } else if if_branch.is_modifier {
                // Inner is modifier form: `if outer\n  body if inner\nend`
                // -> `if chainable(outer) && chainable(inner)\n  body\nend`
                build_correction_inner_modifier_if(node, &if_branch, self.ctx.source)
            } else {
                // Both block form: `if outer\n  if inner\n    body\n  end\nend`
                // -> `if chainable(outer) && chainable(inner)\n    body\n  end`
                build_correction_basic_if(node, &if_branch, self.ctx.source)
            };

            let mut offense = self.ctx.offense_with_range(
                COP_NAME,
                &message,
                Severity::Convention,
                start,
                end,
            );
            if let Some(corr) = correction {
                offense = offense.with_correction(corr);
            }
            self.offenses.push(offense);
        }
    }

    fn check_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        // unless nodes don't have ternary/elsif
        if node.else_clause().is_some() {
            return;
        }

        let if_branch = match get_if_branch_unless(node, self.ctx.source) {
            Some(b) => b,
            None => return,
        };

        if use_variable_assignment_in_condition_unless(node, &if_branch, self.ctx.source) {
            return;
        }

        if !self.offending_branch_from_unless(node, &if_branch) {
            return;
        }

        let message = "Consider merging nested conditions into outer `unless` conditions.";

        let inner_keyword_loc = inner_keyword_loc(&if_branch);
        if let Some((start, end)) = inner_keyword_loc {
            let outer_is_modifier = is_modifier_form_unless(node, self.ctx.source);
            let correction = if outer_is_modifier {
                build_correction_outer_modifier_unless(node, &if_branch, self.ctx.source)
            } else if if_branch.is_modifier {
                build_correction_inner_modifier_unless(node, &if_branch, self.ctx.source)
            } else {
                build_correction_basic_unless(node, &if_branch, self.ctx.source)
            };

            let mut offense = self.ctx.offense_with_range(
                COP_NAME,
                message,
                Severity::Convention,
                start,
                end,
            );
            if let Some(corr) = correction {
                offense = offense.with_correction(corr);
            }
            self.offenses.push(offense);
        }
    }

    fn offending_branch_from_if(
        &self,
        outer: &ruby_prism::IfNode,
        branch: &InnerConditional,
    ) -> bool {
        if branch.has_else {
            return false;
        }
        if branch.is_ternary {
            return false;
        }
        let outer_is_modifier = is_modifier_form_if(outer, self.ctx.source);
        if (outer_is_modifier || branch.is_modifier) && self.allow_modifier {
            return false;
        }
        true
    }

    fn offending_branch_from_unless(
        &self,
        outer: &ruby_prism::UnlessNode,
        branch: &InnerConditional,
    ) -> bool {
        if branch.has_else {
            return false;
        }
        if branch.is_ternary {
            return false;
        }
        let outer_is_modifier = is_modifier_form_unless(outer, self.ctx.source);
        if (outer_is_modifier || branch.is_modifier) && self.allow_modifier {
            return false;
        }
        true
    }
}

/// Represents the inner conditional branch info
struct InnerConditional {
    has_else: bool,
    is_ternary: bool,
    is_modifier: bool,
    keyword_start: usize,
    keyword_end: usize,
    /// inner condition range
    cond_start: usize,
    cond_end: usize,
    /// inner end keyword (None if modifier)
    end_kw_start: Option<usize>,
    end_kw_end: Option<usize>,
    /// whether inner is `unless`
    is_unless: bool,
    /// full inner node range
    node_start: usize,
    node_end: usize,
    /// precomputed chainable form of inner condition
    /// for `if` inner: add_parentheses_if_needed; for `unless` inner: negated form
    chainable_cond: String,
}

fn inner_keyword_loc(inner: &InnerConditional) -> Option<(usize, usize)> {
    Some((inner.keyword_start, inner.keyword_end))
}

/// Get the sole if_branch from an IfNode's body, if it's a single if/unless node
fn get_if_branch_if(node: &ruby_prism::IfNode, source: &str) -> Option<InnerConditional> {
    let stmts = node.statements()?;
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 {
        return None;
    }
    extract_inner_conditional(&body[0], source)
}

/// Get the sole if_branch from an UnlessNode's body
fn get_if_branch_unless(node: &ruby_prism::UnlessNode, source: &str) -> Option<InnerConditional> {
    let stmts = node.statements()?;
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 {
        return None;
    }
    extract_inner_conditional(&body[0], source)
}

fn extract_inner_conditional(node: &Node, source: &str) -> Option<InnerConditional> {
    match node {
        Node::IfNode { .. } => {
            let if_node = node.as_if_node().unwrap();
            let has_else = if_node.subsequent().is_some();
            let is_ternary = if_node.if_keyword_loc().is_none();
            let is_modifier = if_node.end_keyword_loc().is_none() && !is_ternary;

            let (keyword_start, keyword_end) = if let Some(kw_loc) = if_node.if_keyword_loc() {
                (kw_loc.start_offset(), kw_loc.end_offset())
            } else {
                // ternary - use start of node
                let loc = node.location();
                (loc.start_offset(), loc.start_offset() + 2)
            };

            let cond = if_node.predicate();
            let (end_kw_start, end_kw_end) = if let Some(ek) = if_node.end_keyword_loc() {
                (Some(ek.start_offset()), Some(ek.end_offset()))
            } else {
                (None, None)
            };

            let node_loc = node.location();
            let chainable_cond = add_parentheses_if_needed(&cond, source);

            Some(InnerConditional {
                has_else,
                is_ternary,
                is_modifier,
                keyword_start,
                keyword_end,
                cond_start: cond.location().start_offset(),
                cond_end: cond.location().end_offset(),
                end_kw_start,
                end_kw_end,
                is_unless: false,
                node_start: node_loc.start_offset(),
                node_end: node_loc.end_offset(),
                chainable_cond,
            })
        }
        Node::UnlessNode { .. } => {
            let unless_node = node.as_unless_node().unwrap();
            let has_else = unless_node.else_clause().is_some();
            let is_modifier = unless_node.end_keyword_loc().is_none();

            let kw_loc = unless_node.keyword_loc();
            let cond = unless_node.predicate();
            let (end_kw_start, end_kw_end) = if let Some(ek) = unless_node.end_keyword_loc() {
                (Some(ek.start_offset()), Some(ek.end_offset()))
            } else {
                (None, None)
            };

            let node_loc = node.location();
            let chainable_cond = chainable_unless_cond_node(&cond, source);

            Some(InnerConditional {
                has_else,
                is_ternary: false,
                is_modifier,
                keyword_start: kw_loc.start_offset(),
                keyword_end: kw_loc.end_offset(),
                cond_start: cond.location().start_offset(),
                cond_end: cond.location().end_offset(),
                end_kw_start,
                end_kw_end,
                is_unless: true,
                node_start: node_loc.start_offset(),
                node_end: node_loc.end_offset(),
                chainable_cond,
            })
        }
        _ => None,
    }
}

// ============= CORRECTION BUILDERS =============

/// Basic block-block: outer if, inner if/unless
/// `if outer\n  if inner\n    body\n  end\nend` → `if outer && inner\n    body\n  end`
fn build_correction_basic_if(
    node: &ruby_prism::IfNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<Correction> {
    let outer_kw_loc = node.if_keyword_loc()?;
    let outer_cond = node.predicate();
    let outer_cond_end = outer_cond.location().end_offset();
    let outer_end = node.end_keyword_loc()?;
    let outer_end_start = outer_end.start_offset();
    let outer_end_end = outer_end.end_offset();

    let outer_cond_src = &source[outer_cond.location().start_offset()..outer_cond_end];
    let outer_chainable = chainable_if_cond(&outer_cond, outer_cond_src, source);
    let inner_chainable = &inner.chainable_cond;

    let outer_kw_start = outer_kw_loc.start_offset();
    let outer_kw_end = outer_kw_loc.end_offset();

    let comment_text = extract_comments_before_inner(node, inner, source);
    let outer_cond_start = outer_cond.location().start_offset();

    let mut edits = Vec::new();

    // If outer is `unless`, change keyword to `if`
    let outer_kw_text = &source[outer_kw_start..outer_kw_end];
    if outer_kw_text == "unless" {
        edits.push(Edit { start_offset: outer_kw_start, end_offset: outer_kw_end, replacement: "if".to_string() });
    }

    // Move comments before outer keyword (if any)
    if !comment_text.is_empty() {
        edits.push(Edit { start_offset: outer_kw_start, end_offset: outer_kw_start, replacement: comment_text.clone() });
    }

    // Replace outer condition with chainable form
    edits.push(Edit { start_offset: outer_cond_start, end_offset: outer_cond_end, replacement: outer_chainable });

    // Replace gap between outer cond end and inner cond start with ` && `
    edits.push(Edit { start_offset: outer_cond_end, end_offset: inner.cond_start, replacement: " && ".to_string() });

    // Replace inner condition with chainable form
    edits.push(Edit { start_offset: inner.cond_start, end_offset: inner.cond_end, replacement: inner_chainable.clone() });

    // Only delete outer `end` (inner `end` becomes the merged conditional's end)
    {
        let outer_end_line_start = find_line_start(source, outer_end_start);
        let outer_end_line_end = find_line_end_including_newline(source, outer_end_end);

        // Check if outer end is on same line as inner end (single-line case)
        let inner_end_same_line = inner.end_kw_start.map(|ies| {
            find_line_start(source, ies) == outer_end_line_start
        }).unwrap_or(false);

        if inner_end_same_line {
            // Single-line: `if foo; if bar; end; end` → `if foo && bar; end; `
            // Delete only outer `end` keyword (keep `; ` separator between inner end and outer end)
            edits.push(Edit { start_offset: outer_end_start, end_offset: outer_end_end, replacement: "".to_string() });
        } else {
            // Multi-line: delete outer end whole line
            edits.push(Edit { start_offset: outer_end_line_start, end_offset: outer_end_line_end, replacement: "".to_string() });
        }
    }

    // Remove comment lines from inner position (they were moved to before outer keyword)
    if !comment_text.is_empty() {
        if let Some(comment_range) = comment_range_before_inner(node, inner, source) {
            edits.push(Edit { start_offset: comment_range.0, end_offset: comment_range.1, replacement: "".to_string() });
        }
    }

    Some(Correction { edits })
}

/// Basic block-block: outer unless, inner if/unless
fn build_correction_basic_unless(
    node: &ruby_prism::UnlessNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<Correction> {
    let outer_kw_loc = node.keyword_loc();
    let outer_cond = node.predicate();
    let outer_cond_start = outer_cond.location().start_offset();
    let outer_cond_end = outer_cond.location().end_offset();
    let outer_end = node.end_keyword_loc()?;
    let outer_end_start = outer_end.start_offset();
    let outer_end_end = outer_end.end_offset();

    let outer_kw_start = outer_kw_loc.start_offset();
    let outer_kw_end = outer_kw_loc.end_offset();

    let outer_chainable = chainable_unless_cond_node(&outer_cond, source);
    let inner_chainable = &inner.chainable_cond;

    let comment_text = extract_comments_before_inner_unless(node, inner, source);

    let mut edits = Vec::new();

    // Change `unless` to `if`
    edits.push(Edit { start_offset: outer_kw_start, end_offset: outer_kw_end, replacement: "if".to_string() });

    // Move comments before outer keyword
    if !comment_text.is_empty() {
        edits.push(Edit { start_offset: outer_kw_start, end_offset: outer_kw_start, replacement: comment_text.clone() });
    }

    // Replace outer condition with chainable unless form
    edits.push(Edit { start_offset: outer_cond_start, end_offset: outer_cond_end, replacement: outer_chainable });

    // Replace gap between outer cond end and inner cond start with ` && `
    edits.push(Edit { start_offset: outer_cond_end, end_offset: inner.cond_start, replacement: " && ".to_string() });

    // Replace inner condition
    edits.push(Edit { start_offset: inner.cond_start, end_offset: inner.cond_end, replacement: inner_chainable.clone() });

    // Only delete outer `end` line (inner `end` stays as merged conditional's end)
    {
        let outer_end_line_start = find_line_start(source, outer_end_start);
        let outer_end_line_end = find_line_end_including_newline(source, outer_end_end);

        let inner_end_same_line = inner.end_kw_start.map(|ies| {
            find_line_start(source, ies) == outer_end_line_start
        }).unwrap_or(false);

        if inner_end_same_line {
            // Single-line: delete only outer `end` keyword
            edits.push(Edit { start_offset: outer_end_start, end_offset: outer_end_end, replacement: "".to_string() });
        } else {
            edits.push(Edit { start_offset: outer_end_line_start, end_offset: outer_end_line_end, replacement: "".to_string() });
        }
    }

    if !comment_text.is_empty() {
        if let Some(comment_range) = comment_range_before_inner_unless(node, inner, source) {
            edits.push(Edit { start_offset: comment_range.0, end_offset: comment_range.1, replacement: "".to_string() });
        }
    }

    Some(Correction { edits })
}

/// Guard/modifier inner: `if outer\n  body if inner\nend` → `if outer && inner\n  body\nend`
fn build_correction_inner_modifier_if(
    node: &ruby_prism::IfNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<Correction> {
    let outer_kw_loc = node.if_keyword_loc()?;
    let outer_cond = node.predicate();
    let outer_cond_start = outer_cond.location().start_offset();
    let outer_cond_end = outer_cond.location().end_offset();
    let outer_end = node.end_keyword_loc()?;
    let outer_end_start = outer_end.start_offset();
    let outer_end_end = outer_end.end_offset();

    let outer_kw_start = outer_kw_loc.start_offset();
    let outer_kw_end = outer_kw_loc.end_offset();

    let outer_cond_src = &source[outer_cond_start..outer_cond_end];
    let outer_chainable = chainable_if_cond(&outer_cond, outer_cond_src, source);
    let inner_chainable = &inner.chainable_cond;

    let mut edits = Vec::new();

    // Change outer `unless` to `if` if needed
    let outer_kw_text = &source[outer_kw_start..outer_kw_end];
    if outer_kw_text == "unless" {
        edits.push(Edit { start_offset: outer_kw_start, end_offset: outer_kw_end, replacement: "if".to_string() });
    }

    // Replace outer condition with chainable form
    edits.push(Edit { start_offset: outer_cond_start, end_offset: outer_cond_end, replacement: outer_chainable });

    // Insert ` && inner_chainable` after outer condition
    edits.push(Edit { start_offset: outer_cond_end, end_offset: outer_cond_end, replacement: format!(" && {}", inner_chainable) });

    // Remove the inner modifier part: from inner keyword to inner cond end (including surrounding space)
    // Inner node is modifier: `body if inner_cond` — the `if inner_cond` part needs removal
    // inner.keyword_start is start of `if`/`unless` in modifier position
    // We want to delete ` if inner_cond` (space before keyword to end of condition)
    let delete_start = find_space_before(source, inner.keyword_start);
    let delete_end = inner.cond_end;
    edits.push(Edit { start_offset: delete_start, end_offset: delete_end, replacement: "".to_string() });

    // NOTE: outer `end` is KEPT (it closes the merged conditional block)
    let _ = (outer_end_start, outer_end_end);

    Some(Correction { edits })
}

/// Guard/modifier inner: `unless outer\n  body if/unless inner\nend`
fn build_correction_inner_modifier_unless(
    node: &ruby_prism::UnlessNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<Correction> {
    let outer_kw_loc = node.keyword_loc();
    let outer_cond = node.predicate();
    let outer_cond_start = outer_cond.location().start_offset();
    let outer_cond_end = outer_cond.location().end_offset();
    let outer_end = node.end_keyword_loc()?;
    let outer_end_start = outer_end.start_offset();
    let outer_end_end = outer_end.end_offset();

    let outer_kw_start = outer_kw_loc.start_offset();
    let outer_kw_end = outer_kw_loc.end_offset();

    let outer_chainable = chainable_unless_cond_node(&outer_cond, source);
    let inner_chainable = &inner.chainable_cond;

    let mut edits = Vec::new();

    // Change `unless` to `if`
    edits.push(Edit { start_offset: outer_kw_start, end_offset: outer_kw_end, replacement: "if".to_string() });

    // Replace outer condition
    edits.push(Edit { start_offset: outer_cond_start, end_offset: outer_cond_end, replacement: outer_chainable });

    // Insert ` && inner_chainable` after outer condition
    edits.push(Edit { start_offset: outer_cond_end, end_offset: outer_cond_end, replacement: format!(" && {}", inner_chainable) });

    // Remove inner modifier: ` if/unless inner_cond`
    let delete_start = find_space_before(source, inner.keyword_start);
    let delete_end = inner.cond_end;
    edits.push(Edit { start_offset: delete_start, end_offset: delete_end, replacement: "".to_string() });

    // NOTE: outer `end` is KEPT (it closes the merged conditional block)
    let _ = (outer_end_start, outer_end_end);

    Some(Correction { edits })
}

/// Modifier outer if: `inner_block if outer_cond` → `if outer && inner\n  body\nend`
/// Here `node` = outer modifier if, body of node = inner block if
fn build_correction_outer_modifier_if(
    node: &ruby_prism::IfNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<Correction> {
    // Outer is modifier: `if outer_cond` appended to inner block
    // node.keyword_loc() = the `if` or the modifier `if`
    // For modifier if: `inner_block\nif outer_cond` — no, the structure is:
    //   `if inner_block_content\n  body\nend if outer_cond`
    // inner = the `if inner_block_content; ...; end` block
    // outer = the `if outer_cond` modifier wrapping it
    //
    // In Prism: `if foo\n  do_something\nend if bar`
    // The outer node is `if bar` (modifier) with body = `if foo; do_something; end`
    // node.if_keyword_loc() = location of the outer `if bar` keyword
    // node.predicate() = `bar`
    // inner = `if foo; do_something; end`
    //
    // RuboCop's autocorrect_outer_condition_modify_form:
    //   correct_node(corrector, if_branch) -- change inner keyword if unless, replace inner cond with chainable
    //   insert_before inner condition: "chainable(outer) && "
    //   remove outer modifier part: from outer.keyword to outer.condition.end + surrounding space

    let outer_kw_loc = node.if_keyword_loc()?;
    let outer_cond = node.predicate();
    let outer_cond_start = outer_cond.location().start_offset();
    let outer_cond_end = outer_cond.location().end_offset();
    let outer_kw_start = outer_kw_loc.start_offset();

    let outer_cond_src = &source[outer_cond_start..outer_cond_end];
    let outer_chainable = chainable_if_cond(&outer_cond, outer_cond_src, source);

    let inner_chainable = &inner.chainable_cond;

    let mut edits = Vec::new();

    // Change inner keyword to `if` if it's `unless`
    if inner.is_unless {
        edits.push(Edit { start_offset: inner.keyword_start, end_offset: inner.keyword_end, replacement: "if".to_string() });
    }

    // Replace inner condition with chainable form
    edits.push(Edit { start_offset: inner.cond_start, end_offset: inner.cond_end, replacement: inner_chainable.clone() });

    // Insert outer chainable before inner condition
    edits.push(Edit { start_offset: inner.cond_start, end_offset: inner.cond_start, replacement: format!("{} && ", outer_chainable) });

    // Remove outer modifier: from outer keyword to outer cond end + surrounding space (space before keyword)
    let delete_start = find_space_before(source, outer_kw_start);
    let delete_end = outer_cond_end;
    edits.push(Edit { start_offset: delete_start, end_offset: delete_end, replacement: "".to_string() });

    Some(Correction { edits })
}

/// Modifier outer unless: `inner_block unless outer_cond`
fn build_correction_outer_modifier_unless(
    node: &ruby_prism::UnlessNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<Correction> {
    let outer_kw_loc = node.keyword_loc();
    let outer_cond = node.predicate();
    let outer_cond_start = outer_cond.location().start_offset();
    let outer_cond_end = outer_cond.location().end_offset();
    let outer_kw_start = outer_kw_loc.start_offset();

    let outer_chainable = chainable_unless_cond_node(&outer_cond, source);
    let inner_chainable = &inner.chainable_cond;

    let mut edits = Vec::new();

    // Change inner keyword to `if` if it's `unless`
    if inner.is_unless {
        edits.push(Edit { start_offset: inner.keyword_start, end_offset: inner.keyword_end, replacement: "if".to_string() });
    }

    // Replace inner condition with chainable form
    edits.push(Edit { start_offset: inner.cond_start, end_offset: inner.cond_end, replacement: inner_chainable.clone() });

    // Insert outer chainable before inner condition
    edits.push(Edit { start_offset: inner.cond_start, end_offset: inner.cond_start, replacement: format!("{} && ", outer_chainable) });

    // Remove outer modifier
    let delete_start = find_space_before(source, outer_kw_start);
    let delete_end = outer_cond_end;
    edits.push(Edit { start_offset: delete_start, end_offset: delete_end, replacement: "".to_string() });

    Some(Correction { edits })
}

// ============= CHAINABLE CONDITION HELPERS =============

/// For an `if` node condition: add parens if needed, no negation
/// Translates RuboCop's `add_parentheses_if_needed` for `if` conditions.
fn chainable_if_cond(cond_node: &Node, _cond_src: &str, source: &str) -> String {
    add_parentheses_if_needed(cond_node, source)
}

/// For an inner `if` condition given only cond start/end (no full Node available)
/// Used when we only stored cond range in InnerConditional
fn chainable_if_cond_str(cond_src: &str, _source: &str, _cond_start: usize) -> String {
    // Without AST node, use source heuristics
    add_parentheses_if_needed_str(cond_src)
}

/// For an `unless` condition: negate it (string-based, used when AST not available)
fn chainable_unless_cond_str(cond_src: &str, _source: &str, _cond_start: usize) -> String {
    // RuboCop: if condition.and_type? → `"!(#{wrapped})"`, else `"!#{wrapped}"`
    let wrapped = add_parentheses_if_needed_str(cond_src);
    if is_and_type_expression(cond_src) {
        format!("!({})", wrapped)
    } else {
        format!("!{}", wrapped)
    }
}

/// For an `unless` condition: negate it (AST-based, preferred)
fn chainable_unless_cond_node(cond_node: &Node, source: &str) -> String {
    let wrapped = add_parentheses_if_needed(cond_node, source);
    // and_type? → `!(expr)` (the whole and-chain needs parens around it too)
    if is_and_node(cond_node) {
        format!("!({})", wrapped)
    } else {
        format!("!{}", wrapped)
    }
}

/// Translates RuboCop's `add_parentheses_if_needed` using source heuristics
/// Returns the condition source possibly wrapped in parens
fn add_parentheses_if_needed_str(cond_src: &str) -> String {
    // Check if we need to check send_node (block case handled separately at caller)
    if add_parentheses_needed_str(cond_src) {
        if is_parenthesize_method_str(cond_src) {
            parenthesized_method_args_str(cond_src)
        } else if is_and_type_expression(cond_src) {
            // and_type with assignment: use parenthesized_and logic
            parenthesized_and_str(cond_src)
        } else {
            format!("({})", cond_src)
        }
    } else {
        cond_src.to_string()
    }
}

/// Translates RuboCop's `add_parentheses_if_needed` using AST node
fn add_parentheses_if_needed(cond_node: &Node, source: &str) -> String {
    let cond_src = &source[cond_node.location().start_offset()..cond_node.location().end_offset()];

    // prefix_not? check: `not expr` form
    if cond_src.starts_with("not ") {
        return format!("({})", cond_src);
    }

    // Check for block type - use send_node
    let node_to_check = get_send_node_if_block(cond_node);

    if add_parentheses_needed_node(node_to_check, source) {
        // If node has a block (do...end), don't use parenthesize_method — wrap whole expr
        let has_block = match node_to_check {
            Node::CallNode { .. } => node_to_check.as_call_node().unwrap().block().is_some(),
            _ => false,
        };
        if !has_block && is_parenthesize_method_node(cond_node, source) {
            parenthesized_method_args_node(cond_node, source)
        } else if is_and_node(cond_node) {
            parenthesized_and_node(cond_node, source)
        } else {
            format!("({})", cond_src)
        }
    } else {
        cond_src.to_string()
    }
}

/// Get the send node if node is a block type, else return same node
fn get_send_node_if_block<'a>(node: &'a Node<'a>) -> &'a Node<'a> {
    match node {
        Node::BlockNode { .. } => {
            let block = node.as_block_node().unwrap();
            // BlockNode has a `call` field which is the send node
            // We can't easily get it without lifetime issues; fall through to node itself
            // Use the node as-is for now
            let _ = block;
            node
        }
        _ => node,
    }
}

/// Whether we need parens for node-based check (source needed for `(` vs `[` distinction)
fn add_parentheses_needed_node(node: &Node, source: &str) -> bool {
    match node {
        Node::LocalVariableWriteNode { .. }
        | Node::InstanceVariableWriteNode { .. }
        | Node::ClassVariableWriteNode { .. }
        | Node::GlobalVariableWriteNode { .. }
        | Node::ConstantWriteNode { .. }
        | Node::LocalVariableOperatorWriteNode { .. }
        | Node::InstanceVariableOperatorWriteNode { .. }
        | Node::MultiWriteNode { .. } => true,
        Node::OrNode { .. } => true,
        Node::AndNode { .. } => {
            // assignment_in_and?: has any assignment descendant
            has_assignment_descendant(node)
        }
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            // `parenthesized?` in RuboCop = opening paren is `(` specifically (not `[` subscript)
            // h[:a] → opening_loc=`[` → NOT parenthesized → needs wrapping
            // foo(x) → opening_loc=`(` → parenthesized → no wrapping needed
            // ok? bar → opening_loc=None → NOT parenthesized → needs wrapping
            let args_count = call.arguments().map(|a| a.arguments().iter().count()).unwrap_or(0);
            let has_block = call.block().is_some();
            let has_explicit_parens = call.opening_loc().map(|loc| {
                let off = loc.start_offset();
                off < source.len() && source.as_bytes()[off] == b'('
            }).unwrap_or(false);
            // needs parens if: has args without explicit `(` parens, OR has block (do/end ambiguity)
            (args_count > 0 && !has_explicit_parens) || (has_block && !has_explicit_parens)
        }
        _ => false,
    }
}

fn has_assignment_descendant(node: &Node) -> bool {
    match node {
        Node::LocalVariableWriteNode { .. }
        | Node::InstanceVariableWriteNode { .. }
        | Node::ClassVariableWriteNode { .. }
        | Node::GlobalVariableWriteNode { .. }
        | Node::ConstantWriteNode { .. }
        | Node::LocalVariableOperatorWriteNode { .. }
        | Node::InstanceVariableOperatorWriteNode { .. }
        | Node::MultiWriteNode { .. } => true,
        Node::AndNode { .. } => {
            let and_node = node.as_and_node().unwrap();
            has_assignment_descendant(&and_node.left()) || has_assignment_descendant(&and_node.right())
        }
        Node::OrNode { .. } => {
            let or_node = node.as_or_node().unwrap();
            has_assignment_descendant(&or_node.left()) || has_assignment_descendant(&or_node.right())
        }
        _ => false,
    }
}

fn is_and_node(node: &Node) -> bool {
    matches!(node, Node::AndNode { .. })
}

fn is_parenthesize_method_node(node: &Node, source: &str) -> bool {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        let args_count = call.arguments().map(|a| a.arguments().iter().count()).unwrap_or(0);
        // `parenthesized?` = opening_loc is `(` specifically
        let has_explicit_parens = call.opening_loc().map(|loc| {
            let off = loc.start_offset();
            off < source.len() && source.as_bytes()[off] == b'('
        }).unwrap_or(false);
        // not comparison, not operator: check message name
        let msg = call.name();
        let msg_str = String::from_utf8_lossy(msg.as_slice());
        let is_comparison = matches!(msg_str.as_ref(), "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>" | "=~" | "!~");
        let is_operator = msg_str.ends_with('=') || msg_str.len() <= 3 && msg_str.chars().all(|c| !c.is_alphanumeric() && c != '?' && c != '!' && c != '_');
        args_count > 0 && !has_explicit_parens && !is_comparison && !is_operator
    } else {
        false
    }
}

fn parenthesized_method_args_node(node: &Node, source: &str) -> String {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        // method_call = from node start to selector end
        // arguments = from first_arg start to node end
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let msg_loc = call.message_loc();
        if let Some(msg_l) = msg_loc {
            let method_call = &source[node_start..msg_l.end_offset()];
            let args_src = if let Some(args) = call.arguments() {
                let args_items: Vec<_> = args.arguments().iter().collect();
                if !args_items.is_empty() {
                    let first_start = args_items[0].location().start_offset();
                    &source[first_start..node_end]
                } else {
                    ""
                }
            } else { "" };
            if args_src.is_empty() {
                format!("({})", &source[node_start..node_end])
            } else {
                format!("{}({})", method_call, args_src)
            }
        } else {
            format!("({})", &source[node_start..node_end])
        }
    } else {
        let s = &source[node.location().start_offset()..node.location().end_offset()];
        format!("({})", s)
    }
}

// String-based heuristics (used when we don't have full AST node)

fn add_parentheses_needed_str(cond_src: &str) -> bool {
    // assignment at top level
    if is_assignment_expression(cond_src) {
        return true;
    }
    // or_type (||/or) at top level
    if is_or_expression(cond_src) {
        return true;
    }
    // and_type with assignment
    if is_and_with_assignment(cond_src) {
        return true;
    }
    // prefix `not`
    if cond_src.starts_with("not ") {
        return true;
    }
    // method call with unparens args: heuristic — if it looks like `obj.method arg`
    if is_method_call_without_parens(cond_src) {
        return true;
    }
    false
}

fn is_and_type_expression(s: &str) -> bool {
    // Ruby `and` keyword (low precedence) at top level
    contains_top_level_op(s, &[" and "])
}

fn is_parenthesize_method_str(s: &str) -> bool {
    is_method_call_without_parens(s)
}

fn parenthesized_method_args_str(cond_src: &str) -> String {
    // Transform `obj.method args` → `obj.method(args)`
    // Find the last space before the args part
    // Simple heuristic: find the method name end, then args start
    // `ok? bar` → `ok?(bar)`
    // `foo.is_a? Foo` → `foo.is_a?(Foo)`
    // `foo.bar arg1, arg2` → `foo.bar(arg1, arg2)`
    let bytes = cond_src.as_bytes();
    let mut depth = 0i32;
    let mut last_method_end = None;

    // Find the selector end: after last `.` or at start for bare methods
    // Walk to find where the method name ends and args begin
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => { depth += 1; i += 1; }
            b')' | b']' | b'}' => { depth -= 1; i += 1; }
            b' ' if depth == 0 => {
                last_method_end = Some(i);
                break;
            }
            _ => { i += 1; }
        }
    }

    if let Some(method_end) = last_method_end {
        let method_part = &cond_src[..method_end];
        let args_part = &cond_src[method_end+1..];
        format!("{}({})", method_part, args_part)
    } else {
        format!("({})", cond_src)
    }
}

/// AST-based parenthesized_and: uses node structure to preserve whitespace correctly
/// `baz &&\n   foo = bar` → `baz &&\n   (foo = bar)` (wraps only the rhs assignment)
fn parenthesized_and_node(node: &Node, source: &str) -> String {
    if let Node::AndNode { .. } = node {
        let and_node = node.as_and_node().unwrap();
        let left = and_node.left();
        let right = and_node.right();
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let left_end = left.location().end_offset();
        let right_start = right.location().start_offset();
        let right_end = right.location().end_offset();

        // lhs = source up to left end
        let lhs_src = &source[node_start..left_end];
        // operator+whitespace = source between left end and right start
        let op_src = &source[left_end..right_start];
        // rhs = source of right node
        let rhs_src = &source[right_start..right_end];

        // Apply parenthesized_and_clause logic to rhs AST node
        let rhs_processed = parenthesized_and_clause_node(&right, rhs_src, source);

        format!("{}{}{}", lhs_src, op_src, rhs_processed)
    } else {
        // fallback: string-based
        let cond_src = &source[node.location().start_offset()..node.location().end_offset()];
        parenthesized_and_str(cond_src)
    }
}

fn parenthesized_and_clause_node(node: &Node, node_src: &str, source: &str) -> String {
    if let Node::AndNode { .. } = node {
        // Recurse: lhs stays, rhs gets processed
        parenthesized_and_node(node, source)
    } else if is_assignment_node(node) {
        format!("({})", node_src)
    } else {
        node_src.to_string()
    }
}

fn is_assignment_node(node: &Node) -> bool {
    matches!(node,
        Node::LocalVariableWriteNode { .. }
        | Node::InstanceVariableWriteNode { .. }
        | Node::ClassVariableWriteNode { .. }
        | Node::GlobalVariableWriteNode { .. }
        | Node::ConstantWriteNode { .. }
        | Node::LocalVariableOperatorWriteNode { .. }
        | Node::InstanceVariableOperatorWriteNode { .. }
        | Node::MultiWriteNode { .. }
    )
}

/// Transform and-chain: only wrap assignment clauses
/// `foo = bar and baz` → `foo = bar and baz` (and keyword: no change to structure)
/// `baz && (foo = bar) and fred` → `baz && (foo = bar) and (fred = garply)`
fn parenthesized_and_str(s: &str) -> String {
    // Split on top-level `and` (low-prec) first, then handle `&&` within each segment
    // RuboCop's parenthesized_and:
    //   lhs = node.lhs.source
    //   rhs = parenthesized_and_clause(node.rhs)
    //   operator = ` && ` or ` and ` (with surrounding space)
    // parenthesized_and_clause(node):
    //   if and_type → recurse
    //   if assignment → `"(#{node.source})"`
    //   else → node.source
    //
    // Applied recursively on AndNode tree (right-associative splits)

    // We'll simulate by finding top-level `&&` or `and` operators and processing rhs
    let result = parenthesized_and_rhs(s);
    result
}

fn parenthesized_and_rhs(s: &str) -> String {
    // Find rightmost top-level operator (`&&` or ` and `) and split
    // RuboCop: lhs stays unchanged, only rhs gets parenthesized_and_clause treatment
    let (lhs, op, rhs) = split_rightmost_and(s);
    if let Some(op_str) = op {
        let rhs_processed = parenthesized_and_clause_str(rhs);
        format!("{}{}{}", lhs, op_str, rhs_processed)
    } else {
        s.to_string()
    }
}

fn parenthesized_and_clause_str(s: &str) -> String {
    if is_and_with_assignment(s) || is_and_type_expression_with_assignment(s) {
        parenthesized_and_rhs(s)
    } else if is_assignment_expression(s) {
        format!("({})", s)
    } else {
        s.to_string()
    }
}

fn is_and_type_expression_with_assignment(s: &str) -> bool {
    contains_top_level_op(s, &[" and "]) && is_assignment_expression(s)
}

/// Split string at rightmost top-level `&&` or ` and ` operator
/// Returns (lhs, Some(op_with_surrounding_space), rhs) or (s, None, "")
/// The operator includes surrounding whitespace (matches RuboCop's range_with_surrounding_space)
fn split_rightmost_and(s: &str) -> (&str, Option<&str>, &str) {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut last_and_pos: Option<(usize, usize)> = None; // (token_start, token_end)

    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => { depth += 1; i += 1; }
            b')' | b']' | b'}' => { depth -= 1; i += 1; }
            b'&' if depth == 0 && i + 1 < bytes.len() && bytes[i+1] == b'&' => {
                last_and_pos = Some((i, i + 2));
                i += 2;
            }
            b' ' if depth == 0 => {
                if s[i..].starts_with(" and ") {
                    // ` and ` includes the spaces
                    last_and_pos = Some((i, i + 5));
                    i += 5;
                } else {
                    i += 1;
                }
            }
            _ => { i += 1; }
        }
    }

    if let Some((token_start, token_end)) = last_and_pos {
        // Expand to include surrounding spaces: lhs trailing space, rhs leading space
        let lhs_end = {
            let mut e = token_start;
            while e > 0 && bytes[e-1] == b' ' { e -= 1; }
            e
        };
        let rhs_start = {
            let mut s2 = token_end;
            while s2 < bytes.len() && bytes[s2] == b' ' { s2 += 1; }
            s2
        };
        let op = &s[lhs_end..rhs_start];
        (&s[..lhs_end], Some(op), &s[rhs_start..])
    } else {
        (s, None, "")
    }
}

fn is_method_call_without_parens(s: &str) -> bool {
    // Heuristic: method call with space before args, no parens
    // `ok? bar` → true; `foo.is_a? Foo` → true; `foo.bar(x)` → false
    // Look for pattern: identifier/? followed by space followed by non-operator arg
    // But exclude: `foo == bar`, `foo && bar` etc (binary ops)

    // Check if there's a space at top level that isn't preceded by an operator char
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => { depth += 1; i += 1; }
            b')' | b']' | b'}' => { depth -= 1; i += 1; }
            b' ' if depth == 0 && i > 0 => {
                let prev = bytes[i-1];
                // If preceded by `?` or `!` (method name ending), it's a method call
                if prev == b'?' || prev == b'!' {
                    // Check not `!=`
                    if i >= 2 && bytes[i-2] != b'!' {
                        return true;
                    }
                    if prev == b'?' {
                        return true;
                    }
                }
                // If preceded by alphanum or `_` (regular method name): check it's not an operator
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    // Peek what's after the space — if it's an alphanumeric or `[` it's args
                    let next = if i + 1 < bytes.len() { bytes[i+1] } else { 0 };
                    if next.is_ascii_alphanumeric() || next == b'[' || next == b':' || next == b'"' || next == b'\'' {
                        // But we need to exclude `and`/`or` keywords and `do`
                        let word = &s[i+1..];
                        if !word.starts_with("and ") && !word.starts_with("or ") && !word.starts_with("do ") && !word.starts_with("do\n") {
                            return true;
                        }
                    }
                }
                i += 1;
            }
            _ => { i += 1; }
        }
    }
    false
}

fn is_or_expression(s: &str) -> bool {
    contains_top_level_op(s, &["||", " or "])
}

fn is_assignment_expression(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 => {
                let prev = if i > 0 { bytes[i-1] } else { 0 };
                let next = if i+1 < bytes.len() { bytes[i+1] } else { 0 };
                if prev != b'!' && prev != b'<' && prev != b'>' && prev != b'=' && prev != b'~' &&
                   next != b'=' && next != b'>' {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn is_and_with_assignment(s: &str) -> bool {
    if !contains_top_level_op(s, &["&&", " and "]) {
        return false;
    }
    // Check if any clause has assignment (not just top-level = but within &&-chain clauses)
    // Walk and check for assignment at any depth of && chain
    has_assignment_in_and_chain(s)
}

fn has_assignment_in_and_chain(s: &str) -> bool {
    // Split on top-level && and check each part for assignment
    let (lhs, op, rhs) = split_rightmost_and(s);
    if op.is_none() {
        return is_assignment_expression(s);
    }
    is_assignment_expression(lhs) || is_assignment_expression(rhs)
        || has_assignment_in_and_chain(lhs) || has_assignment_in_and_chain(rhs)
}

fn contains_top_level_op(s: &str, ops: &[&str]) -> bool {
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if depth == 0 => {
                for op in ops {
                    if s[i..].starts_with(op) {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

// ============= COMMENT HELPERS =============

/// Extract comment text that appears between outer if keyword and inner node start
fn extract_comments_before_inner(
    outer: &ruby_prism::IfNode,
    inner: &InnerConditional,
    source: &str,
) -> String {
    // Comments appear as `# ...\n` on lines before the inner conditional keyword
    // They are between the outer condition end and inner node start
    let outer_cond_end = outer.predicate().location().end_offset();
    extract_comment_lines(source, outer_cond_end, inner.node_start)
}

fn extract_comments_before_inner_unless(
    outer: &ruby_prism::UnlessNode,
    inner: &InnerConditional,
    source: &str,
) -> String {
    let outer_cond_end = outer.predicate().location().end_offset();
    extract_comment_lines(source, outer_cond_end, inner.node_start)
}

fn extract_comment_lines(source: &str, range_start: usize, range_end: usize) -> String {
    if range_start >= range_end {
        return String::new();
    }
    let region = &source[range_start..range_end];
    let mut comments = Vec::new();
    for line in region.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            comments.push(trimmed.to_string());
        }
    }
    if comments.is_empty() {
        String::new()
    } else {
        comments.join("\n") + "\n"
    }
}

/// Find the range of comment lines to remove from between outer cond and inner node
fn comment_range_before_inner(
    outer: &ruby_prism::IfNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<(usize, usize)> {
    let outer_cond_end = outer.predicate().location().end_offset();
    find_comment_lines_range(source, outer_cond_end, inner.node_start)
}

fn comment_range_before_inner_unless(
    outer: &ruby_prism::UnlessNode,
    inner: &InnerConditional,
    source: &str,
) -> Option<(usize, usize)> {
    let outer_cond_end = outer.predicate().location().end_offset();
    find_comment_lines_range(source, outer_cond_end, inner.node_start)
}

fn find_comment_lines_range(source: &str, range_start: usize, range_end: usize) -> Option<(usize, usize)> {
    if range_start >= range_end {
        return None;
    }
    let region = &source[range_start..range_end];
    // Find first comment line start and last comment line end
    let mut first_start: Option<usize> = None;
    let mut last_end: Option<usize> = None;
    let mut offset = range_start;
    for line in region.split('\n') {
        let line_with_nl = if offset + line.len() < source.len() {
            line.len() + 1 // include \n
        } else {
            line.len()
        };
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            if first_start.is_none() {
                first_start = Some(find_line_start(source, offset));
            }
            last_end = Some(find_line_end_including_newline(source, offset + line.len()));
        }
        offset += line_with_nl;
    }
    match (first_start, last_end) {
        (Some(s), Some(e)) => Some((s, e)),
        _ => None,
    }
}

// ============= UTILITY HELPERS =============

fn find_line_start(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = offset;
    // Don't go past end
    if i >= bytes.len() {
        i = bytes.len().saturating_sub(1);
    }
    while i > 0 && bytes[i - 1] != b'\n' {
        i -= 1;
    }
    i
}

fn find_line_end_including_newline(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    if i < bytes.len() {
        i + 1 // include the \n
    } else {
        i
    }
}

fn find_space_before(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i > 0 && bytes[i - 1] == b' ' {
        i -= 1;
    }
    i
}

// ============= EXISTING DETECTION HELPERS =============

fn is_ternary_if(node: &ruby_prism::IfNode, _source: &str) -> bool {
    node.if_keyword_loc().is_none()
}

fn is_elsif(node: &ruby_prism::IfNode, source: &str) -> bool {
    node.if_keyword_loc().map_or(false, |loc| {
        source[loc.start_offset()..].starts_with("elsif")
    })
}

fn is_modifier_form_if(node: &ruby_prism::IfNode, _source: &str) -> bool {
    // Modifier form if: no end_keyword_loc and has if_keyword_loc (not ternary)
    node.if_keyword_loc().is_some() && node.end_keyword_loc().is_none()
}

fn is_modifier_form_unless(node: &ruby_prism::UnlessNode, _source: &str) -> bool {
    node.end_keyword_loc().is_none()
}

fn if_keyword_text_if<'a>(node: &ruby_prism::IfNode, source: &'a str) -> &'a str {
    if let Some(kw_loc) = node.if_keyword_loc() {
        &source[kw_loc.start_offset()..kw_loc.end_offset()]
    } else {
        "if"
    }
}

/// Check if condition has an assignment whose variable is used in the inner branch's condition
fn use_variable_assignment_in_condition_if(
    node: &ruby_prism::IfNode,
    inner: &InnerConditional,
    source: &str,
) -> bool {
    let condition = node.predicate();
    let assigned = collect_assigned_variables(&condition, source);
    if assigned.is_empty() {
        return false;
    }

    // The inner conditional must be an if_type (not unless) for this check
    // And its condition source must match one of the assigned variables
    if let Some(stmts) = node.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() == 1 {
            if let Node::IfNode { .. } = &body[0] {
                let inner_if = body[0].as_if_node().unwrap();
                let inner_cond = inner_if.predicate();
                let inner_cond_src =
                    &source[inner_cond.location().start_offset()..inner_cond.location().end_offset()];
                if assigned.contains(&inner_cond_src.to_string()) {
                    return true;
                }
            }
        }
    }
    false
}

fn use_variable_assignment_in_condition_unless(
    node: &ruby_prism::UnlessNode,
    inner: &InnerConditional,
    source: &str,
) -> bool {
    let condition = node.predicate();
    let assigned = collect_assigned_variables(&condition, source);
    if assigned.is_empty() {
        return false;
    }

    if let Some(stmts) = node.statements() {
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() == 1 {
            if let Node::IfNode { .. } = &body[0] {
                let inner_if = body[0].as_if_node().unwrap();
                let inner_cond = inner_if.predicate();
                let inner_cond_src =
                    &source[inner_cond.location().start_offset()..inner_cond.location().end_offset()];
                if assigned.contains(&inner_cond_src.to_string()) {
                    return true;
                }
            }
        }
    }
    let _ = inner;
    false
}

/// Collect variable names assigned in a condition (e.g., `foo = bar` assigns "foo")
fn collect_assigned_variables(node: &Node, source: &str) -> Vec<String> {
    let mut result = Vec::new();
    collect_assigned_variables_inner(node, source, &mut result);
    result
}

fn collect_assigned_variables_inner(node: &Node, source: &str, result: &mut Vec<String>) {
    match node {
        Node::LocalVariableWriteNode { .. } => {
            let write = node.as_local_variable_write_node().unwrap();
            let name = String::from_utf8_lossy(write.name().as_slice()).to_string();
            result.push(name);
        }
        Node::LocalVariableOperatorWriteNode { .. }
        | Node::LocalVariableAndWriteNode { .. }
        | Node::LocalVariableOrWriteNode { .. } => {
            // Extract variable name from the node source - first token
            let loc = node.location();
            let src = &source[loc.start_offset()..loc.end_offset()];
            if let Some(name) = src.split(|c: char| !c.is_alphanumeric() && c != '_').next() {
                if !name.is_empty() {
                    result.push(name.to_string());
                }
            }
        }
        Node::AndNode { .. } => {
            let and_node = node.as_and_node().unwrap();
            collect_assigned_variables_inner(&and_node.left(), source, result);
            collect_assigned_variables_inner(&and_node.right(), source, result);
        }
        Node::OrNode { .. } => {
            let or_node = node.as_or_node().unwrap();
            collect_assigned_variables_inner(&or_node.left(), source, result);
            collect_assigned_variables_inner(&or_node.right(), source, result);
        }
        Node::ParenthesesNode { .. } => {
            let paren = node.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                collect_assigned_variables_inner(&body, source, result);
            }
        }
        _ => {}
    }
}

impl Visit<'_> for SoleNestedConditionalVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if_node(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless_node(node);
        ruby_prism::visit_unless_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { allow_modifier: bool }

crate::register_cop!("Style/SoleNestedConditional", |cfg| {
    let c: Cfg = cfg.typed("Style/SoleNestedConditional");
    Some(Box::new(SoleNestedConditional::with_config(c.allow_modifier)))
});
