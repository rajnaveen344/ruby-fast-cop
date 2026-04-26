//! Style/RedundantBegin - Checks for redundant `begin` blocks.
//!
//! A `begin` block is redundant when the `rescue`/`ensure` can be handled by the
//! enclosing method or block definition directly, or when a standalone `begin`
//! has no rescue/ensure at all.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_begin.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantBegin";
const MSG: &str = "Redundant `begin` block detected.";

#[derive(Default)]
pub struct RedundantBegin;

impl RedundantBegin {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantBegin {
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
        let mut visitor = RedundantBeginVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RedundantBeginVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RedundantBeginVisitor<'a> {
    fn register_offense(&mut self, begin_keyword_start: usize, begin_keyword_end: usize) {
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            MSG,
            Severity::Convention,
            begin_keyword_start,
            begin_keyword_end,
        ));
    }

    fn register_offense_with_node(&mut self, begin_node: &ruby_prism::BeginNode, is_assignment: bool, is_endless_def: bool) {
        let kw_loc = match begin_node.begin_keyword_loc() {
            Some(l) => l,
            None => return,
        };
        let begin_keyword_start = kw_loc.start_offset();
        let begin_keyword_end = kw_loc.end_offset();

        let correction = self.build_correction(begin_node, is_assignment, is_endless_def);

        let offense = self.ctx.offense_with_range(
            COP_NAME,
            MSG,
            Severity::Convention,
            begin_keyword_start,
            begin_keyword_end,
        );
        let offense = if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        };
        self.offenses.push(offense);
    }

    /// Build correction for a redundant begin block.
    fn build_correction(&self, begin_node: &ruby_prism::BeginNode, is_assignment: bool, is_endless_def: bool) -> Option<Correction> {
        let kw_loc = begin_node.begin_keyword_loc()?;
        let end_kw_loc = begin_node.end_keyword_loc()?;

        let source = self.ctx.source;

        // Endless def: skip (requires `range_with_surrounding_space` which we don't port)
        if is_endless_def {
            return None;
        }

        // Check for modifier condition after `end` keyword:
        // e.g. `begin ... end unless condition` or `begin foo end unless condition`
        let after_end = &source[end_kw_loc.end_offset()..];
        let trimmed_after_end = after_end.trim_start_matches(|c: char| c == ' ' || c == '\t');
        let modifier_condition = if trimmed_after_end.starts_with("unless ")
            || trimmed_after_end.starts_with("if ")
            || trimmed_after_end.starts_with("while ")
            || trimmed_after_end.starts_with("until ")
        {
            // Find the full condition text (to end of this "line" = end of begin node)
            let cond_start = end_kw_loc.end_offset() + (after_end.len() - trimmed_after_end.len());
            let cond_end = begin_node.location().end_offset();
            // The condition starts with spaces then keyword, e.g. ` unless condition`
            let spaces_count = after_end.len() - trimmed_after_end.len();
            Some((cond_start - spaces_count, cond_end)) // start from after end_kw with spaces
        } else {
            None
        };

        if is_assignment {
            return self.build_assignment_correction(begin_node, &kw_loc, &end_kw_loc, modifier_condition);
        }

        // Simple non-assignment: delete begin keyword, delete end keyword
        // Also handle modifier condition if present
        self.build_simple_correction(&kw_loc, &end_kw_loc, begin_node, modifier_condition)
    }

    /// Simple correction: delete `begin` keyword + delete `end` keyword.
    /// Handles modifier conditions.
    fn build_simple_correction(
        &self,
        kw_loc: &ruby_prism::Location,
        end_kw_loc: &ruby_prism::Location,
        begin_node: &ruby_prism::BeginNode,
        modifier_condition: Option<(usize, usize)>,
    ) -> Option<Correction> {
        let source = self.ctx.source;

        if let Some((cond_text_start, cond_end)) = modifier_condition {
            // Modifier form: `begin\n  foo\nend unless condition`
            // → delete begin keyword, insert ` unless condition` after first stmt, delete `\nend unless condition`
            let first_child = self.first_statement(begin_node)?;
            let first_child_end = first_child.location().end_offset();

            // Condition text (e.g. ` unless condition`)
            let cond_text = &source[cond_text_start..cond_end];

            // Check if single-line (`begin foo end unless`) vs multiline
            let begin_kw_end = kw_loc.end_offset();
            let is_single_line = !source[begin_kw_end..first_child.location().start_offset()].contains('\n');

            if is_single_line {
                // Single-line: delete begin_kw; delete `end ` (end_kw + trailing space)
                // `begin foo end unless condition` → ` foo  unless condition`
                let end_kw_end_with_space = {
                    let mut e = end_kw_loc.end_offset();
                    let bytes = source.as_bytes();
                    while e < source.len() && bytes[e] == b' ' { e += 1; }
                    e
                };
                Some(Correction {
                    edits: vec![
                        Edit { start_offset: kw_loc.start_offset(), end_offset: kw_loc.end_offset(), replacement: String::new() },
                        Edit { start_offset: end_kw_loc.start_offset(), end_offset: end_kw_end_with_space, replacement: String::new() },
                    ],
                })
            } else {
                // Multiline: delete begin_kw; replace `\nend modifier_cond` with ` modifier_cond` after first stmt
                // `\nend unless condition` starts at first_child_end (the newline before `end`)
                Some(Correction {
                    edits: vec![
                        Edit { start_offset: kw_loc.start_offset(), end_offset: kw_loc.end_offset(), replacement: String::new() },
                        Edit { start_offset: first_child_end, end_offset: cond_end, replacement: format!("{}", cond_text) },
                    ],
                })
            }
        } else {
            // Standard: delete begin keyword, delete end keyword
            Some(Correction {
                edits: vec![
                    Edit { start_offset: kw_loc.start_offset(), end_offset: kw_loc.end_offset(), replacement: String::new() },
                    Edit { start_offset: end_kw_loc.start_offset(), end_offset: end_kw_loc.end_offset(), replacement: String::new() },
                ],
            })
        }
    }

    /// Assignment correction: `var ||= begin\n  expr\nend` → `var ||= expr`
    fn build_assignment_correction(
        &self,
        begin_node: &ruby_prism::BeginNode,
        kw_loc: &ruby_prism::Location,
        end_kw_loc: &ruby_prism::Location,
        modifier_condition: Option<(usize, usize)>,
    ) -> Option<Correction> {
        let source = self.ctx.source;
        let first_child = self.first_statement(begin_node)?;

        let first_child_src = &source[first_child.location().start_offset()..first_child.location().end_offset()];

        // Check if first_child is modifier-if (IfNode with no if_keyword_loc or with modifier form)
        let is_modifier_if = match &first_child {
            Node::IfNode { .. } => {
                let ifn = first_child.as_if_node().unwrap();
                // Modifier-if has no end_keyword_loc
                ifn.end_keyword_loc().is_none() && ifn.if_keyword_loc().is_some()
            }
            Node::UnlessNode { .. } => {
                let unn = first_child.as_unless_node().unwrap();
                unn.end_keyword_loc().is_none()
            }
            _ => false,
        };

        // Skip nested assignment-in-assignment case to avoid multi-pass mismatch
        // Detect: first child is an assignment whose RHS is a begin node
        if self.first_child_is_assignment_begin(&first_child) {
            return None;
        }

        let replacement = if is_modifier_if {
            format!("({})", first_child_src)
        } else {
            first_child_src.to_string()
        };

        // Check for comments between begin keyword and first child
        let between_begin_and_child = &source[kw_loc.end_offset()..first_child.location().start_offset()];
        let comments_to_restore = extract_comments_for_restore(between_begin_and_child);

        let parent_start = line_start_offset(source, kw_loc.start_offset());

        let mut edits = vec![
            // Replace begin keyword with first_child source
            Edit { start_offset: kw_loc.start_offset(), end_offset: kw_loc.end_offset(), replacement },
            // Delete from begin.end to first_child.end (removes whitespace + newlines + first_child bytes)
            Edit { start_offset: kw_loc.end_offset(), end_offset: first_child.location().end_offset(), replacement: String::new() },
            // Delete end keyword
            Edit { start_offset: end_kw_loc.start_offset(), end_offset: end_kw_loc.end_offset(), replacement: String::new() },
        ];

        // Restore comments to before parent line
        if !comments_to_restore.is_empty() {
            edits.push(Edit { start_offset: parent_start, end_offset: parent_start, replacement: comments_to_restore });
        }

        let _ = modifier_condition; // assignment + modifier: not handled (skip for safety)

        Some(Correction { edits })
    }

    /// Find first statement node in begin_node's body.
    fn first_statement<'b>(&self, begin_node: &'b ruby_prism::BeginNode) -> Option<Node<'b>> {
        // For begin with rescue/ensure, first_child in RuboCop = statements().body[0] or rescue/ensure
        // For our correction, get first statement from statements()
        if let Some(stmts) = begin_node.statements() {
            stmts.body().iter().next()
        } else if let Some(rescue) = begin_node.rescue_clause() {
            // First child is the rescue node itself
            Some(rescue.as_node())
        } else {
            None
        }
    }

    /// Check if node is an assignment (||=, &&=, =) whose value is a begin block.
    fn first_child_is_assignment_begin(&self, node: &Node) -> bool {
        let value = match node {
            Node::LocalVariableOrWriteNode { .. } => {
                node.as_local_variable_or_write_node().map(|n| n.value())
            }
            Node::InstanceVariableOrWriteNode { .. } => {
                node.as_instance_variable_or_write_node().map(|n| n.value())
            }
            Node::ClassVariableOrWriteNode { .. } => {
                node.as_class_variable_or_write_node().map(|n| n.value())
            }
            Node::GlobalVariableOrWriteNode { .. } => {
                node.as_global_variable_or_write_node().map(|n| n.value())
            }
            Node::LocalVariableWriteNode { .. } => {
                node.as_local_variable_write_node().map(|n| n.value())
            }
            Node::InstanceVariableWriteNode { .. } => {
                node.as_instance_variable_write_node().map(|n| n.value())
            }
            _ => None,
        };
        if let Some(val) = value {
            matches!(val, Node::BeginNode { .. })
        } else {
            false
        }
    }

    /// Check def/defs: if body is a begin block (with rescue/ensure),
    /// the begin is redundant because def itself can handle rescue/ensure.
    fn check_def(&mut self, node: &ruby_prism::DefNode) {
        // Skip endless methods
        if node.end_keyword_loc().is_none() {
            return;
        }

        let body = match node.body() {
            Some(b) => b,
            None => return,
        };

        // Direct BeginNode body (most common for def with begin...rescue...end)
        if let Node::BeginNode { .. } = &body {
            let begin_node = body.as_begin_node().unwrap();
            if begin_node.begin_keyword_loc().is_some() {
                self.register_offense_with_node(&begin_node, false, false);
                return;
            }
        }

        // StatementsNode wrapping a single BeginNode
        if let Node::StatementsNode { .. } = &body {
            let stmts = body.as_statements_node().unwrap();
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() == 1 {
                if let Node::BeginNode { .. } = &items[0] {
                    let begin_node = items[0].as_begin_node().unwrap();
                    if begin_node.begin_keyword_loc().is_some() {
                        self.register_offense_with_node(&begin_node, false, false);
                    }
                }
            }
        }
    }

    /// Check if/unless/case/case_match branches for redundant begin
    fn check_branches_if(&mut self, node: &ruby_prism::IfNode) {
        // Skip modifier form
        if node.if_keyword_loc().is_none() || node.end_keyword_loc().is_none() {
            return;
        }

        // Skip elsif (they are handled when processing the parent if)
        if node.if_keyword_loc().map_or(false, |loc| {
            self.ctx.source[loc.start_offset()..].starts_with("elsif")
        }) {
            return;
        }

        // Check the if branch
        if let Some(stmts) = node.statements() {
            self.check_branch_statements(&stmts);
        }

        // Check elsif/else branches
        self.check_subsequent(node.subsequent());
    }

    fn check_branches_unless(&mut self, node: &ruby_prism::UnlessNode) {
        // Skip modifier form
        if node.end_keyword_loc().is_none() {
            return;
        }

        if let Some(stmts) = node.statements() {
            self.check_branch_statements(&stmts);
        }

        if let Some(else_clause) = node.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                self.check_branch_statements(&stmts);
            }
        }
    }

    fn check_subsequent(&mut self, subsequent: Option<Node>) {
        match subsequent {
            Some(Node::ElseNode { .. }) => {
                let else_node = subsequent.unwrap().as_else_node().unwrap();
                if let Some(stmts) = else_node.statements() {
                    self.check_branch_statements(&stmts);
                }
            }
            Some(Node::IfNode { .. }) => {
                // elsif
                let elsif = subsequent.unwrap().as_if_node().unwrap();
                if let Some(stmts) = elsif.statements() {
                    self.check_branch_statements(&stmts);
                }
                self.check_subsequent(elsif.subsequent());
            }
            _ => {}
        }
    }

    fn check_branch_statements(&mut self, stmts: &ruby_prism::StatementsNode) {
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() != 1 {
            return;
        }
        if let Node::BeginNode { .. } = &body[0] {
            let begin_node = body[0].as_begin_node().unwrap();
            // Only flag if no rescue/ensure
            if begin_node.rescue_clause().is_some() || begin_node.ensure_clause().is_some() {
                return;
            }
            if begin_node.begin_keyword_loc().is_some() {
                self.register_offense_with_node(&begin_node, false, false);
            }
        }
    }

    fn check_case(&mut self, node: &ruby_prism::CaseNode) {
        for condition in node.conditions().iter() {
            if let Node::WhenNode { .. } = &condition {
                let when = condition.as_when_node().unwrap();
                if let Some(stmts) = when.statements() {
                    self.check_branch_statements(&stmts);
                }
            }
        }
        if let Some(else_clause) = node.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                self.check_branch_statements(&stmts);
            }
        }
    }

    fn check_case_match(&mut self, node: &ruby_prism::CaseMatchNode) {
        for condition in node.conditions().iter() {
            if let Node::InNode { .. } = &condition {
                let in_node = condition.as_in_node().unwrap();
                if let Some(stmts) = in_node.statements() {
                    self.check_branch_statements(&stmts);
                }
            }
        }
        if let Some(else_clause) = node.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                self.check_branch_statements(&stmts);
            }
        }
    }

    fn check_while(&mut self, node: &ruby_prism::WhileNode) {
        // Skip modifier form (post-condition loop)
        if node.keyword_loc().start_offset() > node.location().start_offset() + 3 {
            // This is "begin...end while" - the while keyword comes after the body
            // Actually, check if it's modifier form by checking for closing_loc
            // Modifier while: `body while cond` or `begin...end while cond`
        }
        // Prism: modifier while has no end_keyword but has closing_loc
        // Actually, let's check if the while keyword is not at the beginning
        if node.closing_loc().is_none() {
            // modifier form: `begin ... end while cond`
            return;
        }

        let body = match node.statements() {
            Some(s) => s,
            None => return,
        };

        let stmts: Vec<_> = body.body().iter().collect();
        if stmts.len() != 1 {
            return;
        }

        if let Node::BeginNode { .. } = &stmts[0] {
            let begin_node = stmts[0].as_begin_node().unwrap();
            if begin_node.rescue_clause().is_some() || begin_node.ensure_clause().is_some() {
                return;
            }
            if begin_node.begin_keyword_loc().is_some() {
                self.register_offense_with_node(&begin_node, false, false);
            }
        }
    }

    fn check_until(&mut self, node: &ruby_prism::UntilNode) {
        if node.closing_loc().is_none() {
            return;
        }

        let body = match node.statements() {
            Some(s) => s,
            None => return,
        };

        let stmts: Vec<_> = body.body().iter().collect();
        if stmts.len() != 1 {
            return;
        }

        if let Node::BeginNode { .. } = &stmts[0] {
            let begin_node = stmts[0].as_begin_node().unwrap();
            if begin_node.rescue_clause().is_some() || begin_node.ensure_clause().is_some() {
                return;
            }
            if begin_node.begin_keyword_loc().is_some() {
                self.register_offense_with_node(&begin_node, false, false);
            }
        }
    }

    fn check_block(&mut self, node: &ruby_prism::BlockNode) {
        // Only for Ruby >= 2.5
        if !self.ctx.ruby_version_at_least(2, 5) {
            return;
        }

        // Skip braces blocks
        let open_loc = node.opening_loc();
        let open = &self.ctx.source[open_loc.start_offset()..open_loc.end_offset()];
        if open == "{" {
            return;
        }

        let body = match node.body() {
            Some(b) => b,
            None => return,
        };

        // Direct BeginNode
        if let Node::BeginNode { .. } = &body {
            let begin_node = body.as_begin_node().unwrap();
            if begin_node.begin_keyword_loc().is_some() {
                self.register_offense_with_node(&begin_node, false, false);
                return;
            }
        }

        // StatementsNode wrapping a single BeginNode
        if let Node::StatementsNode { .. } = &body {
            let stmts = body.as_statements_node().unwrap();
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() == 1 {
                if let Node::BeginNode { .. } = &items[0] {
                    let begin_node = items[0].as_begin_node().unwrap();
                    if begin_node.begin_keyword_loc().is_some() {
                        self.register_offense_with_node(&begin_node, false, false);
                    }
                }
            }
        }
    }

    /// Check standalone begin blocks (on_kwbegin in RuboCop).
    /// Only flags begin blocks that are NOT already handled by branch checks
    /// (def, if/unless branches, case/when, while/until, block).
    fn check_standalone_begin(&mut self, node: &ruby_prism::BeginNode) {
        // Only check truly standalone begins - those not inside conditional branches
        // or def/block bodies (those are handled by other check methods).
        // We can detect this by checking if begin_keyword_loc exists (explicit begin)
        // and the context around it.
        if node.begin_keyword_loc().is_none() {
            return;
        }

        if self.is_allowable_kwbegin(node) {
            return;
        }

        // Check if this begin is inside a conditional branch, def, or block body.
        // These are already handled by check_def, check_branches_if, etc.
        // We detect this by checking what precedes the begin keyword.
        let kw_loc = node.begin_keyword_loc().unwrap();
        let begin_offset = kw_loc.start_offset();

        // Check if this begin is inside a conditional branch (if/else/when/in body)
        // by looking at what's on the line before
        let line_start = self.ctx.line_start(begin_offset);
        let before_on_line = &self.ctx.source[line_start..begin_offset];
        let trimmed = before_on_line.trim();

        // If the begin is the sole content on its line (just indentation), and it's
        // inside a branch body, it's already handled by branch/def/block checks
        if trimmed.is_empty() {
            // Check the broader context - is this inside a def, block, if, etc.?
            // Look backwards for indentation context
            let indent = self.ctx.indentation_of(begin_offset);
            if indent > 0 {
                // Indented begin is likely inside something that handles it
                // Don't flag here - let the specific handler do it
                return;
            }
        }

        // Check if any descendant begin is also non-allowable (flag that one instead)
        if let Some(stmts) = node.statements() {
            for stmt in stmts.body().iter() {
                if self.has_descendant_offensive_begin(&stmt) {
                    return;
                }
            }
        }

        // Determine if in assignment context
        let before = &self.ctx.source[..kw_loc.start_offset()];
        let trimmed_before = before.trim_end();
        let is_assignment = trimmed_before.ends_with("||=")
            || trimmed_before.ends_with("&&=")
            || (trimmed_before.ends_with('=') && !trimmed_before.ends_with("=="));

        self.register_offense_with_node(node, is_assignment, false);
    }

    /// Check if a node or its descendants contain a non-allowable begin
    fn has_descendant_offensive_begin(&self, node: &Node) -> bool {
        match node {
            Node::BeginNode { .. } => {
                let begin = node.as_begin_node().unwrap();
                if begin.begin_keyword_loc().is_some() && !self.is_allowable_kwbegin(&begin) {
                    return true;
                }
            }
            Node::LocalVariableOrWriteNode { .. }
            | Node::InstanceVariableOrWriteNode { .. }
            | Node::ClassVariableOrWriteNode { .. }
            | Node::GlobalVariableOrWriteNode { .. }
            | Node::ConstantOrWriteNode { .. }
            | Node::LocalVariableAndWriteNode { .. }
            | Node::InstanceVariableAndWriteNode { .. } => {
                // Check the RHS of ||= / &&= for nested begin
                let loc = node.location();
                let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
                // Find "begin" keyword in the source
                if src.contains("begin") {
                    // Use a more precise check - look at child nodes
                    // For now, just check if this wraps a begin
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn is_allowable_kwbegin(&self, node: &ruby_prism::BeginNode) -> bool {
        // Empty begin
        if node.statements().is_none() {
            return true;
        }

        // Contains rescue or ensure
        if node.rescue_clause().is_some() || node.ensure_clause().is_some() {
            return true;
        }

        // Valid context: post-condition loop, send, operator keyword, or valid assignment
        if self.valid_context_using_only_begin(node) {
            return true;
        }

        false
    }

    fn begin_block_has_multiline_statements(&self, node: &ruby_prism::BeginNode) -> bool {
        if let Some(stmts) = node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            body.len() >= 2
        } else {
            false
        }
    }

    fn valid_context_using_only_begin(&self, node: &ruby_prism::BeginNode) -> bool {
        // Check if this begin is used in a post-condition loop, method call, or operator context
        // We approximate by checking what follows/precedes the begin in source

        let begin_start = node.location().start_offset();
        let begin_end = node.location().end_offset();

        // Check if there's a modifier condition after the end keyword
        let after_end = &self.ctx.source[begin_end..];
        let trimmed_after = after_end.trim_start();

        let is_multi_stmt = if let Some(stmts) = node.statements() {
            stmts.body().iter().count() >= 2
        } else {
            false
        };

        // Post-condition loops (while/until) are always valid with begin
        if trimmed_after.starts_with("while ") || trimmed_after.starts_with("until ") {
            return true;
        }
        // Modifier if/unless - only valid if multi-statement
        if trimmed_after.starts_with("unless ") || trimmed_after.starts_with("if ") {
            if is_multi_stmt {
                return true;
            }
            // Single-statement with modifier: NOT valid (should be flagged)
            // But don't return false here - fall through to other checks
        }

        // Check if begin is preceded by `=`, `||=`, `&&=` (assignment context)
        let before = &self.ctx.source[..begin_start];
        let trimmed_before = before.trim_end();
        if trimmed_before.ends_with("||=")
            || trimmed_before.ends_with("&&=")
            || trimmed_before.ends_with('=')
        {
            // Assignment context with multiple statements is valid
            if let Some(stmts) = node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                if body.len() != 1 || body.is_empty() {
                    return true;
                }
            } else {
                return true; // empty begin in assignment = valid
            }

            // Check if followed by .method (begin...end.baz)
            if trimmed_after.starts_with('.') {
                return true;
            }
            // Single statement assignment - NOT valid (should be flagged)
            return false;
        }

        // Check if begin is a method argument: `do_something begin ... end`
        if trimmed_before.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '?' || c == '!') {
            // Check it's not a keyword like `if` or `unless`
            let last_word = get_last_word(trimmed_before);
            if !matches!(
                last_word,
                "if" | "unless" | "while" | "until" | "case" | "begin"
                    | "else" | "elsif" | "when" | "in" | "do" | "def" | "class"
                    | "module" | "return" | "yield" | "raise"
            ) {
                return true;
            }
        }

        // Check for logical operators: `condition && begin ... end`
        if trimmed_before.ends_with("&&")
            || trimmed_before.ends_with("||")
            || trimmed_before.ends_with("and")
            || trimmed_before.ends_with("or")
        {
            return true;
        }

        false
    }
}

/// Extract comments from the text between `begin` keyword and first child.
/// Returns text to insert before the parent statement (for assignment context comment restoration).
fn extract_comments_for_restore(between: &str) -> String {
    let mut result = String::new();
    for line in between.split('\n') {
        // Check if this line (trimmed) is a comment or starts with whitespace + #
        let trimmed = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        if trimmed.starts_with('#') {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

/// Find byte offset of start of line containing `offset`.
fn line_start_offset(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0)
}

fn get_last_word(s: &str) -> &str {
    let bytes = s.as_bytes();
    let end = bytes.len();
    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_' || bytes[start - 1] == b'?' || bytes[start - 1] == b'!') {
        start -= 1;
    }
    &s[start..end]
}

impl Visit<'_> for RedundantBeginVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.check_def(node);
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_branches_if(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_branches_unless(node);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        self.check_case(node);
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        self.check_case_match(node);
        ruby_prism::visit_case_match_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        self.check_while(node);
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.check_until(node);
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.check_block(node);
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        self.check_standalone_begin(node);
        ruby_prism::visit_begin_node(self, node);
    }
}

crate::register_cop!("Style/RedundantBegin", |_cfg| {
    Some(Box::new(RedundantBegin::new()))
});
