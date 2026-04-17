//! Layout/SpaceAroundOperators
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/space_around_operators.rb
//! Alignment logic mirrors the `PrecedingFollowingAlignment` mixin.
//!
//! Two-pass strategy:
//!   Pass 1 (Collector) walks the AST, recording every operator into an
//!   `OperatorIndex` keyed by line. Pass 2 iterates the index and performs the
//!   spacing + alignment check for each record. This mirrors RuboCop's
//!   `processed_source.tokens` table — alignment queries need the whole token
//!   set available before checking any single operator.
//!
//! Alignment queries translate RuboCop's predicates faithfully:
//!   `aligned_with_preceding_equals_operator` / `aligned_with_subsequent_equals_operator`
//!   for plain `=` assignments; `aligned_with_operator?` for binary/op-assign/pair;
//!   `aligned_with_something?` (on the RHS range) for excess-trailing.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;
use std::collections::HashMap;

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
        // Pass 1: collect all operators into an index.
        let mut index = OperatorIndex::new(ctx.source);
        let mut collector = Collector {
            ctx,
            index: &mut index,
            hash_table_style: self.hash_table_style,
            parent_hash_multiline_stack: Vec::new(),
        };
        collector.visit_program_node(node);
        index.finalize();

        // Pass 2: run check for each recorded op.
        let mut offenses = Vec::new();
        for i in 0..index.ops.len() {
            let op = &index.ops[i];
            if let Some(offense) = self.check_one(op, &index, ctx) {
                offenses.push(offense);
            }
        }
        // Sort by (line, col) for stable output.
        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

// ────────────────────────────────────────────────────────────────────────────
//  OperatorIndex — collected during pass 1, queried during pass 2.
// ────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpKind {
    Assignment,  // =
    OpAssign,    // +=, ||=, &&=, -=, *=, etc.
    Send,        // binary operator method call: +, -, ==, !=, <=, >=, *, /, <<, %, ^, &, |, <=>
    Class,       // class Foo < Bar
    Sclass,      // class << self
    Pair,        // key => value
    Resbody,     // rescue Foo => e
    Ternary,     // ? or :
    Setter,      // x.y = v / x[i] = v  (operator is the `=` trailing the method name)
}

/// Is this kind considered an "assignment or comparison" for alignment?
/// Matches RuboCop's ASSIGNMENT_OR_COMPARISON_TOKENS check — any op whose text
/// is `=`, `==`, `===`, `!=`, `<=`, `>=`, `<<`, or an op-assign.
fn is_assign_or_cmp(kind: OpKind, text: &str) -> bool {
    match kind {
        OpKind::Assignment | OpKind::OpAssign | OpKind::Setter | OpKind::Sclass => true,
        OpKind::Send => matches!(text, "==" | "===" | "!=" | "<=" | ">=" | "<<"),
        OpKind::Class => false, // `<` for superclass — not in the set
        OpKind::Pair | OpKind::Resbody | OpKind::Ternary => false,
    }
}

#[derive(Clone, Debug)]
struct OpRecord {
    start_offset: usize,
    end_offset: usize,
    line: usize,        // 0-based
    start_col: usize,   // 0-based byte column
    end_col: usize,
    text: String,
    kind: OpKind,
    /// Start offset of the right-hand operand (for excess_trailing alignment).
    rhs_start: usize,
    /// End offset of the right-hand operand (for aligned_with_something range).
    /// If unknown, set equal to rhs_start.
    rhs_end: usize,
}

struct OperatorIndex {
    ops: Vec<OpRecord>,
    /// line -> Vec<index into ops>, sorted by start_col.
    by_line: HashMap<usize, Vec<usize>>,
    /// Start offset of each line (0-based).
    line_starts: Vec<usize>,
    /// Leading whitespace width per line (col of first non-ws char), or None for blank.
    line_indent: Vec<Option<usize>>,
    /// Source length for bounds checks.
    source_len: usize,
}

impl OperatorIndex {
    fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut line_starts = vec![0usize];
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' { line_starts.push(i + 1); }
        }
        let line_indent = line_starts
            .iter()
            .enumerate()
            .map(|(line_idx, &start)| {
                let end = if line_idx + 1 < line_starts.len() {
                    line_starts[line_idx + 1] - 1
                } else {
                    bytes.len()
                };
                let mut col = 0;
                while start + col < end && (bytes[start + col] == b' ' || bytes[start + col] == b'\t') {
                    col += 1;
                }
                if start + col >= end { None } else { Some(col) }
            })
            .collect();
        Self {
            ops: Vec::new(),
            by_line: HashMap::new(),
            line_starts,
            line_indent,
            source_len: bytes.len(),
        }
    }

    fn add(&mut self, rec: OpRecord) {
        self.ops.push(rec);
    }

    fn finalize(&mut self) {
        // Sort by (line, start_col) for stable iteration.
        self.ops.sort_by_key(|r| (r.line, r.start_col, r.start_offset));
        // Dedupe exact duplicates.
        self.ops.dedup_by(|a, b| a.start_offset == b.start_offset && a.end_offset == b.end_offset);
        // Build by_line.
        for (i, rec) in self.ops.iter().enumerate() {
            self.by_line.entry(rec.line).or_default().push(i);
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        // Binary search line_starts.
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    fn ops_on_line(&self, line: usize) -> Vec<&OpRecord> {
        self.by_line
            .get(&line)
            .map(|ixs| ixs.iter().map(|&i| &self.ops[i]).collect())
            .unwrap_or_default()
    }

    fn first_assign_or_cmp_on_line(&self, line: usize) -> Option<&OpRecord> {
        self.ops_on_line(line)
            .into_iter()
            .find(|op| is_assign_or_cmp(op.kind, &op.text))
    }

    fn has_assignment_on_line(&self, line: usize) -> bool {
        self.ops_on_line(line)
            .iter()
            .any(|op| matches!(op.kind, OpKind::Assignment | OpKind::OpAssign | OpKind::Setter))
    }

    fn line_count(&self) -> usize { self.line_starts.len() }

    fn line_indent(&self, line: usize) -> Option<usize> {
        self.line_indent.get(line).copied().flatten()
    }

    /// True if `line` is blank (only whitespace).
    fn is_blank(&self, line: usize) -> bool { self.line_indent(line).is_none() }

    /// Extract the substring for a given line (excluding trailing newline).
    fn line_source<'s>(&self, source: &'s str, line: usize) -> &'s str {
        if line >= self.line_starts.len() { return ""; }
        let start = self.line_starts[line];
        let end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1] - 1
        } else {
            source.len()
        };
        &source[start..end]
    }
}

// ────────────────────────────────────────────────────────────────────────────
//  Collector — walks AST, records each operator into the index.
// ────────────────────────────────────────────────────────────────────────────

const IRREGULAR_METHODS: &[&str] = &["[]", "!", "[]=", "`"];

struct Collector<'a> {
    ctx: &'a CheckContext<'a>,
    index: &'a mut OperatorIndex,
    hash_table_style: bool,
    parent_hash_multiline_stack: Vec<bool>,
}

impl<'a> Collector<'a> {
    fn record(&mut self, start: usize, end: usize, text: &str, kind: OpKind, rhs_start: usize, rhs_end: usize) {
        if start >= self.index.source_len || end > self.index.source_len { return; }
        let line = self.index.line_of(start);
        let line_start = self.index.line_starts[line];
        let start_col = start - line_start;
        let end_col = end - line_start;
        self.index.add(OpRecord {
            start_offset: start,
            end_offset: end,
            line,
            start_col,
            end_col,
            text: text.to_string(),
            kind,
            rhs_start,
            rhs_end,
        });
    }

    fn record_loc(&mut self, loc: ruby_prism::Location, kind: OpKind, rhs_start: usize, rhs_end: usize) {
        let s = loc.start_offset();
        let e = loc.end_offset();
        let text = &self.ctx.source[s..e];
        self.record(s, e, &text.to_string(), kind, rhs_start, rhs_end);
    }
}

impl<'a> Visit<'_> for Collector<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Setter call: `x.y = 2`, `z[0] = 0` — name ends with `=`.
        if node.receiver().is_some() {
            let name = node_name!(node);
            let is_setter = name.ends_with('=')
                && !matches!(&name[..], "==" | "===" | "!=" | "<=" | ">=" | "<=>" | "=~");
            if is_setter {
                if let Some(args) = node.arguments() {
                    if let Some(last) = args.arguments().iter().last() {
                        let bytes = self.ctx.source.as_bytes();
                        let recv_end = node.receiver().unwrap().location().end_offset();
                        let value_start = last.location().start_offset();
                        let value_end = last.location().end_offset();
                        let mut eq = value_start;
                        while eq > recv_end && bytes[eq - 1] != b'=' { eq -= 1; }
                        if eq > recv_end && bytes[eq - 1] == b'=' {
                            self.record(eq - 1, eq, "=", OpKind::Setter, value_start, value_end);
                        }
                    }
                }
                ruby_prism::visit_call_node(self, node);
                return;
            }
        }
        // Binary operator send: `a + b`, `a == b`, etc.
        if node.call_operator_loc().is_none() && node.receiver().is_some() {
            if let Some(sel) = node.message_loc() {
                let name = &self.ctx.source[sel.start_offset()..sel.end_offset()];
                let is_op = !name.is_empty()
                    && name.chars().all(|c| !c.is_alphanumeric() && c != '_')
                    && !IRREGULAR_METHODS.contains(&name);
                let no_dot = !is_dot_call(self.ctx.source, node);
                if is_op && no_dot {
                    if let Some(args) = node.arguments() {
                        if let Some(first) = args.arguments().iter().next() {
                            let rhs_start = first.location().start_offset();
                            let rhs_end = first.location().end_offset();
                            let recv_end = node.receiver().unwrap().location().end_offset();
                            if recv_end <= sel.start_offset() {
                                let bytes = self.ctx.source.as_bytes();
                                let mut i = sel.start_offset();
                                while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') { i -= 1; }
                                if i > 0 {
                                    self.record(sel.start_offset(), sel.end_offset(), name, OpKind::Send, rhs_start, rhs_end);
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
        let v = node.value().location();
        self.record_loc(op, OpKind::Assignment, v.start_offset(), v.end_offset());
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::Assignment, v.start_offset(), v.end_offset());
        ruby_prism::visit_instance_variable_write_node(self, node);
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::Assignment, v.start_offset(), v.end_offset());
        ruby_prism::visit_class_variable_write_node(self, node);
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::Assignment, v.start_offset(), v.end_offset());
        ruby_prism::visit_global_variable_write_node(self, node);
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::Assignment, v.start_offset(), v.end_offset());
        ruby_prism::visit_constant_write_node(self, node);
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::Assignment, v.start_offset(), v.end_offset());
        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let op = node.operator_loc();
        let rhs = node.right().location();
        self.record_loc(op, OpKind::Send, rhs.start_offset(), rhs.end_offset());
        ruby_prism::visit_and_node(self, node);
    }
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let op = node.operator_loc();
        let rhs = node.right().location();
        self.record_loc(op, OpKind::Send, rhs.start_offset(), rhs.end_offset());
        ruby_prism::visit_or_node(self, node);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_instance_variable_operator_write_node(&mut self, node: &ruby_prism::InstanceVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_instance_variable_operator_write_node(self, node);
    }
    fn visit_class_variable_operator_write_node(&mut self, node: &ruby_prism::ClassVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_class_variable_operator_write_node(self, node);
    }
    fn visit_global_variable_operator_write_node(&mut self, node: &ruby_prism::GlobalVariableOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_global_variable_operator_write_node(self, node);
    }
    fn visit_constant_operator_write_node(&mut self, node: &ruby_prism::ConstantOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_constant_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }
    fn visit_instance_variable_and_write_node(&mut self, node: &ruby_prism::InstanceVariableAndWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_instance_variable_and_write_node(self, node);
    }
    fn visit_class_variable_or_write_node(&mut self, node: &ruby_prism::ClassVariableOrWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_class_variable_or_write_node(self, node);
    }
    fn visit_class_variable_and_write_node(&mut self, node: &ruby_prism::ClassVariableAndWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_class_variable_and_write_node(self, node);
    }
    fn visit_global_variable_or_write_node(&mut self, node: &ruby_prism::GlobalVariableOrWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_global_variable_or_write_node(self, node);
    }
    fn visit_global_variable_and_write_node(&mut self, node: &ruby_prism::GlobalVariableAndWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_global_variable_and_write_node(self, node);
    }
    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_constant_or_write_node(self, node);
    }
    fn visit_constant_and_write_node(&mut self, node: &ruby_prism::ConstantAndWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_constant_and_write_node(self, node);
    }
    fn visit_index_operator_write_node(&mut self, node: &ruby_prism::IndexOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_index_operator_write_node(self, node);
    }
    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_index_or_write_node(self, node);
    }
    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_index_and_write_node(self, node);
    }
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode) {
        let op = node.binary_operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_call_operator_write_node(self, node);
    }
    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_call_or_write_node(self, node);
    }
    fn visit_call_and_write_node(&mut self, node: &ruby_prism::CallAndWriteNode) {
        let op = node.operator_loc();
        let v = node.value().location();
        self.record_loc(op, OpKind::OpAssign, v.start_offset(), v.end_offset());
        ruby_prism::visit_call_and_write_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        let multiline = !self.ctx.same_line(node.location().start_offset(), node.location().end_offset());
        self.parent_hash_multiline_stack.push(multiline);
        ruby_prism::visit_hash_node(self, node);
        self.parent_hash_multiline_stack.pop();
    }

    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
        if let Some(op) = node.operator_loc() {
            let op_src = &self.ctx.source[op.start_offset()..op.end_offset()];
            if op_src == "=>" {
                let multiline = *self.parent_hash_multiline_stack.last().unwrap_or(&false);
                // hash_table_style: skip `=>` checks on multiline hashes.
                if !(self.hash_table_style && multiline) {
                    let v = node.value().location();
                    self.record_loc(op, OpKind::Pair, v.start_offset(), v.end_offset());
                }
            }
        }
        ruby_prism::visit_assoc_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(q) = node.then_keyword_loc() {
            let q_src = &self.ctx.source[q.start_offset()..q.end_offset()];
            if q_src == "?" {
                if let Some(stmts) = node.statements() {
                    if let Some(first) = stmts.body().iter().next() {
                        let fl = first.location();
                        self.record_loc(q, OpKind::Ternary, fl.start_offset(), fl.end_offset());
                    }
                }
                if let Some(subseq) = node.subsequent() {
                    if let Some(else_node) = subseq.as_else_node() {
                        if let Some(stmts) = else_node.statements() {
                            if let Some(first) = stmts.body().iter().next() {
                                let else_loc = else_node.else_keyword_loc();
                                let colon_start = else_loc.start_offset();
                                let colon_end = else_loc.end_offset();
                                let colon_src = &self.ctx.source[colon_start..colon_end];
                                if colon_src == ":" {
                                    let fl = first.location();
                                    self.record(colon_start, colon_end, ":", OpKind::Ternary, fl.start_offset(), fl.end_offset());
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
        if let Some(op) = node.operator_loc() {
            if let Some(ref_node) = node.reference() {
                let rhs = ref_node.location();
                self.record_loc(op, OpKind::Resbody, rhs.start_offset(), rhs.end_offset());
            }
        }
        ruby_prism::visit_rescue_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(inherit_op) = node.inheritance_operator_loc() {
            if let Some(parent) = node.superclass() {
                let rhs = parent.location();
                self.record_loc(inherit_op, OpKind::Class, rhs.start_offset(), rhs.end_offset());
            }
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let op = node.operator_loc();
        let rhs = node.expression().location();
        self.record_loc(op, OpKind::Sclass, rhs.start_offset(), rhs.end_offset());
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_alternation_pattern_node(&mut self, node: &ruby_prism::AlternationPatternNode) {
        // `a | b` — operator `|`. RHS = right.
        let op = node.operator_loc();
        let rhs = node.right().location();
        self.record_loc(op, OpKind::Send, rhs.start_offset(), rhs.end_offset());
        ruby_prism::visit_alternation_pattern_node(self, node);
    }

    fn visit_capture_pattern_node(&mut self, node: &ruby_prism::CapturePatternNode) {
        // `bar => baz` — operator `=>`. RHS = target (baz).
        let op = node.operator_loc();
        let rhs = node.target().as_node().location();
        self.record_loc(op, OpKind::Pair, rhs.start_offset(), rhs.end_offset());
        ruby_prism::visit_capture_pattern_node(self, node);
    }

    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode) {
        // Top-level `value => pattern` — operator `=>`.
        let op = node.operator_loc();
        let rhs = node.pattern().location();
        self.record_loc(op, OpKind::Pair, rhs.start_offset(), rhs.end_offset());
        ruby_prism::visit_match_required_node(self, node);
    }
}

// ────────────────────────────────────────────────────────────────────────────
//  Check (pass 2)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align { Yes, No, Missing }

impl SpaceAroundOperators {
    fn check_one(&self, op: &OpRecord, index: &OperatorIndex, ctx: &CheckContext) -> Option<Offense> {
        let src = ctx.source;
        let bytes = src.as_bytes();
        let op_start = op.start_offset;
        let op_end = op.end_offset;
        let op_text = &op.text;

        // Skip if operator is the first non-whitespace on its line (continuation).
        {
            let mut p = op_start;
            while p > 0 && (bytes[p - 1] == b' ' || bytes[p - 1] == b'\t') { p -= 1; }
            if p == 0 || bytes[p - 1] == b'\n' { return None; }
        }

        let at_eol = op_end >= bytes.len() || bytes[op_end] == b'\n';
        let left_ws = count_ws_before(bytes, op_start);
        if at_eol {
            if left_ws == 0 {
                let msg = format!("Surrounding space missing for operator `{}`.", op_text);
                let correction = Correction::replace(op_start, op_end, format!(" {}", op_text));
                return Some(make_offense(ctx, op_start, op_end, &msg, correction));
            }
            return None;
        }
        let right_ws = count_ws_after(bytes, op_end);
        let right_is_comment = op_end + right_ws < bytes.len() && bytes[op_end + right_ws] == b'#';

        // should_not_have_space: ** / rational /
        let should_not_have_space = (op_text == "**" && !self.exponent_space)
            || (op_text == "/" && !self.slash_space && is_rhs_rational(src, op.rhs_start));
        if should_not_have_space {
            if left_ws > 0 || right_ws > 0 {
                let msg = format!("Space around operator `{}` detected.", op_text);
                let correction = Correction::replace(op_start - left_ws, op_end + right_ws, op_text.to_string());
                return Some(make_offense(ctx, op_start, op_end, &msg, correction));
            }
            return None;
        }

        if left_ws == 0 || right_ws == 0 {
            let msg = format!("Surrounding space missing for operator `{}`.", op_text);
            let correction = Correction::replace(op_start - left_ws, op_end + right_ws, format!(" {} ", op_text));
            return Some(make_offense(ctx, op_start, op_end, &msg, correction));
        }

        // Excess space.
        let raw_excess_left = left_ws > 1;
        let raw_excess_right = right_ws > 1 && !right_is_comment;
        if !raw_excess_left && !raw_excess_right { return None; }

        // Apply AllowForAlignment filter.
        let excess_left = if !raw_excess_left || !self.allow_for_alignment {
            raw_excess_left
        } else {
            !self.is_leading_excess_allowed(op, index)
        };
        let excess_right = if !raw_excess_right || !self.allow_for_alignment {
            raw_excess_right
        } else {
            !aligned_with_something(op, index, src)
        };

        if excess_left || excess_right {
            let msg = format!("Operator `{}` should be surrounded by a single space.", op_text);
            let correction = Correction::replace(op_start - left_ws, op_end + right_ws, format!(" {} ", op_text));
            return Some(make_offense(ctx, op_start, op_end, &msg, correction));
        }
        None
    }

    /// Returns true iff RuboCop would consider the leading excess acceptable
    /// (so we should NOT flag). Mirrors `excess_leading_space?` inverted.
    fn is_leading_excess_allowed(&self, op: &OpRecord, index: &OperatorIndex) -> bool {
        // For plain `=` assignments, use preceding/subsequent equals-sign alignment.
        if op.kind == OpKind::Assignment {
            let pre = aligned_with_preceding_equals_operator(op, index);
            if pre == Align::Yes { return true; }
            let sub = aligned_with_subsequent_equals_operator(op, index);
            if sub == Align::Missing { return true; }
            return sub == Align::Yes;
        }
        // For op-assign, setter, binary ops, pair, ternary, class, sclass, resbody:
        // aligned_with_operator? — aligned_identical or aligned_equals_operator.
        aligned_with_operator(op, index)
    }
}

fn make_offense(ctx: &CheckContext, start: usize, end: usize, msg: &str, correction: Correction) -> Offense {
    let loc = Location::from_offsets(ctx.source, start, end);
    Offense::new("Layout/SpaceAroundOperators", msg, Severity::Convention, loc, ctx.filename)
        .with_correction(correction)
}

// ────────────────────────────────────────────────────────────────────────────
//  Alignment predicates (port of PrecedingFollowingAlignment)
// ────────────────────────────────────────────────────────────────────────────

/// `aligned_with_preceding_equals_operator(token)` — walks from token.line down
/// to line 1, collecting assignment lines at the same indentation; returns the
/// second such line's alignment status (Yes / No / Missing).
fn aligned_with_preceding_equals_operator(op: &OpRecord, index: &OperatorIndex) -> Align {
    // Line range: op.line downto 0 (inclusive, descending).
    let mut range: Vec<usize> = (0..=op.line).rev().collect();
    aligned_with_equals_sign(op, index, &mut range)
}

/// `aligned_with_subsequent_equals_operator(token)` — walks from token.line up
/// to last line.
fn aligned_with_subsequent_equals_operator(op: &OpRecord, index: &OperatorIndex) -> Align {
    let mut range: Vec<usize> = (op.line..index.line_count()).collect();
    aligned_with_equals_sign(op, index, &mut range)
}

fn aligned_with_equals_sign(op: &OpRecord, index: &OperatorIndex, line_range: &mut [usize]) -> Align {
    let token_indent = index.line_indent(op.line).unwrap_or(0);
    let relevant = relevant_assignment_lines(index, line_range);
    // assignment_lines[1] in Ruby is the SECOND element (0-based: index 1).
    let Some(&rel_line) = relevant.get(1) else { return Align::Missing; };
    let rel_indent = index.line_indent(rel_line).unwrap_or(0);
    if rel_indent < token_indent { return Align::Missing; }
    if rel_line >= index.line_count() { return Align::Missing; }
    if aligned_equals_operator_on_line(op, rel_line, index) { Align::Yes } else { Align::No }
}

/// Walk `line_range` collecting lines that (a) contain an assignment at the
/// same indentation as the first line in range, and (b) have not yet crossed an
/// indent-drop or trailing blank-at-level.  Mirrors RuboCop's `relevant_assignment_lines`.
fn relevant_assignment_lines(index: &OperatorIndex, line_range: &[usize]) -> Vec<usize> {
    let mut result = Vec::new();
    let first = match line_range.first() { Some(&l) => l, None => return result };
    let original_indent = index.line_indent(first).unwrap_or(0);
    let mut relevant_at_level = true;
    for &n in line_range {
        let cur_indent = index.line_indent(n).unwrap_or(0);
        let blank = index.is_blank(n);
        if (cur_indent < original_indent && !blank) || (relevant_at_level && blank) {
            break;
        }
        if index.has_assignment_on_line(n) && cur_indent == original_indent {
            result.push(n);
        }
        if !blank {
            relevant_at_level = cur_indent == original_indent;
        }
    }
    result
}

/// `aligned_equals_operator?` — find first ASSIGNMENT_OR_COMPARISON token on
/// `line`, check if its end col matches our op's end col (with end-char rules).
fn aligned_equals_operator_on_line(op: &OpRecord, line: usize, index: &OperatorIndex) -> bool {
    let Some(tok) = index.first_assign_or_cmp_on_line(line) else { return false; };
    let ends_with_eq = op.text.ends_with('=');
    let tok_ends_eq = tok.text.ends_with('=');
    let same_end_col = op.end_col == tok.end_col;
    // aligned_with_preceding_equals?: both end with `=` and end cols match.
    if ends_with_eq && tok_ends_eq && same_end_col { return true; }
    // aligned_with_append_operator?:
    //   (op is `<<` AND tok is assignment-like `equal_sign?`) OR
    //   (op ends with `=` AND tok is `<<`)
    //   AND end cols match.
    if same_end_col {
        if op.text == "<<" && (tok_ends_eq || tok.text == "<<") { return true; }
        if ends_with_eq && tok.text == "<<" { return true; }
    }
    false
}

/// `aligned_with_operator?(range)` on an operator — aligned_identical? || aligned_equals_operator?.
fn aligned_with_operator(op: &OpRecord, index: &OperatorIndex) -> bool {
    adjacent_line_any(op.line, index, |line| {
        aligned_identical_on_line(op, line, index) || aligned_equals_operator_on_line(op, line, index)
    })
}

/// `aligned_with_something?(range)` on the RHS operand — aligned_words? || aligned_equals_operator?.
fn aligned_with_something(op: &OpRecord, index: &OperatorIndex, source: &str) -> bool {
    let rhs_line = index.line_of(op.rhs_start);
    let rhs_start_col = op.rhs_start - index.line_starts[rhs_line];
    let rhs_text = &source[op.rhs_start..op.rhs_end.max(op.rhs_start)];
    adjacent_line_any(rhs_line, index, |line| {
        aligned_words_on_line(rhs_start_col, rhs_text, line, index, source)
            || aligned_equals_operator_for_rhs(rhs_start_col, rhs_text, line, index)
    })
}

/// `aligned_with_adjacent_line?` — look above and below `target_line`, first without
/// an indent filter (first non-blank line wins), then with a base-indent filter
/// (first line matching `target_line`'s indent wins). `predicate(line)` checks the
/// alignment condition and short-circuits the walk.
fn adjacent_line_any(
    target_line: usize,
    index: &OperatorIndex,
    mut predicate: impl FnMut(usize) -> bool,
) -> bool {
    let pre: Vec<usize> = (0..target_line).rev().collect();
    let post: Vec<usize> = ((target_line + 1)..index.line_count()).collect();
    let first_match = |lines: &[usize], indent_filter: Option<usize>, pred: &mut dyn FnMut(usize) -> bool| -> bool {
        for &l in lines {
            match indent_filter {
                None => {
                    if index.is_blank(l) { continue; }
                    return pred(l);
                }
                Some(want) => match index.line_indent(l) {
                    Some(i) if i == want => return pred(l),
                    _ => continue,
                },
            }
        }
        false
    };
    if first_match(&pre, None, &mut predicate) || first_match(&post, None, &mut predicate) {
        return true;
    }
    if let Some(indent) = index.line_indent(target_line) {
        if first_match(&pre, Some(indent), &mut predicate)
            || first_match(&post, Some(indent), &mut predicate)
        {
            return true;
        }
    }
    false
}

/// `aligned_identical?(range, line)` — check if any recorded op on `line` has the
/// same start_col and same text.
fn aligned_identical_on_line(op: &OpRecord, line: usize, index: &OperatorIndex) -> bool {
    index
        .ops_on_line(line)
        .iter()
        .any(|o| o.start_col == op.start_col && o.text == op.text)
}

/// `aligned_words?(range, line)` — a space→non-space transition at `left_edge`, or
/// the same token text starting at `left_edge`. `text` is the RHS operand source.
fn aligned_words_on_line(left_edge: usize, text: &str, line: usize, index: &OperatorIndex, source: &str) -> bool {
    if left_edge == 0 { return false; }
    let line_src = index.line_source(source, line);
    let line_bytes = line_src.as_bytes();
    if left_edge < line_bytes.len() {
        let a = line_bytes[left_edge - 1];
        let b = line_bytes[left_edge];
        if is_space(a) && !is_space(b) { return true; }
    }
    let tok_len = text.len();
    if left_edge + tok_len <= line_bytes.len() && &line_src[left_edge..left_edge + tok_len] == text {
        return true;
    }
    false
}

/// `aligned_equals_operator?(rhs_range, line)` — for a RHS probe (start_col, text).
/// Mostly dormant since RHS text rarely ends with `=`; retained for fidelity.
fn aligned_equals_operator_for_rhs(start_col: usize, text: &str, line: usize, index: &OperatorIndex) -> bool {
    let Some(tok) = index.first_assign_or_cmp_on_line(line) else { return false; };
    let end_col = start_col + text.len();
    let ends_eq = text.ends_with('=');
    let tok_ends_eq = tok.text.ends_with('=');
    if ends_eq && tok_ends_eq && end_col == tok.end_col { return true; }
    if end_col == tok.end_col {
        if text == "<<" && (tok_ends_eq || tok.text == "<<") { return true; }
        if ends_eq && tok.text == "<<" { return true; }
    }
    false
}

// ────────────────────────────────────────────────────────────────────────────
//  Low-level helpers
// ────────────────────────────────────────────────────────────────────────────

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

fn is_space(b: u8) -> bool { b == b' ' || b == b'\t' }

fn is_rhs_rational(source: &str, right_start: usize) -> bool {
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
    let bytes = source.as_bytes();
    if let Some(sel) = node.message_loc() {
        let mut i = sel.start_offset();
        while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') { i -= 1; }
        if i > 0 && bytes[i - 1] == b'.' { return true; }
    }
    false
}

crate::register_cop!("Layout/SpaceAroundOperators", |cfg| {
    let c = cfg.get_cop_config("Layout/SpaceAroundOperators");
    let allow_for_alignment = c.and_then(|c| c.raw.get("AllowForAlignment")).and_then(|v| v.as_bool()).unwrap_or(true);
    let exp = c.and_then(|c| c.raw.get("EnforcedStyleForExponentOperator")).and_then(|v| v.as_str()).map(|s| s == "space").unwrap_or(false);
    let sl = c.and_then(|c| c.raw.get("EnforcedStyleForRationalLiterals")).and_then(|v| v.as_str()).map(|s| s == "space").unwrap_or(false);
    let hash_table_style = cfg
        .get_cop_config("Layout/HashAlignment")
        .and_then(|c| c.raw.get("EnforcedHashRocketStyle"))
        .map(|v| match v {
            serde_yaml::Value::String(s) => s == "table",
            serde_yaml::Value::Sequence(seq) => seq.iter().any(|x| x.as_str() == Some("table")),
            _ => false,
        })
        .unwrap_or(false);
    Some(Box::new(SpaceAroundOperators::with_config(allow_for_alignment, exp, sl, hash_table_style)))
});
