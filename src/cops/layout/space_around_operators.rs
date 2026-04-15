//! Layout/SpaceAroundOperators
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_around_operators.rb
//!
//! This is a basic port covering the most common operator node types (assignment,
//! binary and/or, hash pair rockets, ternary, parent class `<`, resbody `=>`, and
//! operator-method send calls like `a+b`). Vertical-alignment detection
//! (AllowForAlignment) is intentionally simple — we only allow extra spaces when
//! the token on the adjacent non-blank line has the same column or the same
//! character at the same column, matching RuboCop's `aligned_token?` check.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct SpaceAroundOperators {
    allow_for_alignment: bool,
    exponent_space: bool,
    slash_space: bool,
    hash_table_style: bool,
}

impl Default for SpaceAroundOperators {
    fn default() -> Self {
        Self {
            allow_for_alignment: true,
            exponent_space: false,
            slash_space: false,
            hash_table_style: false,
        }
    }
}

impl SpaceAroundOperators {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(
        allow_for_alignment: bool,
        exponent_space: bool,
        slash_space: bool,
        hash_table_style: bool,
    ) -> Self {
        Self { allow_for_alignment, exponent_space, slash_space, hash_table_style }
    }
}

impl Cop for SpaceAroundOperators {
    fn name(&self) -> &'static str { "Layout/SpaceAroundOperators" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            allow_for_alignment: self.allow_for_alignment,
            exponent_space: self.exponent_space,
            slash_space: self.slash_space,
            hash_table_style: self.hash_table_style,
            offenses: Vec::new(),
            inside_pair: false,
            parent_hash_multiline: false,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    allow_for_alignment: bool,
    exponent_space: bool,
    slash_space: bool,
    hash_table_style: bool,
    offenses: Vec<Offense>,
    inside_pair: bool,
    parent_hash_multiline: bool,
}

const IRREGULAR_METHODS: &[&str] = &["[]", "!", "[]=", "`"];

impl<'a> Visitor<'a> {
    fn check_operator(&mut self, op_start: usize, op_end: usize, right_start: usize) {
        let src = self.ctx.source;
        let bytes = src.as_bytes();
        if op_start >= bytes.len() || op_end > bytes.len() {
            return;
        }
        // Operator text.
        let op_text = &src[op_start..op_end];

        // Skip if operator is followed by newline (operator at end of line).
        if op_end < bytes.len() && bytes[op_end] == b'\n' {
            return;
        }
        // Skip if prev char is newline (operator at start of line).
        if op_start > 0 && bytes[op_start - 1] == b'\n' {
            return;
        }

        // Surrounding space on the left/right (same-line only).
        let left_ws = count_ws_before(bytes, op_start);
        let right_ws = count_ws_after(bytes, op_end);

        // Comment after operator — allow multiple spaces (handled below by right_ws being
        // followed by '#'): RuboCop check `with_space.last_column == comment.loc.column`.
        let right_is_comment = op_end + right_ws < bytes.len() && bytes[op_end + right_ws] == b'#';

        let should_not_have_space = (op_text == "**" && !self.exponent_space)
            || (op_text == "/" && !self.slash_space && is_rhs_rational(src, right_start));

        if should_not_have_space {
            if left_ws > 0 || right_ws > 0 {
                // "Space around operator `**` detected."
                let msg = format!("Space around operator `{}` detected.", op_text);
                // Correction: replace [op_start - left_ws, op_end + right_ws) with op_text
                let range_start = op_start - left_ws;
                let range_end = op_end + right_ws;
                self.emit(op_start, op_end, &msg, Correction::replace(range_start, range_end, op_text.to_string()));
            }
            return;
        }

        if left_ws == 0 || right_ws == 0 {
            let msg = format!("Surrounding space missing for operator `{}`.", op_text);
            let correction = Correction::replace(
                op_start - left_ws,
                op_end + right_ws,
                format!(" {} ", op_text),
            );
            self.emit(op_start, op_end, &msg, correction);
            return;
        }

        // Excess space (>1). Allow if comment follows and RuboCop would skip.
        let excess_left = left_ws > 1;
        let excess_right = right_ws > 1 && !right_is_comment;

        if excess_left || excess_right {
            // Check alignment.
            if self.allow_for_alignment && self.is_aligned(op_start, op_end, excess_left, excess_right) {
                return;
            }
            let msg = format!("Operator `{}` should be surrounded by a single space.", op_text);
            let correction = Correction::replace(
                op_start - left_ws,
                op_end + right_ws,
                format!(" {} ", op_text),
            );
            self.emit(op_start, op_end, &msg, correction);
        }
    }

    fn emit(&mut self, op_start: usize, op_end: usize, msg: &str, correction: Correction) {
        let loc = Location::from_offsets(self.ctx.source, op_start, op_end);
        self.offenses.push(
            Offense::new(
                "Layout/SpaceAroundOperators",
                msg,
                Severity::Convention,
                loc,
                self.ctx.filename,
            )
            .with_correction(correction),
        );
    }

    /// Simple alignment check: look at line above and below. If a non-whitespace
    /// char on that adjacent line lands on the same column as `op_start`, accept.
    /// Matches RuboCop's `aligned_token?` for common vertical-alignment patterns.
    fn is_aligned(&self, op_start: usize, op_end: usize, excess_left: bool, excess_right: bool) -> bool {
        let src = self.ctx.source;
        let bytes = src.as_bytes();
        // Column of operator start.
        let line_start = line_start_of(bytes, op_start);
        let op_col = op_start - line_start;
        // Look at preceding line.
        if let Some(prev_line) = prev_line_range(bytes, line_start) {
            if aligned_on_line(src, prev_line, op_col, op_end - op_start) {
                return true;
            }
        }
        // Look at following line.
        if let Some(next_line) = next_line_range(bytes, op_start) {
            if aligned_on_line(src, next_line, op_col, op_end - op_start) {
                return true;
            }
        }
        // If only excess_right (e.g. `name  = value   # comment`), allow — comments handled.
        let _ = (excess_left, excess_right);
        false
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Operator-method send: `a + b`, `a == b`, etc.
        // Skip unary operations (no LHS), dot/safe-nav, and double-colon calls.
        if node.call_operator_loc().is_none() && node.receiver().is_some() {
            if let Some(sel) = node.message_loc() {
                let name_bytes = self.ctx.source.as_bytes();
                let name = &self.ctx.source[sel.start_offset()..sel.end_offset()];
                // Heuristic: operator methods consist only of non-alnum chars.
                let is_op = !name.is_empty()
                    && name.chars().all(|c| !c.is_alphanumeric() && c != '_')
                    && !IRREGULAR_METHODS.contains(&name);
                // Ensure args on the right (actual binary op syntax, not method-style `.+(x)`).
                let no_dot = !is_dot_call(self.ctx.source, node);
                if is_op && no_dot {
                    if let Some(args) = node.arguments() {
                        if let Some(first) = args.arguments().iter().next() {
                            let right_start = first.location().start_offset();
                            // Heuristic: reject if prev non-ws is `(` i.e. def-style `.+(x)`.
                            let receiver_loc = node.receiver().unwrap().location();
                            let recv_end = receiver_loc.end_offset();
                            if recv_end <= sel.start_offset() {
                                // Skip if selector immediately after `.` or `::`.
                                let op_start = sel.start_offset();
                                // Skip if the byte before op_start is not a space AND not directly a receiver-end (avoid unary minus etc).
                                let prev_non_ws = prev_non_ws_byte(name_bytes, op_start);
                                if prev_non_ws.is_some() {
                                    self.check_operator(sel.start_offset(), sel.end_offset(), right_start);
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let op = node.operator_loc();
        let rhs_start = node.right().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let op = node.operator_loc();
        let rhs_start = node.right().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_or_node(self, node);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_instance_variable_operator_write_node(&mut self, node: &ruby_prism::InstanceVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_instance_variable_operator_write_node(self, node);
    }
    fn visit_class_variable_operator_write_node(&mut self, node: &ruby_prism::ClassVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_class_variable_operator_write_node(self, node);
    }
    fn visit_global_variable_operator_write_node(&mut self, node: &ruby_prism::GlobalVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_global_variable_operator_write_node(self, node);
    }
    fn visit_constant_operator_write_node(&mut self, node: &ruby_prism::ConstantOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_constant_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }
    fn visit_instance_variable_and_write_node(&mut self, node: &ruby_prism::InstanceVariableAndWriteNode) {
        let op = node.operator_loc();
        let rhs_start = node.value().location().start_offset();
        self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
        ruby_prism::visit_instance_variable_and_write_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        let multiline = !self.ctx.same_line(node.location().start_offset(), node.location().end_offset());
        let prev = self.parent_hash_multiline;
        self.parent_hash_multiline = multiline;
        ruby_prism::visit_hash_node(self, node);
        self.parent_hash_multiline = prev;
    }

    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
        // Only hash-rocket rockets (=>); skip symbol-style (key:) which has no operator_loc.
        if let Some(op) = node.operator_loc() {
            let op_src = &self.ctx.source[op.start_offset()..op.end_offset()];
            if op_src == "=>" {
                // RuboCop: if hash_table_style && !pairs_on_same_line → skip.
                let skip = self.hash_table_style && self.parent_hash_multiline;
                if !skip {
                    let rhs_start = node.value().location().start_offset();
                    self.check_operator(op.start_offset(), op.end_offset(), rhs_start);
                }
            }
        }
        ruby_prism::visit_assoc_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Ternary: `cond ? a : b` has question/colon locs when it's ternary.
        // Prism exposes this by checking for a `?` then `:` in source between predicate and then.
        // For simplicity, detect if then_keyword_loc source == "?" (Prism encodes ternary this way).
        if let Some(q) = node.then_keyword_loc() {
            let q_src = &self.ctx.source[q.start_offset()..q.end_offset()];
            if q_src == "?" {
                if let Some(stmts) = node.statements() {
                    if let Some(first) = stmts.body().iter().next() {
                        self.check_operator(q.start_offset(), q.end_offset(), first.location().start_offset());
                    }
                }
                // Find the `:` between then branch end and else branch start.
                if let Some(subseq) = node.subsequent() {
                    if let Some(else_node) = subseq.as_else_node() {
                        if let Some(stmts) = else_node.statements() {
                            if let Some(first) = stmts.body().iter().next() {
                                let else_loc = else_node.else_keyword_loc();
                                let colon_start = else_loc.start_offset();
                                let colon_end = else_loc.end_offset();
                                let colon_src = &self.ctx.source[colon_start..colon_end];
                                if colon_src == ":" {
                                    self.check_operator(colon_start, colon_end, first.location().start_offset());
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        // `rescue Foo => e` — operator_loc is `=>`
        if let Some(op) = node.operator_loc() {
            if let Some(ref_node) = node.reference() {
                let rhs = ref_node.location().start_offset();
                self.check_operator(op.start_offset(), op.end_offset(), rhs);
            }
        }
        ruby_prism::visit_rescue_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        // Operator `<` between class name and parent class.
        if let Some(inherit_op) = node.inheritance_operator_loc() {
            if let Some(parent) = node.superclass() {
                let rhs = parent.location().start_offset();
                self.check_operator(inherit_op.start_offset(), inherit_op.end_offset(), rhs);
            }
        }
        ruby_prism::visit_class_node(self, node);
    }
}

// ── Helpers ──

fn count_ws_before(bytes: &[u8], pos: usize) -> usize {
    let mut n = 0;
    while pos > n && (bytes[pos - n - 1] == b' ' || bytes[pos - n - 1] == b'\t') {
        n += 1;
    }
    n
}

fn count_ws_after(bytes: &[u8], pos: usize) -> usize {
    let mut n = 0;
    while pos + n < bytes.len() && (bytes[pos + n] == b' ' || bytes[pos + n] == b'\t') {
        n += 1;
    }
    n
}

fn line_start_of(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i > 0 && bytes[i - 1] != b'\n' { i -= 1; }
    i
}

fn prev_line_range(bytes: &[u8], line_start: usize) -> Option<(usize, usize)> {
    if line_start == 0 { return None; }
    // line_start-1 is '\n'. Walk back to find previous line start.
    let end = line_start - 1; // exclusive of '\n'
    let mut st = end;
    while st > 0 && bytes[st - 1] != b'\n' { st -= 1; }
    Some((st, end))
}

fn next_line_range(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    // Find the '\n' at/after pos.
    let mut i = pos;
    while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
    if i >= bytes.len() { return None; }
    let st = i + 1;
    if st >= bytes.len() { return None; }
    let mut en = st;
    while en < bytes.len() && bytes[en] != b'\n' { en += 1; }
    Some((st, en))
}

fn aligned_on_line(source: &str, line_range: (usize, usize), col: usize, op_len: usize) -> bool {
    let (st, en) = line_range;
    if col >= en - st { return false; }
    let bytes = source.as_bytes();
    let pos = st + col;
    if pos >= en { return false; }
    // aligned_words?: check if source[pos-1..pos+1] contains "\S" followed by space,
    // or char at pos matches operator start.
    if pos + op_len <= en {
        let ch = bytes[pos];
        // Match if the line has a non-ws char here that matches identically.
        if ch != b' ' && ch != b'\t' {
            return true;
        }
    }
    false
}

fn is_rhs_rational(source: &str, right_start: usize) -> bool {
    // Rational literal: digits followed by 'r', optionally with underscores.
    let bytes = source.as_bytes();
    let mut i = right_start;
    let mut saw_digit = false;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        if bytes[i].is_ascii_digit() { saw_digit = true; }
        i += 1;
    }
    saw_digit && i < bytes.len() && bytes[i] == b'r'
}

fn is_dot_call(source: &str, node: &ruby_prism::CallNode) -> bool {
    // Method-style operator call: `a.+(b)`. Detect a `.` just before selector.
    let bytes = source.as_bytes();
    if let Some(sel) = node.message_loc() {
        let mut i = sel.start_offset();
        while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') { i -= 1; }
        if i > 0 && bytes[i - 1] == b'.' { return true; }
    }
    false
}

fn prev_non_ws_byte(bytes: &[u8], pos: usize) -> Option<u8> {
    let mut i = pos;
    while i > 0 {
        i -= 1;
        let b = bytes[i];
        if b != b' ' && b != b'\t' { return Some(b); }
    }
    None
}
