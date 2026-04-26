//! Layout/RedundantLineBreak — flags expressions that fit on a single line but are split across multiple.
//!
//! Ported from: https://raw.githubusercontent.com/rubocop/rubocop/v1.85.0/lib/rubocop/cop/layout/redundant_line_break.rb
//! With CheckSingleLineSuitability mixin in src/helpers/single_line_suitability.rs

use crate::cops::{CheckContext, Cop};
use crate::helpers::single_line_suitability::{comment_within, safe_to_split, to_single_line};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP: &str = "Layout/RedundantLineBreak";
const MSG: &str = "Redundant line break detected.";

pub struct RedundantLineBreak {
    inspect_blocks: bool,
    line_length_max: Option<usize>,
    single_line_block_chain_enabled: bool,
}

impl RedundantLineBreak {
    pub fn new(inspect_blocks: bool, line_length_max: Option<usize>, slbc_enabled: bool) -> Self {
        Self { inspect_blocks, line_length_max, single_line_block_chain_enabled: slbc_enabled }
    }
}

impl Cop for RedundantLineBreak {
    fn name(&self) -> &'static str { COP }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let percent_blank_end = ctx.source.ends_with("%\n\n");
        let mut v = RVisitor {
            ctx,
            cfg: self,
            stack: Vec::new(),
            offenses: Vec::new(),
            reported: Vec::new(),
            percent_blank_end,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Call,
    BinOp,
    Assign,
}

#[derive(Clone)]
struct StackFrame {
    /// Node start_offset
    start: usize,
    /// Node end_offset
    end: usize,
    kind: FrameKind,
    /// For call: receiver start_offset (if any)
    receiver_start: Option<usize>,
}

struct RVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cfg: &'a RedundantLineBreak,
    stack: Vec<StackFrame>,
    offenses: Vec<Offense>,
    reported: Vec<(usize, usize)>,
    percent_blank_end: bool,
}

/// For a CallNode, return the effective end offset.
/// In Prism the BlockNode is a child of CallNode; in RuboCop's parser the block wraps the send.
/// RuboCop walks `node = node.parent while ... convertible_block?(node)` where convertible_block
/// is `parent block && node==parent.send_node && (parenthesized? || !arguments?)`.
/// So if call has block AND (parens OR no-args), include the block; otherwise exclude it.
fn call_effective_end(node: &ruby_prism::CallNode, source: &str) -> usize {
    let Some(blk) = node.block() else { return node.location().end_offset(); };
    let has_parens = node.opening_loc().is_some();
    let has_args = node.arguments().is_some();
    if has_parens || !has_args {
        node.location().end_offset()
    } else {
        // Send-only range: trim whitespace before `do` keyword.
        let mut e = blk.location().start_offset();
        let bytes = source.as_bytes();
        while e > 0 && (bytes[e - 1] == b' ' || bytes[e - 1] == b'\t') {
            e -= 1;
        }
        e
    }
}

/// Detects if `node` is `x[...]` where `x` is itself a `[]`-call.
fn index_access_call_chained(node: &ruby_prism::CallNode) -> bool {
    let name = String::from_utf8_lossy(node.name().as_slice());
    if name != "[]" { return false; }
    if let Some(recv) = node.receiver() {
        if let Some(rc) = recv.as_call_node() {
            let rname = String::from_utf8_lossy(rc.name().as_slice());
            return rname == "[]";
        }
    }
    false
}

/// True if any descendant block in `node` (within byte range) is multiline.
fn any_multiline_block_in_range(node: &Node<'_>, source: &str, start: usize, end: usize) -> bool {
    struct BV<'a> { src: &'a str, found: bool, start: usize, end: usize }
    impl<'a, 'b> Visit<'b> for BV<'a> {
        fn visit_block_node(&mut self, n: &ruby_prism::BlockNode) {
            if self.found { return; }
            let l = n.location();
            if l.start_offset() < self.start || l.end_offset() > self.end {
                ruby_prism::visit_block_node(self, n);
                return;
            }
            let span = &self.src.as_bytes()[l.start_offset()..l.end_offset()];
            if span.contains(&b'\n') { self.found = true; return; }
            ruby_prism::visit_block_node(self, n);
        }
    }
    let mut bv = BV { src: source, found: false, start, end };
    bv.visit(node);
    bv.found
}

/// True if any descendant block in `node` is multiline.
fn any_multiline_block_descendant(node: &Node<'_>, source: &str) -> bool {
    struct BV<'a> { src: &'a str, found: bool }
    impl<'a, 'b> Visit<'b> for BV<'a> {
        fn visit_block_node(&mut self, n: &ruby_prism::BlockNode) {
            if self.found { return; }
            let l = n.location();
            let span = &self.src.as_bytes()[l.start_offset()..l.end_offset()];
            if span.contains(&b'\n') { self.found = true; return; }
            ruby_prism::visit_block_node(self, n);
        }
    }
    let mut bv = BV { src: source, found: false };
    bv.visit(node);
    bv.found
}

/// `Layout/SingleLineBlockChain` precedence: descendant block whose parent send has a dot
/// and whose block is single-line.
fn has_dot_chained_single_line_block(node: &Node<'_>, source: &str) -> bool {
    struct V<'a> { src: &'a str, found: bool }
    impl<'a, 'b> Visit<'b> for V<'a> {
        fn visit_call_node(&mut self, n: &ruby_prism::CallNode) {
            if self.found { return; }
            if let Some(recv) = n.receiver() {
                if let Some(rc) = recv.as_call_node() {
                    if let Some(blk) = rc.block() {
                        let l = blk.location();
                        let span = &self.src.as_bytes()[l.start_offset()..l.end_offset()];
                        if !span.contains(&b'\n') {
                            if let Some(op) = n.call_operator_loc() {
                                let ob = op.as_slice();
                                if ob == b"." || ob == b"&." {
                                    self.found = true;
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V { src: source, found: false };
    v.visit(node);
    v.found
}

impl<'a, 'b> Visit<'b> for RVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let n_loc = node.location();
        let n_start = n_loc.start_offset();
        // Use effective end (excludes trailing block) as the offense range AND for whole-line scan.
        let n_end = call_effective_end(node, self.ctx.source);

        if !self.parent_qualifies_for_call(n_start, n_end) {
            self.report_outermost(n_start, n_end, OuterKind::Call(node));
        }

        let frame = StackFrame {
            start: n_start,
            end: node.location().end_offset(),
            kind: FrameKind::Call,
            receiver_start: node.receiver().map(|r| r.location().start_offset()),
        };
        self.stack.push(frame);
        ruby_prism::visit_call_node(self, node);
        self.stack.pop();
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let l = node.location();
        let n_start = l.start_offset();
        let n_end = l.end_offset();
        if !self.parent_qualifies_for_binop(n_start, n_end) {
            self.report_outermost(n_start, n_end, OuterKind::Or(node));
        }
        let frame = StackFrame { start: n_start, end: n_end, kind: FrameKind::BinOp, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_or_node(self, node);
        self.stack.pop();
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let l = node.location();
        let n_start = l.start_offset();
        let n_end = l.end_offset();
        if !self.parent_qualifies_for_binop(n_start, n_end) {
            self.report_outermost(n_start, n_end, OuterKind::And(node));
        }
        let frame = StackFrame { start: n_start, end: n_end, kind: FrameKind::BinOp, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_and_node(self, node);
        self.stack.pop();
    }

    fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_local_variable_write_node(self, n);
        self.stack.pop();
    }

    fn visit_instance_variable_write_node(&mut self, n: &ruby_prism::InstanceVariableWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_instance_variable_write_node(self, n);
        self.stack.pop();
    }

    fn visit_class_variable_write_node(&mut self, n: &ruby_prism::ClassVariableWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_class_variable_write_node(self, n);
        self.stack.pop();
    }

    fn visit_global_variable_write_node(&mut self, n: &ruby_prism::GlobalVariableWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_global_variable_write_node(self, n);
        self.stack.pop();
    }

    fn visit_constant_write_node(&mut self, n: &ruby_prism::ConstantWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_constant_write_node(self, n);
        self.stack.pop();
    }

    fn visit_constant_path_write_node(&mut self, n: &ruby_prism::ConstantPathWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_constant_path_write_node(self, n);
        self.stack.pop();
    }

    fn visit_multi_write_node(&mut self, n: &ruby_prism::MultiWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_multi_write_node(self, n);
        self.stack.pop();
    }

    fn visit_local_variable_operator_write_node(&mut self, n: &ruby_prism::LocalVariableOperatorWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_local_variable_operator_write_node(self, n);
        self.stack.pop();
    }

    fn visit_local_variable_or_write_node(&mut self, n: &ruby_prism::LocalVariableOrWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_local_variable_or_write_node(self, n);
        self.stack.pop();
    }

    fn visit_local_variable_and_write_node(&mut self, n: &ruby_prism::LocalVariableAndWriteNode) {
        self.handle_assignment_node(&n.as_node());
        let l = n.location();
        let frame = StackFrame { start: l.start_offset(), end: l.end_offset(), kind: FrameKind::Assign, receiver_start: None };
        self.stack.push(frame);
        ruby_prism::visit_local_variable_and_write_node(self, n);
        self.stack.pop();
    }
}

enum OuterKind<'a, 'pr> {
    Call(&'a ruby_prism::CallNode<'pr>),
    Or(&'a ruby_prism::OrNode<'pr>),
    And(&'a ruby_prism::AndNode<'pr>),
}

impl<'a> RVisitor<'a> {
    /// True if any direct stack ancestor would be the "ascend target" for a Call child.
    /// RuboCop walks up while parent.send_type? || convertible_block? || BinaryOperatorNode.
    /// With Prism we ascend if parent is:
    ///  - a CallNode whose receiver is THIS child (chained call)
    ///  - a BinOp (or/and)
    fn parent_qualifies_for_call(&self, child_start: usize, child_end: usize) -> bool {
        let Some(top) = self.stack.last() else { return false; };
        match top.kind {
            FrameKind::Call => {
                top.receiver_start.is_some_and(|rs| rs == child_start && top.end >= child_end)
            }
            FrameKind::BinOp => {
                top.start <= child_start && top.end >= child_end
            }
            FrameKind::Assign => false,
        }
    }

    /// Same but for binop child: ascend if parent is binop (chain `a || b || c`).
    fn parent_qualifies_for_binop(&self, child_start: usize, child_end: usize) -> bool {
        let Some(top) = self.stack.last() else { return false; };
        match top.kind {
            FrameKind::BinOp => top.start <= child_start && top.end >= child_end,
            FrameKind::Call => {
                // a binop arg of a send doesn't ascend in RuboCop (binop parent must be send for ascend); skip.
                false
            }
            FrameKind::Assign => false,
        }
    }

    /// Handle assignment node: report on whole assignment if it fits, else allow rhs to be inspected.
    fn handle_assignment_node(&mut self, node: &Node<'_>) {
        if self.percent_blank_end { return; }
        let l = node.location();
        let start = l.start_offset();
        let end = l.end_offset();
        // Already reported?
        for (s, e) in &self.reported {
            if start >= *s && end <= *e { return; }
        }
        // multiline?
        let snippet = self.ctx.src(start, end);
        if !snippet.contains('\n') { return; }
        if !self.suitable_as_single_line(start, end) { return; }
        if comment_within(self.ctx.source, self.ctx.line_of(start), self.ctx.line_of(end.saturating_sub(1).max(start))) { return; }
        // configured_to_not_be_inspected? checks descendants
        if self.cfg.single_line_block_chain_enabled
            && has_dot_chained_single_line_block(node, self.ctx.source)
        {
            return;
        }
        if !self.cfg.inspect_blocks {
            // any_block_type? check is irrelevant for assignment node itself; check descendants
            if any_multiline_block_descendant(node, self.ctx.source) { return; }
        }
        if !safe_to_split(node, self.ctx.source) { return; }

        self.reported.push((start, end));
        let single = to_single_line(snippet).trim().to_string();
        let mut off = self.ctx.offense_with_range(COP, MSG, Severity::Convention, start, end);
        off.correction = Some(Correction { edits: vec![Edit { start_offset: start, end_offset: end, replacement: single }] });
        self.offenses.push(off);
    }

    /// suitable_as_single_line — implements RuboCop's CheckSingleLineSuitability::too_long? + comment_within
    /// using whole-line approach (`processed_source.lines[(first_line-1)...last_line]`).
    fn suitable_as_single_line(&self, start: usize, end: usize) -> bool {
        if let Some(max) = self.cfg.line_length_max {
            let first_line = self.ctx.line_of(start);
            let last_line = self.ctx.line_of(end.saturating_sub(1).max(start));
            let line_text = self.whole_lines(first_line, last_line);
            let collapsed = to_single_line(&line_text);
            if collapsed.chars().count() > max { return false; }
        }
        let fl = self.ctx.line_of(start);
        let ll = self.ctx.line_of(end.saturating_sub(1).max(start));
        if comment_within(self.ctx.source, fl, ll) { return false; }
        true
    }

    fn whole_lines(&self, first_line: usize, last_line: usize) -> String {
        let bytes = self.ctx.source.as_bytes();
        let mut start_off = 0usize;
        let mut line = 1usize;
        for (i, &b) in bytes.iter().enumerate() {
            if line == first_line { start_off = i; break; }
            if b == b'\n' { line += 1; }
        }
        if line < first_line {
            // first_line beyond EOF
            return String::new();
        }
        // find end_off: end of last_line (exclusive of trailing newline)
        let mut end_off = bytes.len();
        let mut line2 = 1usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                if line2 == last_line { end_off = i; break; }
                line2 += 1;
            }
        }
        std::str::from_utf8(&bytes[start_off..end_off]).unwrap_or("").to_string()
    }

    fn report_outermost(&mut self, start: usize, end: usize, kind: OuterKind<'_, '_>) {
        for (s, e) in &self.reported {
            if start >= *s && end <= *e { return; }
        }

        // multiline?
        let snippet = self.ctx.src(start, end);
        if !snippet.contains('\n') { return; }

        if !self.suitable_as_single_line(start, end) { return; }

        // safe_to_split via the typed node
        let safe = match &kind {
            OuterKind::Call(c) => safe_to_split(&c.as_node(), self.ctx.source),
            OuterKind::Or(o) => safe_to_split(&o.as_node(), self.ctx.source),
            OuterKind::And(a) => safe_to_split(&a.as_node(), self.ctx.source),
        };
        if !safe { return; }

        // Per-kind:
        match &kind {
            OuterKind::Call(c) => {
                if index_access_call_chained(c) { return; }
                if self.cfg.single_line_block_chain_enabled
                    && has_dot_chained_single_line_block(&c.as_node(), self.ctx.source)
                {
                    return;
                }
                if !self.cfg.inspect_blocks {
                    // The "node" we're reporting on may be call-with-block (convertible) or
                    // send-only (non-convertible). Use the same effective-end logic.
                    let n_end = call_effective_end(c, self.ctx.source);
                    let has_block_in_offense = c.block().is_some()
                        && c.location().end_offset() == n_end;
                    if has_block_in_offense { return; }
                    // Check descendants within the offense range only.
                    let n_start = c.location().start_offset();
                    if any_multiline_block_in_range(&c.as_node(), self.ctx.source, n_start, n_end) {
                        return;
                    }
                }
            }
            OuterKind::Or(o) => {
                let op_off = o.operator_loc().start_offset();
                let op_line = self.ctx.line_of(op_off);
                let line_off = self.line_offset(op_line);
                let line_text = self.ctx.line_text(line_off);
                if !line_text.trim_end_matches(|c: char| c == ' ' || c == '\t').ends_with('\\') {
                    return;
                }
            }
            OuterKind::And(a) => {
                let op_off = a.operator_loc().start_offset();
                let op_line = self.ctx.line_of(op_off);
                let line_off = self.line_offset(op_line);
                let line_text = self.ctx.line_text(line_off);
                if !line_text.trim_end_matches(|c: char| c == ' ' || c == '\t').ends_with('\\') {
                    return;
                }
            }
        }

        self.reported.push((start, end));
        let single = to_single_line(snippet).trim().to_string();
        let mut off = self.ctx.offense_with_range(COP, MSG, Severity::Convention, start, end);
        off.correction = Some(Correction { edits: vec![Edit { start_offset: start, end_offset: end, replacement: single }] });
        self.offenses.push(off);
    }

    fn line_offset(&self, line: usize) -> usize {
        let mut i = 0usize;
        let mut n = 1;
        for &b in self.ctx.source.as_bytes() {
            if n == line { return i; }
            if b == b'\n' { n += 1; }
            i += 1;
        }
        i
    }
}

crate::register_cop!("Layout/RedundantLineBreak", |cfg| {
    let inspect_blocks = cfg
        .get_cop_config("Layout/RedundantLineBreak")
        .and_then(|c| c.raw.get("InspectBlocks"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let line_length_max = if cfg.is_cop_enabled("Layout/LineLength") {
        cfg.get_cop_config("Layout/LineLength")
            .and_then(|c| c.raw.get("Max"))
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
    } else {
        None
    };
    let slbc_enabled = cfg.is_cop_enabled("Layout/SingleLineBlockChain");
    Some(Box::new(RedundantLineBreak::new(inspect_blocks, line_length_max, slbc_enabled)))
});
