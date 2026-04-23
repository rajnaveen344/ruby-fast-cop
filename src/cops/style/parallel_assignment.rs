//! Style/ParallelAssignment
//!
//! Checks for usages of parallel assignment (`a, b, c = 1, 2, 3`) and offers
//! corrections to expand them to individual assignments. Ported from RuboCop's
//! `lib/rubocop/cop/style/parallel_assignment.rb`.
//!
//! Algorithm mirrors RuboCop:
//!   * Allowed when LHS has only one element or any splat target.
//!   * Allowed when RHS is not an array (e.g. `a, b = foo`).
//!   * Allowed when any RHS element is a splat.
//!   * Allowed when LHS and RHS counts disagree.
//!   * Allowed when assignments form a cyclic dependency (`a, b = b, a`).
//!
//! Corrections:
//!   * Generic   – just unroll into one assignment per line, indented to match
//!                 the original `MultiWriteNode`.
//!   * Modifier  – wrap the unrolled assignments in `if/unless/while/until`.
//!   * Rescue    – wrap the unrolled assignments in `begin .. rescue .. end`,
//!                 *unless* the rescue-modified assignment is the only thing
//!                 inside a method body, in which case use the implicit `def`
//!                 rescue.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Do not use parallel assignment.";

#[derive(Default)]
pub struct ParallelAssignment {
    indentation_width: Option<usize>,
}

impl ParallelAssignment {
    pub fn new() -> Self {
        Self { indentation_width: None }
    }

    pub fn with_indentation_width(width: usize) -> Self {
        Self { indentation_width: Some(width) }
    }
}

impl Cop for ParallelAssignment {
    fn name(&self) -> &'static str {
        "Style/ParallelAssignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let indent_width = self.indentation_width.unwrap_or(2);
        let mut visitor = ParallelAssignmentVisitor {
            ctx,
            indent_width,
            offenses: Vec::new(),
            parents: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

// ── Parent-tracking visitor ──

struct ParallelAssignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    indent_width: usize,
    offenses: Vec<Offense>,
    /// Stack of node "kinds" (start_offset, end_offset, kind discriminator) for
    /// the AST chain leading to the current node. We only need to recognise
    /// `if/unless/while/until` (modifier vs full form) and `def`.
    parents: Vec<ParentInfo>,
}

#[derive(Clone)]
struct ParentInfo {
    kind: ParentKind,
    /// Byte offsets of the parent node.
    start: usize,
    end: usize,
    /// For modifier-form `IfNode/UnlessNode/WhileNode/UntilNode`: byte offset
    /// of the keyword (`if`, `unless`, ...) – we need it to build the
    /// corrected modifier source.
    keyword_start: Option<usize>,
    keyword_text: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq)]
enum ParentKind {
    IfModifier,
    UnlessModifier,
    WhileModifier,
    UntilModifier,
    /// Non-modifier conditional/loop (full form, has `end` keyword).
    NonModifier,
    Def,
    Statements,
    Begin,
    Other,
    IgnoredMultiWrite,
}

impl<'a> Visit<'_> for ParallelAssignmentVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let is_modifier = node.end_keyword_loc().is_none();
        let info = ParentInfo {
            kind: if is_modifier { ParentKind::IfModifier } else { ParentKind::NonModifier },
            start: node.location().start_offset(),
            end: node.location().end_offset(),
            keyword_start: node.if_keyword_loc().map(|l| l.start_offset()),
            keyword_text: Some("if"),
        };
        self.parents.push(info);
        ruby_prism::visit_if_node(self, node);
        self.parents.pop();
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let is_modifier = node.end_keyword_loc().is_none();
        let info = ParentInfo {
            kind: if is_modifier { ParentKind::UnlessModifier } else { ParentKind::NonModifier },
            start: node.location().start_offset(),
            end: node.location().end_offset(),
            keyword_start: Some(node.keyword_loc().start_offset()),
            keyword_text: Some("unless"),
        };
        self.parents.push(info);
        ruby_prism::visit_unless_node(self, node);
        self.parents.pop();
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        // `while x do ... end` vs `body while x` — modifier when closing_loc is empty.
        let kw = node.keyword_loc();
        let body_start = node
            .statements()
            .map(|s| s.location().start_offset())
            .unwrap_or(usize::MAX);
        let is_modifier = kw.start_offset() > body_start;
        let info = ParentInfo {
            kind: if is_modifier { ParentKind::WhileModifier } else { ParentKind::NonModifier },
            start: node.location().start_offset(),
            end: node.location().end_offset(),
            keyword_start: Some(kw.start_offset()),
            keyword_text: Some("while"),
        };
        self.parents.push(info);
        ruby_prism::visit_while_node(self, node);
        self.parents.pop();
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let kw = node.keyword_loc();
        let body_start = node
            .statements()
            .map(|s| s.location().start_offset())
            .unwrap_or(usize::MAX);
        let is_modifier = kw.start_offset() > body_start;
        let info = ParentInfo {
            kind: if is_modifier { ParentKind::UntilModifier } else { ParentKind::NonModifier },
            start: node.location().start_offset(),
            end: node.location().end_offset(),
            keyword_start: Some(kw.start_offset()),
            keyword_text: Some("until"),
        };
        self.parents.push(info);
        ruby_prism::visit_until_node(self, node);
        self.parents.pop();
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let info = ParentInfo {
            kind: ParentKind::Def,
            start: node.location().start_offset(),
            end: node.location().end_offset(),
            keyword_start: None,
            keyword_text: None,
        };
        self.parents.push(info);
        ruby_prism::visit_def_node(self, node);
        self.parents.pop();
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode) {
        let info = ParentInfo {
            kind: ParentKind::Statements,
            start: node.location().start_offset(),
            end: node.location().end_offset(),
            keyword_start: None,
            keyword_text: None,
        };
        self.parents.push(info);
        ruby_prism::visit_statements_node(self, node);
        self.parents.pop();
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        let info = ParentInfo {
            kind: ParentKind::Begin,
            start: node.location().start_offset(),
            end: node.location().end_offset(),
            keyword_start: None,
            keyword_text: None,
        };
        self.parents.push(info);
        ruby_prism::visit_begin_node(self, node);
        self.parents.pop();
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        // Mirror RuboCop's `part_of_ignored_node?`: once an outer
        // `MultiWriteNode` has been flagged, nested `MultiWriteNode`s inside
        // its source range are ignored to avoid double-reporting.
        let my_start = node.location().start_offset();
        let my_end = node.location().end_offset();
        let inside_flagged = self
            .parents
            .iter()
            .any(|p| p.kind == ParentKind::IgnoredMultiWrite
                && p.start <= my_start
                && p.end >= my_end);
        let mut flagged = false;
        if !inside_flagged {
            if let Some(off) = self.check(node) {
                self.offenses.push(off);
                flagged = true;
            }
        }
        let info = ParentInfo {
            kind: if flagged { ParentKind::IgnoredMultiWrite } else { ParentKind::Other },
            start: my_start,
            end: my_end,
            keyword_start: None,
            keyword_text: None,
        };
        self.parents.push(info);
        ruby_prism::visit_multi_write_node(self, node);
        self.parents.pop();
    }
}

impl<'a> ParallelAssignmentVisitor<'a> {
    fn check(&self, node: &ruby_prism::MultiWriteNode) -> Option<Offense> {
        // Collect lhs targets. A `rest` target is allowed only when it's an
        // `ImplicitRestNode` (trailing comma like `a, b, c, = ...`). A real
        // `SplatNode` rest (`a, *b = ...`) means the assignment is disallowed
        // from being flagged.
        if let Some(rest) = node.rest() {
            if rest.as_implicit_rest_node().is_none() {
                return None;
            }
        }
        let lefts: Vec<Node<'_>> = node.lefts().iter().collect();
        let rights: Vec<Node<'_>> = node.rights().iter().collect();
        if !rights.is_empty() {
            // After-splat targets are present — splat must exist; already handled.
            return None;
        }
        if lefts.len() <= 1 {
            return None; // `a, = ...`
        }
        // Detect rescue modifier on the value.
        let value = node.value();
        let (rhs_node, has_rescue_mod) =
            if let Some(rm) = value.as_rescue_modifier_node() {
                (rm.expression(), true)
            } else {
                (value, false)
            };

        // RHS must be an array literal.
        let rhs_array = rhs_node.as_array_node()?;
        let rhs_elements: Vec<Node<'_>> = rhs_array.elements().iter().collect();

        if rhs_elements.is_empty() {
            return None;
        }
        if rhs_elements.iter().any(|e| e.as_splat_node().is_some()) {
            return None;
        }
        if lefts.len() != rhs_elements.len() {
            return None;
        }

        // Cycle check: build dependency graph for topological sort. If sort
        // fails (cycle), the assignment is allowed.
        let order = find_valid_order(self.ctx, &lefts, &rhs_elements)?;

        // ── Build offense ──
        let mwrite_start = node.location().start_offset();
        let rhs_end = rhs_array.location().end_offset();
        let off = self
            .ctx
            .offense_with_range(
                "Style/ParallelAssignment",
                MSG,
                Severity::Convention,
                mwrite_start,
                rhs_end,
            );

        // ── Build correction ──
        let correction = self.build_correction(node, &rhs_array, &order, has_rescue_mod);
        Some(off.with_correction(correction))
    }

    fn build_correction(
        &self,
        node: &ruby_prism::MultiWriteNode,
        _rhs_array: &ruby_prism::ArrayNode,
        order: &[(Node<'_>, Node<'_>)],
        has_rescue_mod: bool,
    ) -> Correction {
        let src = self.ctx.source;
        let mwrite_start = node.location().start_offset();
        let mwrite_end = node.location().end_offset();

        // Indentation derived from the column where the `MultiWriteNode` begins.
        let node_col = self.ctx.col_of(mwrite_start);
        let offset_str = " ".repeat(node_col);
        let inner_indent = " ".repeat(node_col + self.indent_width);

        let assignments: Vec<String> = order
            .iter()
            .map(|(lhs, rhs)| format!("{} = {}", node_source(src, lhs), rhs_source(src, rhs)))
            .collect();

        // ── Modifier form (if/unless/while/until) ──
        if !has_rescue_mod {
            if let Some(parent) = self.modifier_parent(mwrite_start, mwrite_end) {
                // Replace the *parent* (the modifier conditional) with a full form.
                let kw_start = parent.keyword_start.unwrap();
                let kw_text = parent.keyword_text.unwrap();
                // Modifier head: `if foo`, `unless foo`, ...
                let head = &src[kw_start..parent.end];
                let mut out = String::new();
                out.push_str(head);
                out.push('\n');
                for a in &assignments {
                    out.push_str(&inner_indent);
                    out.push_str(a);
                    out.push('\n');
                }
                out.push_str(&offset_str);
                out.push_str("end");
                // We are *replacing* the entire parent conditional. The
                // conditional starts at `parent.start` (which equals
                // `mwrite_start` for top-level modifier) and ends at
                // `parent.end`.
                let _ = kw_text; // suppress unused warning
                return Correction::replace(parent.start, parent.end, out);
            }
        }

        // ── Rescue modifier ──
        if has_rescue_mod {
            let rm = node.value().as_rescue_modifier_node().unwrap();
            let rescue_result = rm.rescue_expression();
            let rescue_src = node_source(src, &rescue_result);
            // Determine whether the immediately enclosing scope (parent of the
            // mwrite that *isn't* a Statements wrapper) is a `def` whose body
            // contains only this mwrite — in which case use the implicit `def`
            // rescue.
            if self.is_only_stmt_in_def(mwrite_start, mwrite_end) {
                let mut out = String::new();
                // Generic body (no leading indent — caller's prefix kept).
                out.push_str(&assignments.join(&format!("\n{}", offset_str)));
                out.push_str("\nrescue\n");
                out.push_str(&offset_str);
                out.push_str(&rescue_src);
                return Correction::replace(mwrite_start, mwrite_end, out);
            }
            // begin/rescue/end wrapper.
            let mut out = String::new();
            out.push_str("begin\n");
            out.push_str(&inner_indent);
            out.push_str(&assignments.join(&format!("\n{}", inner_indent)));
            out.push_str("\n");
            out.push_str(&offset_str);
            out.push_str("rescue\n");
            out.push_str(&inner_indent);
            out.push_str(&rescue_src);
            out.push_str("\n");
            out.push_str(&offset_str);
            out.push_str("end");
            return Correction::replace(mwrite_start, mwrite_end, out);
        }

        // ── Generic ──
        let body = assignments.join(&format!("\n{}", offset_str));
        Correction::replace(mwrite_start, mwrite_end, body)
    }

    /// If the immediate enclosing parent of the mwrite is a modifier
    /// `if/unless/while/until`, return its info.
    fn modifier_parent(&self, mw_start: usize, mw_end: usize) -> Option<&ParentInfo> {
        // Walk parents from the innermost outward, stopping at the first
        // non-(Statements) frame. That is the AST parent of the mwrite.
        for p in self.parents.iter().rev() {
            // Skip the mwrite's own frame (won't be in stack since we push
            // before recursing into self).
            if p.start <= mw_start && p.end >= mw_end {
                match p.kind {
                    ParentKind::IfModifier
                    | ParentKind::UnlessModifier
                    | ParentKind::WhileModifier
                    | ParentKind::UntilModifier => return Some(p),
                    ParentKind::Statements | ParentKind::Begin | ParentKind::Other => {
                        // Continue searching outward — these may wrap the
                        // mwrite within a modifier.
                        continue;
                    }
                    _ => return None,
                }
            }
        }
        None
    }

    /// Detect: mwrite is inside a `def` whose body is just this mwrite (or a
    /// Statements wrapping just this mwrite).
    fn is_only_stmt_in_def(&self, mw_start: usize, mw_end: usize) -> bool {
        // Walk outward; first non-Statements parent must be a Def, and the
        // enclosing Statements (if any) must contain only the mwrite range.
        let mut stmt_only = true;
        for p in self.parents.iter().rev() {
            if p.start <= mw_start && p.end >= mw_end {
                match p.kind {
                    ParentKind::Statements => {
                        // Statements range must equal the mwrite range
                        // (modulo whitespace) — if equal byte range, it's a
                        // single-stmt body.
                        if p.start != mw_start || p.end != mw_end {
                            stmt_only = false;
                        }
                    }
                    ParentKind::Def => return stmt_only,
                    _ => return false,
                }
            }
        }
        false
    }
}

// ── Helpers ──

fn node_source<'s>(src: &'s str, n: &Node<'_>) -> &'s str {
    let loc = n.location();
    &src[loc.start_offset()..loc.end_offset()]
}

/// Mirrors RuboCop's `source(node, loc)` from GenericCorrector — handles a few
/// Ruby literal edge cases when expanding from a percent literal:
///   * Word array element (`StringNode` with no `opening_loc`) → `'src'`
///   * Symbol array element (`SymbolNode` with no opening) → `:src`
///   * `__FILE__` etc. — written as-is.
fn rhs_source<'s>(src: &'s str, n: &Node<'_>) -> std::borrow::Cow<'s, str> {
    if let Some(s) = n.as_string_node() {
        if s.opening_loc().is_none() {
            // bare word-array element
            let raw = node_source(src, n);
            return std::borrow::Cow::Owned(format!("'{}'", raw));
        }
    }
    if let Some(s) = n.as_symbol_node() {
        if s.opening_loc().is_none() {
            let raw = node_source(src, n);
            return std::borrow::Cow::Owned(format!(":{}", raw));
        }
    }
    std::borrow::Cow::Borrowed(node_source(src, n))
}

// ── Topological sort over parallel assignments ──

fn find_valid_order<'n>(
    ctx: &CheckContext,
    lefts: &[Node<'n>],
    rights: &[Node<'n>],
) -> Option<Vec<(Node<'n>, Node<'n>)>> {
    let n = lefts.len();
    // Adjacency: deps[i] = set of indices j such that assignment i depends on
    // assignment j (j must be emitted before i).
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            if dependency(ctx, &lefts[i], &rights[j]) {
                deps[i].push(j);
            }
        }
    }

    let mut emitted = vec![false; n];
    let mut result: Vec<(Node<'n>, Node<'n>)> = Vec::with_capacity(n);
    while result.len() < n {
        // Find first un-emitted assignment whose deps are fully satisfied.
        let next = (0..n).find(|&i| !emitted[i] && deps[i].iter().all(|&j| emitted[j]));
        let next = next?; // cycle → no valid order
        emitted[next] = true;
        result.push((clone_node(&lefts[next]), clone_node(&rights[next])));
    }
    Some(result)
}

fn clone_node<'n>(n: &Node<'n>) -> Node<'n> {
    // Node is Copy in practice (just pointers + lifetime markers) — but the
    // type itself doesn't impl Copy, so we re-construct via as_node helpers.
    // The simplest safe path: call `as_node()` on the matched variant.
    // We sidestep this by using the `Node::new`-style reconstruction via the
    // location-preserving `as_*` helpers. Easier: just convert back through
    // the same enum variant via the matched type.
    // In practice every Prism node type provides `as_node`. We rely on
    // discrimination by the `as_*` matching cascade.
    if let Some(x) = n.as_local_variable_target_node() { return x.as_node(); }
    if let Some(x) = n.as_constant_target_node() { return x.as_node(); }
    if let Some(x) = n.as_constant_path_target_node() { return x.as_node(); }
    if let Some(x) = n.as_call_target_node() { return x.as_node(); }
    if let Some(x) = n.as_index_target_node() { return x.as_node(); }
    if let Some(x) = n.as_local_variable_read_node() { return x.as_node(); }
    if let Some(x) = n.as_constant_read_node() { return x.as_node(); }
    if let Some(x) = n.as_constant_path_node() { return x.as_node(); }
    if let Some(x) = n.as_call_node() { return x.as_node(); }
    if let Some(x) = n.as_integer_node() { return x.as_node(); }
    if let Some(x) = n.as_float_node() { return x.as_node(); }
    if let Some(x) = n.as_string_node() { return x.as_node(); }
    if let Some(x) = n.as_symbol_node() { return x.as_node(); }
    if let Some(x) = n.as_array_node() { return x.as_node(); }
    if let Some(x) = n.as_hash_node() { return x.as_node(); }
    if let Some(x) = n.as_self_node() { return x.as_node(); }
    if let Some(x) = n.as_lambda_node() { return x.as_node(); }
    if let Some(x) = n.as_x_string_node() { return x.as_node(); }
    if let Some(x) = n.as_source_file_node() { return x.as_node(); }
    if let Some(x) = n.as_parentheses_node() { return x.as_node(); }
    if let Some(x) = n.as_true_node() { return x.as_node(); }
    if let Some(x) = n.as_false_node() { return x.as_node(); }
    if let Some(x) = n.as_nil_node() { return x.as_node(); }
    if let Some(x) = n.as_block_node() { return x.as_node(); }
    if let Some(x) = n.as_interpolated_string_node() { return x.as_node(); }
    // Fallback: unsafe re-cast through pointer copy via Debug ‒ we just
    // re-fetch by looking up the offset in the source. This path should be
    // unreachable for the node types we encounter inside a `MultiWriteNode`
    // lhs/rhs. Panic loudly so tests catch it.
    panic!("ParallelAssignment::clone_node: unsupported node variant");
}

/// Does `lhs`-target's variable get read inside `rhs`?
fn dependency(ctx: &CheckContext, lhs: &Node<'_>, rhs: &Node<'_>) -> bool {
    if let Some(name) = target_var_name(lhs) {
        if reads_var(ctx.source, rhs, &name) {
            return true;
        }
    }
    // Attribute / index assignment: lhs is a CallTargetNode (`obj.foo=`) or
    // IndexTargetNode (`a[0]=`). We treat any read of the same getter on the
    // same receiver as a dependency.
    if let Some(ct) = lhs.as_call_target_node() {
        let recv = ct.receiver();
        let mname = ctx.src(ct.message_loc().start_offset(), ct.message_loc().end_offset());
        // Strip trailing `=` to get the getter name.
        let getter = mname.trim_end_matches('=');
        if reads_call(ctx.source, rhs, &recv, getter, None) {
            return true;
        }
        // RuboCop's `add_self_to_getters` — treat a bare `foo` (zero-arg call
        // with no explicit receiver) as `self.foo` when the lhs is `self.foo=`.
        if recv.as_self_node().is_some() && reads_bare_call(ctx.source, rhs, getter) {
            return true;
        }
    }
    if let Some(it) = lhs.as_index_target_node() {
        let recv = it.receiver();
        // Compare argument source against any `recv[args]` call inside rhs.
        let args_src = if let Some(args) = it.arguments() {
            let l = args.location();
            Some(&ctx.source[l.start_offset()..l.end_offset()])
        } else {
            None
        };
        if reads_call(ctx.source, rhs, &recv, "[]", args_src) {
            return true;
        }
    }
    false
}

/// Extract the variable name for a target node, if it's a simple variable target.
fn target_var_name(n: &Node<'_>) -> Option<String> {
    if let Some(t) = n.as_local_variable_target_node() {
        return Some(crate::node_name!(t).into_owned());
    }
    if let Some(t) = n.as_constant_target_node() {
        return Some(crate::node_name!(t).into_owned());
    }
    if let Some(t) = n.as_instance_variable_target_node() {
        return Some(crate::node_name!(t).into_owned());
    }
    if let Some(t) = n.as_class_variable_target_node() {
        return Some(crate::node_name!(t).into_owned());
    }
    if let Some(t) = n.as_global_variable_target_node() {
        return Some(crate::node_name!(t).into_owned());
    }
    None
}

/// Does `node` (or any descendant) read a variable named `name`?
fn reads_var(_src: &str, node: &Node<'_>, name: &str) -> bool {
    let mut v = ReadFinder { name, found: false };
    v.visit(node);
    v.found
}

struct ReadFinder<'a> {
    name: &'a str,
    found: bool,
}

impl<'a> Visit<'_> for ReadFinder<'a> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let n = crate::node_name!(node);
        if n == self.name {
            self.found = true;
        }
    }
    fn visit_instance_variable_read_node(&mut self, node: &ruby_prism::InstanceVariableReadNode) {
        let n = crate::node_name!(node);
        if n == self.name {
            self.found = true;
        }
    }
    fn visit_class_variable_read_node(&mut self, node: &ruby_prism::ClassVariableReadNode) {
        let n = crate::node_name!(node);
        if n == self.name {
            self.found = true;
        }
    }
    fn visit_global_variable_read_node(&mut self, node: &ruby_prism::GlobalVariableReadNode) {
        let n = crate::node_name!(node);
        if n == self.name {
            self.found = true;
        }
    }
    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode) {
        let n = crate::node_name!(node);
        if n == self.name {
            self.found = true;
        }
    }
    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode) {
        // Only the *root* of the path matters: `Foo::Bar` reads constant `Foo`.
        let _ = node;
        // Continue traversal — the leftmost ConstantReadNode descendant will
        // be picked up by visit_constant_read_node above.
        ruby_prism::visit_constant_path_node(self, node);
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Variable-call zero-arg method invocations may appear as CallNodes.
        if node.receiver().is_none() && node.arguments().is_none() && node.block().is_none() {
            let m = crate::node_name!(node);
            if m == self.name {
                self.found = true;
            }
        }
        if !self.found {
            ruby_prism::visit_call_node(self, node);
        }
    }

    // Stop traversing into nested `MultiWriteNode` rhs — those have their own
    // scope considerations.
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        let _ = self;
        let _ = node;
        // Do not recurse — matches `def_node_search` behaviour which would
        // also descend, but for our purposes deferring is safer to avoid
        // false positives across a lambda boundary.
        ruby_prism::visit_multi_write_node(self, node);
    }
}

/// Does `node` (or any descendant) call `getter` on `recv`?  Receivers are
/// matched by source slice equality (works for `obj`, `obj.foo`, `Foo::Bar`,
/// etc.).
fn reads_call(
    src: &str,
    node: &Node<'_>,
    recv: &Node<'_>,
    method: &str,
    args_src: Option<&str>,
) -> bool {
    let recv_loc = recv.location();
    let recv_src = &src[recv_loc.start_offset()..recv_loc.end_offset()];
    let mut v = CallFinder {
        src,
        recv_src,
        method,
        args_src,
        found: false,
    };
    v.visit(node);
    v.found
}

/// Does `node` (or any descendant) contain a bare zero-arg call to `method`
/// (implicit self receiver)? Used to treat `b` as `self.b` when the lhs is
/// an attribute writer on `self`.
fn reads_bare_call(src: &str, node: &Node<'_>, method: &str) -> bool {
    let mut v = BareCallFinder { src, method, found: false };
    v.visit(node);
    v.found
}

struct BareCallFinder<'a> {
    src: &'a str,
    method: &'a str,
    found: bool,
}

impl<'a> Visit<'_> for BareCallFinder<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let _ = self.src;
        if node.receiver().is_none() && node.arguments().is_none() && node.block().is_none() {
            let m = crate::node_name!(node);
            if m == self.method {
                self.found = true;
            }
        }
        if !self.found {
            ruby_prism::visit_call_node(self, node);
        }
    }
}

struct CallFinder<'a> {
    src: &'a str,
    recv_src: &'a str,
    method: &'a str,
    args_src: Option<&'a str>,
    found: bool,
}

impl<'a> Visit<'_> for CallFinder<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(recv) = node.receiver() {
            let l = recv.location();
            let rs = &self.src[l.start_offset()..l.end_offset()];
            let mname = crate::node_name!(node);
            if rs == self.recv_src && mname == self.method {
                if let Some(want_args) = self.args_src {
                    if let Some(args) = node.arguments() {
                        let al = args.location();
                        let got = &self.src[al.start_offset()..al.end_offset()];
                        if got == want_args {
                            self.found = true;
                        }
                    }
                } else {
                    self.found = true;
                }
            }
        }
        if !self.found {
            ruby_prism::visit_call_node(self, node);
        }
    }
}

crate::register_cop!("Style/ParallelAssignment", |cfg| {
    let width = if cfg.is_cop_enabled("Layout/IndentationWidth") {
        cfg.get_cop_config("Layout/IndentationWidth")
            .and_then(|c| c.raw.get("Width"))
            .and_then(|v| v.as_i64())
            .map(|w| w as usize)
    } else {
        None
    };
    Some(Box::new(match width {
        Some(w) => ParallelAssignment::with_indentation_width(w),
        None => ParallelAssignment::new(),
    }))
});
