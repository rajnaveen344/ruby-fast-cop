//! Layout/BlockAlignment — alignment of `end` / `}` of do/end + brace blocks.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAlignmentStyle {
    Either,
    StartOfBlock,
    StartOfLine,
}

pub struct BlockAlignment {
    style: BlockAlignmentStyle,
}

impl BlockAlignment {
    pub fn new(style: BlockAlignmentStyle) -> Self {
        Self { style }
    }
}

impl Default for BlockAlignment {
    fn default() -> Self {
        Self::new(BlockAlignmentStyle::Either)
    }
}

impl Cop for BlockAlignment {
    fn name(&self) -> &'static str {
        "Layout/BlockAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = BlockVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
            stack: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FrameKind {
    /// Plain assignment: `x = ...`, `foo.bar = ...`, `foo[x] = ...`, `@x = ...`, `$x = ...`, `CONST = ...`.
    /// find_lhs_node does NOT unwrap these.
    Assignment,
    /// OpAsgn: `x += ...`. find_lhs_node unwraps to `x`.
    OpAsgn,
    /// MultiWrite: `a, b = ...`. find_lhs_node unwraps to LHS mlhs.
    MultiWrite,
    Def,
    Splat,
    And,
    Or,
    /// `send _ :<<` — current is the argument of `<<`.
    LShift,
    /// `send equal?(child) !:[]` — current node is the receiver of parent send.
    CallReceiver,
    Other,
}

#[derive(Clone)]
struct StackFrame {
    kind: FrameKind,
    /// Node's full location.
    start_off: usize,
    end_off: usize,
    first_line: usize,
    is_masgn: bool,
    /// LHS range used for offense message (for Assignment = whole node range; for OpAsgn = lhs;
    /// for MultiWrite = lhs mlhs range).
    lhs_start: usize,
    lhs_end: usize,
    /// For CallReceiver-kind frames: the start_offset of the receiver (or the call itself if no receiver).
    /// For LShift: the start_offset of the argument that holds the block (child that must match).
    /// For other kinds: 0 (unused).
    call_receiver_start: usize,
}

struct BlockVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: BlockAlignmentStyle,
    offenses: Vec<Offense>,
    stack: Vec<StackFrame>,
}

impl<'a> BlockVisitor<'a> {
    fn push_frame_full(
        &mut self,
        kind: FrameKind,
        start_off: usize,
        end_off: usize,
        lhs_start: usize,
        lhs_end: usize,
        call_receiver_start: usize,
    ) {
        let first_line = self.ctx.line_of(start_off);
        self.stack.push(StackFrame {
            kind,
            start_off,
            end_off: end_off.max(start_off),
            first_line,
            is_masgn: kind == FrameKind::MultiWrite,
            lhs_start,
            lhs_end,
            call_receiver_start,
        });
    }
    fn push_frame(&mut self, kind: FrameKind, start_off: usize, end_off: usize) {
        self.push_frame_full(kind, start_off, end_off, start_off, end_off, 0);
    }
    fn pop_frame(&mut self) {
        self.stack.pop();
    }

    /// Find the align target frame by walking up ancestors.
    fn find_align_frame(&self, block_start_off: usize) -> AlignFrame {
        let mut cur_first_line = self.ctx.line_of(block_start_off);
        let mut cur_lhs_start = block_start_off;
        let mut cur_lhs_end = block_start_off;
        let mut cur_kind: Option<FrameKind> = None;
        // Track the current node's start offset for "is current the receiver?" check.
        let mut cur_start = block_start_off;

        for i in (0..self.stack.len()).rev() {
            let parent = &self.stack[i];
            if parent.end_off < block_start_off || parent.start_off > block_start_off {
                continue;
            }
            let disqualified = parent.first_line != cur_first_line && !parent.is_masgn;

            // CallReceiver requires current to actually be the receiver of parent.
            // i.e., cur_start == parent.call_receiver_start.
            let pattern_match = match parent.kind {
                FrameKind::Assignment
                | FrameKind::OpAsgn
                | FrameKind::MultiWrite
                | FrameKind::Def
                | FrameKind::Splat
                | FrameKind::And
                | FrameKind::Or => true,
                FrameKind::LShift => {
                    // `a << b` — b (the argument with the block) must be current.
                    cur_start == parent.call_receiver_start
                }
                FrameKind::CallReceiver => {
                    // Parent is `recv.method(...)` — advance only if current is the receiver.
                    cur_start == parent.call_receiver_start
                }
                FrameKind::Other => false,
            };
            if disqualified || !pattern_match {
                break;
            }
            cur_first_line = parent.first_line;
            cur_lhs_start = parent.lhs_start;
            cur_lhs_end = parent.lhs_end;
            cur_kind = Some(parent.kind);
            cur_start = parent.start_off;
        }

        AlignFrame {
            first_line: cur_first_line,
            lhs_start: cur_lhs_start,
            lhs_end: cur_lhs_end,
            kind: cur_kind,
        }
    }

    fn first_line_source(&self, start: usize, end: usize) -> String {
        let src_end = self.ctx.source[start..].find('\n')
            .map(|p| start + p)
            .unwrap_or(self.ctx.source.len())
            .min(end);
        self.ctx.source[start..src_end].trim_end().to_string()
    }

    fn check_block_end(
        &mut self,
        block_enclosing_start_off: usize,
        opening_loc: &ruby_prism::Location,
        closing_loc: &ruby_prism::Location,
    ) {
        let end_off = closing_loc.start_offset();
        let end_end = closing_loc.end_offset();
        let end_text = &self.ctx.source[end_off..end_end];

        if !self.ctx.begins_its_line(end_off) {
            return;
        }

        let end_col = self.ctx.col_of(end_off);
        let end_line = self.ctx.line_of(end_off);

        let align = self.find_align_frame(block_enclosing_start_off);
        let start_line = align.first_line;
        let start_col = self.ctx.col_of(align.lhs_start);
        let start_text = self.first_line_source(align.lhs_start, align.lhs_end);

        // do-line: line containing `do`/`{`; col = first non-ws of that line; text = trimmed line.
        let do_off = opening_loc.start_offset();
        let do_line = self.ctx.line_of(do_off);
        let do_line_start = self.ctx.line_start(do_off);
        let do_line_end = self.ctx.source[do_line_start..]
            .find('\n')
            .map(|p| do_line_start + p)
            .unwrap_or(self.ctx.source.len());
        let do_line_bytes = self.ctx.source[do_line_start..do_line_end].as_bytes();
        let mut do_col = 0usize;
        while do_col < do_line_bytes.len()
            && (do_line_bytes[do_col] == b' ' || do_line_bytes[do_col] == b'\t')
        {
            do_col += 1;
        }
        let do_text = self.ctx.source[do_line_start + do_col..do_line_end]
            .trim_end()
            .to_string();

        let same_target = start_line == do_line && start_col == do_col;

        let should_emit = match self.style {
            BlockAlignmentStyle::Either => end_col != start_col && end_col != do_col,
            BlockAlignmentStyle::StartOfLine => end_col != start_col,
            BlockAlignmentStyle::StartOfBlock => end_col != do_col,
        };
        if !should_emit {
            return;
        }

        let current = format!("`{}` at {}, {}", end_text, end_line, end_col);
        let msg = match self.style {
            BlockAlignmentStyle::StartOfBlock => {
                format!("{} is not aligned with `{}` at {}, {}.", current, do_text, do_line, do_col)
            }
            BlockAlignmentStyle::StartOfLine => {
                format!("{} is not aligned with `{}` at {}, {}.", current, start_text, start_line, start_col)
            }
            BlockAlignmentStyle::Either => {
                if same_target {
                    format!(
                        "{} is not aligned with `{}` at {}, {}.",
                        current, start_text, start_line, start_col
                    )
                } else {
                    format!(
                        "{} is not aligned with `{}` at {}, {} or `{}` at {}, {}.",
                        current, start_text, start_line, start_col, do_text, do_line, do_col
                    )
                }
            }
        };

        let location = crate::offense::Location::from_offsets(self.ctx.source, end_off, end_end);
        self.offenses.push(Offense::new(
            "Layout/BlockAlignment",
            msg,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }
}

struct AlignFrame {
    first_line: usize,
    lhs_start: usize,
    lhs_end: usize,
    #[allow(dead_code)]
    kind: Option<FrameKind>,
}

// ---------- Helpers for computing LHS ranges ---------------------------------

fn is_attr_writer(name: &str) -> bool {
    if name == "[]=" {
        return true;
    }
    if !name.ends_with('=') || name.len() < 2 {
        return false;
    }
    // Exclude comparison operators: ==, ===, !=, <=, >=, <=>
    matches!(name, "==" | "===" | "!=" | "<=" | ">=" | "<=>") == false
        // Also exclude `=~`
        && name != "=~"
}

fn call_lhs_range(node: &ruby_prism::CallNode) -> (usize, usize) {
    // Use the full call location; first_line_source trims at the first newline so
    // the offense message correctly shows "test do |ala|" etc.
    let call_start = node.location().start_offset();
    let call_end = node.location().end_offset();
    (call_start, call_end)
}

// ---------- Visit impls ------------------------------------------------------

impl Visit<'_> for BlockVisitor<'_> {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // The CallNode enclosing this block is the most recent frame on the stack.
        // Its (start_off, end_off) covers the call-with-block; its start is the
        // alignment reference for the block itself.
        let block_enclosing_start_off = self
            .stack
            .last()
            .map(|f| f.start_off)
            .unwrap_or_else(|| node.location().start_offset());

        let open = node.opening_loc();
        let close = node.closing_loc();

        // Temporarily push a "CallReceiver"-like frame for the block itself so that
        // the block's source range (from call start to end) becomes the base if
        // no further ancestor matches. We represent the block frame with its call's
        // source range as LHS, but with kind=Other (so it won't be promoted further).
        // Actually: we want the block itself to be the starting "current" of the walk.
        // The find_align_frame already initializes cur_first_line = line_of(block_start).
        // We just need the start-of-line text. For the block-itself case the LHS text
        // comes from the CallNode stack top, so first_line_source(start, end_of_call) is OK.
        // But for `expect(arr.all? do |o| ...)`, the enclosing call is arr.all? (first pushed),
        // then expect() is pushed above, with block arg. We need the BlockNode's "start_node" to
        // be the arr.all? call. Since we use stack.last() for block_enclosing_start_off, and
        // visit_call_node pushes/pops its frame around its children — at visit_block_node time
        // the topmost frame *is* the call owning the block. Good.

        // For LHS text of the block when it doesn't walk up: use call's source range
        // (call_start .. min(call_end, first '\n')).
        let top_lhs_start;
        let top_lhs_end;
        if let Some(frame) = self.stack.last() {
            top_lhs_start = frame.lhs_start;
            top_lhs_end = frame.lhs_end;
        } else {
            top_lhs_start = node.location().start_offset();
            top_lhs_end = node.location().end_offset();
        }
        // Swap in the block's call as the "current" reference by temporarily replacing
        // the align lookup — since find_align_frame uses stack top's lhs_start/end already,
        // we're good. Just call check_block_end.
        let _ = (top_lhs_start, top_lhs_end);

        self.check_block_end(block_enclosing_start_off, &open, &close);
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let loc = node.location();
        let start_off = loc.start_offset();
        let end_off = loc.end_offset();

        let name = std::str::from_utf8(node.name().as_slice()).unwrap_or("");
        let kind = if name == "<<" {
            FrameKind::LShift
        } else if name == "[]" {
            FrameKind::Other
        } else if is_attr_writer(name) {
            // `foo[bar] = x`, `foo.attr = x` — treat as assignment.
            FrameKind::Assignment
        } else {
            FrameKind::CallReceiver
        };

        let (lhs_start, lhs_end) = call_lhs_range(node);
        // For CallReceiver: receiver_start = receiver's start offset. No receiver → start_off.
        // For LShift: receiver_start = argument's start offset (the thing holding block).
        let call_receiver_start = match kind {
            FrameKind::CallReceiver | FrameKind::Assignment => node
                .receiver()
                .map(|r| r.location().start_offset())
                .unwrap_or(start_off),
            FrameKind::LShift => {
                // `<<` is a method call; the argument with the block is args.arguments().first
                if let Some(args) = node.arguments() {
                    args.arguments()
                        .iter()
                        .next()
                        .map(|a| a.location().start_offset())
                        .unwrap_or(start_off)
                } else {
                    start_off
                }
            }
            _ => 0,
        };
        self.push_frame_full(kind, start_off, end_off, lhs_start, lhs_end, call_receiver_start);
        ruby_prism::visit_call_node(self, node);
        self.pop_frame();
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_local_variable_write_node(self, node);
        self.pop_frame();
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_instance_variable_write_node(self, node);
        self.pop_frame();
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_class_variable_write_node(self, node);
        self.pop_frame();
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_global_variable_write_node(self, node);
        self.pop_frame();
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_constant_write_node(self, node);
        self.pop_frame();
    }

    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_constant_path_write_node(self, node);
        self.pop_frame();
    }

    // OpAsgn — unwrap to LHS (target ident/receiver.attr etc.)
    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode,
    ) {
        let loc = node.location();
        let name_loc = node.name_loc();
        self.push_frame_full(
            FrameKind::OpAsgn,
            loc.start_offset(),
            loc.end_offset(),
            name_loc.start_offset(),
            name_loc.end_offset(),
            0,
        );
        ruby_prism::visit_local_variable_operator_write_node(self, node);
        self.pop_frame();
    }

    fn visit_instance_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableOperatorWriteNode,
    ) {
        let loc = node.location();
        let name_loc = node.name_loc();
        self.push_frame_full(
            FrameKind::OpAsgn,
            loc.start_offset(),
            loc.end_offset(),
            name_loc.start_offset(),
            name_loc.end_offset(),
            0,
        );
        ruby_prism::visit_instance_variable_operator_write_node(self, node);
        self.pop_frame();
    }

    // AndWrite / OrWrite: RuboCop's find_lhs_node does NOT unwrap these.
    // Treat as Assignment (full node range).
    fn visit_local_variable_and_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableAndWriteNode,
    ) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_local_variable_and_write_node(self, node);
        self.pop_frame();
    }

    fn visit_local_variable_or_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOrWriteNode,
    ) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_local_variable_or_write_node(self, node);
        self.pop_frame();
    }

    fn visit_instance_variable_and_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableAndWriteNode,
    ) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_instance_variable_and_write_node(self, node);
        self.pop_frame();
    }

    fn visit_instance_variable_or_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableOrWriteNode,
    ) {
        let loc = node.location();
        self.push_frame(FrameKind::Assignment, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_instance_variable_or_write_node(self, node);
        self.pop_frame();
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        let loc = node.location();
        // LHS = first left start to last left end
        let lefts: Vec<_> = node.lefts().iter().collect();
        let (lhs_start, lhs_end) = if lefts.is_empty() {
            (loc.start_offset(), loc.end_offset())
        } else {
            let first = lefts.first().unwrap().location();
            let last = lefts.last().unwrap().location();
            (first.start_offset(), last.end_offset())
        };
        self.push_frame_full(
            FrameKind::MultiWrite,
            loc.start_offset(),
            loc.end_offset(),
            lhs_start,
            lhs_end,
            0,
        );
        ruby_prism::visit_multi_write_node(self, node);
        self.pop_frame();
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Def, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_def_node(self, node);
        self.pop_frame();
    }

    fn visit_splat_node(&mut self, node: &ruby_prism::SplatNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Splat, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_splat_node(self, node);
        self.pop_frame();
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let loc = node.location();
        self.push_frame(FrameKind::And, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_and_node(self, node);
        self.pop_frame();
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let loc = node.location();
        self.push_frame(FrameKind::Or, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_or_node(self, node);
        self.pop_frame();
    }
}
