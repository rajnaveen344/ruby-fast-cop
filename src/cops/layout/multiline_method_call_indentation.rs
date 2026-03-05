//! Layout/MultilineMethodCallIndentation - Checks indentation of method calls spanning multiple lines.
//!
//! Ensures that continuation lines in method call chains are properly indented according to
//! the configured style: aligned, indented, or indented_relative_to_receiver.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Aligned,
    Indented,
    IndentedRelativeToReceiver,
}

pub struct MultilineMethodCallIndentation {
    style: Style,
    indentation_width: usize,
}

impl MultilineMethodCallIndentation {
    pub fn new(style: Style, width: Option<usize>) -> Self {
        Self {
            style,
            indentation_width: width.unwrap_or(2),
        }
    }
}

impl Cop for MultilineMethodCallIndentation {
    fn name(&self) -> &'static str {
        "Layout/MultilineMethodCallIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = MultilineVisitor {
            ctx,
            style: self.style,
            indentation_width: self.indentation_width,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

// Alignment base: byte range in source that the RHS should align with.
struct AlignBase {
    offset: usize,
    end_offset: usize,
}

struct MultilineVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: Style,
    indentation_width: usize,
    offenses: Vec<Offense>,
}

// ============================================================================
// Source position helpers
// ============================================================================
impl<'a> MultilineVisitor<'a> {
    fn src(&self) -> &[u8] {
        self.ctx.source.as_bytes()
    }

    fn source_str(&self) -> &str {
        self.ctx.source
    }

    fn line_of(&self, offset: usize) -> usize {
        self.src()[..offset].iter().filter(|&&b| b == b'\n').count() + 1
    }

    fn col_of(&self, offset: usize) -> usize {
        let mut i = offset;
        while i > 0 && self.src()[i - 1] != b'\n' {
            i -= 1;
        }
        offset - i
    }

    fn line_start(&self, offset: usize) -> usize {
        let mut i = offset;
        while i > 0 && self.src()[i - 1] != b'\n' {
            i -= 1;
        }
        i
    }

    fn begins_its_line(&self, offset: usize) -> bool {
        let s = self.src();
        let mut i = offset;
        while i > 0 {
            i -= 1;
            if s[i] == b'\n' {
                return true;
            }
            if s[i] != b' ' && s[i] != b'\t' {
                return false;
            }
        }
        true
    }

    fn indentation_of(&self, offset: usize) -> usize {
        let ls = self.line_start(offset);
        let s = self.src();
        let mut i = ls;
        while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
            i += 1;
        }
        i - ls
    }

    fn line_text(&self, ls: usize) -> &str {
        let src = self.source_str();
        let end = src[ls..].find('\n').map(|p| ls + p).unwrap_or(src.len());
        &src[ls..end]
    }

    /// Find the indentation of the outermost statement containing a given offset.
    /// This walks backwards through continuation lines (lines ending with . or &.)
    /// to find the true statement start.
    fn statement_indentation(&self, offset: usize) -> usize {
        let mut ls = self.line_start(offset);
        loop {
            if ls == 0 {
                return self.indentation_of(ls);
            }
            // Check the previous line
            let prev_ls = self.line_start(ls - 1);
            let prev_line = self.line_text(prev_ls);
            let prev_trimmed = prev_line.trim_end();
            // Check if previous line ends with . or &. (continuation)
            if prev_trimmed.ends_with('.') || prev_trimmed.ends_with("&.") {
                ls = prev_ls;
                continue;
            }
            // Check if our line starts with . or &. (leading dot continuation)
            let cur_line = self.line_text(ls);
            let cur_trimmed = cur_line.trim_start();
            if cur_trimmed.starts_with('.') || cur_trimmed.starts_with("&.") {
                ls = prev_ls;
                continue;
            }
            return self.indentation_of(ls);
        }
    }

    fn text(&self, start: usize, end: usize) -> &str {
        &self.source_str()[start..end]
    }

    /// Get the text on the first line starting from `start` up to `end`.
    /// This is used for alignment base display in messages.
    fn first_line_text(&self, start: usize, end: usize) -> &str {
        let s = self.text(start, end);
        s.lines().next().unwrap_or(s)
    }

    /// Get the end-of-line offset from a given start position (up to newline or end of source).
    fn end_of_line(&self, offset: usize) -> usize {
        let s = self.src();
        let mut i = offset;
        while i < s.len() && s[i] != b'\n' {
            i += 1;
        }
        i
    }
}

// ============================================================================
// Core check
// ============================================================================
impl<'a> MultilineVisitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        // Must have a receiver
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        // Skip [] and []=
        let name = String::from_utf8_lossy(node.name().as_slice());
        if name.as_ref() == "[]" || name.as_ref() == "[]=" {
            return;
        }

        // Must have a dot
        let dot = match node.call_operator_loc() {
            Some(d) => d,
            None => return,
        };

        let dot_off = dot.start_offset();
        let dot_end = dot.end_offset();
        let sel_loc = node.message_loc();

        // Compute RHS (the thing we check indentation of)
        let (rhs_off, rhs_end) = if let Some(ref sel) = sel_loc {
            if self.line_of(dot_off) == self.line_of(sel.start_offset()) {
                (dot_off, sel.end_offset())
            } else {
                (sel.start_offset(), sel.end_offset())
            }
        } else {
            // Implicit call: .(args) or &.(args)
            if let Some(open) = node.opening_loc() {
                (dot_off, open.end_offset())
            } else {
                (dot_off, dot_end)
            }
        };

        // RHS must begin its line
        if !self.begins_its_line(rhs_off) {
            return;
        }

        // Must be multiline (receiver and RHS on different lines)
        let recv_start = receiver.location().start_offset();
        if self.line_of(recv_start) == self.line_of(rhs_off) {
            return;
        }

        // Walk up chain to find LHS
        let lhs_off = walk_up_chain(&receiver);

        // Check for pair ancestor (hash pair context)
        let pair_ancestor = find_pair_ancestor(node, self);

        // Hash pair with aligned style: use hash pair indentation
        if pair_ancestor.is_some() && self.style == Style::Aligned {
            if is_base_hash(&receiver) {
                self.check_hash_pair_indentation(node, &receiver, lhs_off, rhs_off, rhs_end);
            } else {
                // Non-hash base receiver in hash pair - use semantic alignment
                // but skip the argument_in_parenthesized_call filter
                self.check_hash_pair_non_hash_aligned(node, &receiver, lhs_off, rhs_off, rhs_end);
            }
            return;
        }

        // Hash pair with indented style and hash-type base receiver
        if pair_ancestor.is_some() && self.style == Style::Indented {
            if is_base_hash(&receiver) {
                self.check_hash_pair_indented_style(node, &receiver, lhs_off, rhs_off, rhs_end, pair_ancestor.unwrap());
                return;
            }
        }

        // Skip if inside grouped expression or arg list parentheses (only when no pair ancestor)
        if pair_ancestor.is_none() && self.not_for_this_cop(node, &receiver) {
            return;
        }

        let rhs_col = self.col_of(rhs_off);

        match self.style {
            Style::Aligned => self.check_aligned(node, &receiver, lhs_off, rhs_off, rhs_end, rhs_col),
            Style::Indented => self.check_indented(node, &receiver, lhs_off, rhs_off, rhs_end, rhs_col),
            Style::IndentedRelativeToReceiver => {
                self.check_relative(node, &receiver, lhs_off, rhs_off, rhs_end, rhs_col)
            }
        }
    }

    fn not_for_this_cop(&self, node: &ruby_prism::CallNode, receiver: &Node) -> bool {
        // Check if the call's dot is inside a grouped expression (begin/paren node)
        // or inside argument list parentheses.
        // Key: the dot must be ENCLOSED by the parens, not just following a paren expression.
        self.is_inside_grouped_expression(node) || self.is_inside_arg_list_parens(node)
    }

    /// Check if the call node's dot is inside a parenthesized grouped expression like (a.b)
    fn is_inside_grouped_expression(&self, node: &ruby_prism::CallNode) -> bool {
        let dot = match node.call_operator_loc() {
            Some(d) => d,
            None => return false,
        };
        let dot_off = dot.start_offset();
        let node_end = node.location().end_offset();

        // Scan backwards for an unmatched open paren
        let s = self.src();
        let mut depth = 0i32;
        let mut i = dot_off;
        while i > 0 {
            i -= 1;
            match s[i] {
                b')' => depth += 1,
                b'(' => {
                    if depth > 0 {
                        depth -= 1;
                    } else {
                        // Found unmatched open paren. Check if it's a grouped expression
                        // (not a method call paren).
                        if i > 0 && (s[i-1].is_ascii_alphanumeric() || s[i-1] == b'_'
                            || s[i-1] == b'!' || s[i-1] == b'?') {
                            return false; // method call paren
                        }
                        // It's a grouping paren. Now verify the closing paren
                        // is AFTER our node (i.e., we're truly inside it).
                        // Find the matching close paren.
                        let mut close_depth = 1i32;
                        let mut j = i + 1;
                        while j < s.len() && close_depth > 0 {
                            match s[j] {
                                b'(' => close_depth += 1,
                                b')' => close_depth -= 1,
                                _ => {}
                            }
                            j += 1;
                        }
                        // j-1 is the position of the closing paren
                        let close_pos = j - 1;
                        // The dot must be inside the parens
                        if close_pos > dot_off {
                            return true;
                        }
                        return false;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Check if the call node is inside parenthesized method argument list
    fn is_inside_arg_list_parens(&self, node: &ruby_prism::CallNode) -> bool {
        let dot = match node.call_operator_loc() {
            Some(d) => d,
            None => return false,
        };
        let dot_off = dot.start_offset();

        // Walk backwards from dot looking for an unmatched open paren
        // that's part of a method call
        let s = self.src();
        let mut depth = 0i32;
        let mut i = dot_off;
        while i > 0 {
            i -= 1;
            match s[i] {
                b')' => depth += 1,
                b'(' => {
                    if depth > 0 {
                        depth -= 1;
                    } else {
                        // Found unmatched open paren. Check if it's a method call paren.
                        if i > 0 && (s[i-1].is_ascii_alphanumeric() || s[i-1] == b'_'
                            || s[i-1] == b'!' || s[i-1] == b'?') {
                            // Verify the closing paren is after our dot
                            let mut close_depth = 1i32;
                            let mut j = i + 1;
                            while j < s.len() && close_depth > 0 {
                                match s[j] {
                                    b'(' => close_depth += 1,
                                    b')' => close_depth -= 1,
                                    _ => {}
                                }
                                j += 1;
                            }
                            let close_pos = j - 1;
                            if close_pos > dot_off {
                                return true; // method call paren enclosing our dot
                            }
                        }
                        return false;
                    }
                }
                _ => {}
            }
        }
        false
    }

    // ========================================================================
    // Hash pair indentation (aligned style)
    // ========================================================================
    fn check_hash_pair_indentation(
        &mut self,
        node: &ruby_prism::CallNode,
        receiver: &Node,
        lhs_off: usize,
        rhs_off: usize,
        rhs_end: usize,
    ) {
        let rhs_col = self.col_of(rhs_off);

        // Check if aligned with first line dot (for hash-type base receiver)
        if let Some(base) = self.find_hash_pair_alignment_base(node, receiver) {
            // Check aligned_with_first_line_dot
            if self.aligned_with_first_line_dot(node, receiver, rhs_off, rhs_col) {
                return;
            }
            let bc = self.col_of(base.offset);
            if rhs_col == bc {
                return;
            }
            let msg = format!(
                "Align `{}` with `{}` on line {}.",
                self.text(rhs_off, rhs_end),
                self.first_line_text(base.offset, base.end_offset),
                self.line_of(base.offset),
            );
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        // For non-hash base receiver in hash pair, align with receiver
        // First try the receiver chain alignment
        if let Some(base) = self.hash_pair_receiver_base(node, receiver) {
            // Check aligned_with_first_line_dot
            if self.aligned_with_first_line_dot(node, receiver, rhs_off, rhs_col) {
                return;
            }
            let bc = self.col_of(base.offset);
            if rhs_col == bc {
                return;
            }
            let msg = format!(
                "Align `{}` with `{}` on line {}.",
                self.text(rhs_off, rhs_end),
                self.first_line_text(base.offset, base.end_offset),
                self.line_of(base.offset),
            );
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        // Fallback: use regular aligned check
        self.check_aligned(node, receiver, lhs_off, rhs_off, rhs_end, rhs_col);
    }

    /// Check alignment for non-hash base receiver in hash pair context (aligned style).
    /// In RuboCop, hash pair check uses `lhs.source_range` (the top of the chain) as base.
    /// The base column is the start of the whole receiver chain.
    fn check_hash_pair_non_hash_aligned(
        &mut self,
        node: &ruby_prism::CallNode,
        receiver: &Node,
        lhs_off: usize,
        rhs_off: usize,
        rhs_end: usize,
    ) {
        let rhs_col = self.col_of(rhs_off);

        // For hash pair with do-end block receiver, use the first call's text as base
        if let Some(base) = self.hash_pair_doend_block_base(node, receiver) {
            let bc = self.col_of(base.offset);
            if rhs_col == bc {
                return;
            }
            let msg = format!(
                "Align `{}` with `{}` on line {}.",
                self.text(rhs_off, rhs_end),
                self.first_line_text(base.offset, base.end_offset),
                self.line_of(base.offset),
            );
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        // The alignment base is the start of the whole receiver chain
        // This corresponds to RuboCop's `lhs.source_range` (the top call node's start)
        // which starts at the base receiver (e.g., `Foo` in `Foo.bar`)
        let chain_start = walk_up_chain(receiver);
        let chain_col = self.col_of(chain_start);

        // Check aligned_with_first_line_dot
        if self.aligned_with_first_line_dot(node, receiver, rhs_off, rhs_col) {
            return;
        }

        // For the message, use the receiver text (e.g., "Foo.bar" or "value.foo.bar")
        let recv_start = receiver.location().start_offset();
        let recv_end = receiver.location().end_offset();
        // Get the first line of receiver (may span multiple lines)
        let recv_first_line_end = self.end_of_line(recv_start);
        // Include trailing dot for trailing-dot style
        let s = self.src();
        let text_end = if recv_first_line_end < s.len() && s[recv_first_line_end - 1] == b'.' {
            recv_first_line_end
        } else if recv_first_line_end <= recv_end {
            recv_first_line_end
        } else {
            recv_end
        };

        if rhs_col == chain_col {
            return;
        }
        let msg = format!(
            "Align `{}` with `{}` on line {}.",
            self.text(rhs_off, rhs_end),
            self.first_line_text(recv_start, text_end),
            self.line_of(recv_start),
        );
        self.offense(rhs_off, rhs_end, &msg, chain_col as isize - rhs_col as isize);
    }

    /// Find alignment base for do-end block chains in hash pair context.
    /// E.g., `Foo.bar do |x| x end.baz .qux`
    fn hash_pair_doend_block_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        // Walk up the receiver chain looking for a do-end block
        if let Node::CallNode { .. } = receiver {
            let recv_call = receiver.as_call_node().unwrap();
            if let Some(recv_recv) = recv_call.receiver() {
                // Check if receiver's receiver is a call with a do-end block
                if let Node::CallNode { .. } = recv_recv {
                    let inner = recv_recv.as_call_node().unwrap();
                    if let Some(block) = inner.block() {
                        if !is_single_line_node(&block, self) {
                            // Multiline (do-end) block. The alignment base is the first call text.
                            let inner_recv = inner.receiver();
                            if let Some(ir) = inner_recv {
                                let first_start = walk_up_chain(&ir);
                                let eol = self.end_of_line(first_start);
                                return Some(AlignBase { offset: first_start, end_offset: eol });
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Find alignment base for hash pair context - when the base receiver is a hash
    fn find_hash_pair_alignment_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        if !is_base_hash(receiver) {
            return None;
        }
        let (dot, sel_end, _, _) = first_call_dot(receiver)?;
        Some(AlignBase { offset: dot, end_offset: sel_end })
    }

    /// Find alignment base for hash pair with non-hash base receiver
    fn hash_pair_receiver_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        // Walk to find the base (top) receiver of the chain
        let (base_start, base_end, _, _, _) = find_base_receiver_info(receiver);

        // For block chain: get dot.selector of the first call with a block
        if let Some(block_base) = self.find_block_chain_base_for_hash_pair(node, receiver) {
            return Some(block_base);
        }

        // Return the base receiver's range
        Some(AlignBase { offset: base_start, end_offset: base_end })
    }

    fn find_block_chain_base_for_hash_pair(&self, _node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        // If receiver is a call whose receiver has a block, use dot.selector of receiver
        if matches!(receiver, Node::CallNode { .. }) {
            let recv_call = receiver.as_call_node().unwrap();
            if let Some(recv_recv) = recv_call.receiver() {
                if matches!(recv_recv, Node::CallNode { .. }) {
                    let inner = recv_recv.as_call_node().unwrap();
                    if inner.block().is_some() {
                        return dot_sel_base(&recv_call, self);
                    }
                }
            }
        }
        None
    }

    fn aligned_with_first_line_dot(&self, node: &ruby_prism::CallNode, receiver: &Node, rhs_off: usize, rhs_col: usize) -> bool {
        let b = *self.src().get(rhs_off).unwrap_or(&0);
        if b != b'.' && b != b'&' {
            return false;
        }
        let first = match first_call_dot_for_alignment(node, receiver, self) {
            Some(f) => f,
            None => return false,
        };
        let dot_col = self.col_of(first.0);
        let dot_line = self.line_of(first.0);
        let node_dot = node.call_operator_loc().map(|d| d.start_offset()).unwrap_or(0);
        if first.0 == node_dot {
            return false;
        }
        // In RuboCop: return false if first_call == node.receiver
        // If the first call's dot belongs to the direct receiver, skip.
        if let Node::CallNode { .. } = receiver {
            let recv_call = receiver.as_call_node().unwrap();
            if let Some(recv_dot) = recv_call.call_operator_loc() {
                if recv_dot.start_offset() == first.0 {
                    return false;
                }
            }
        }
        dot_line == self.line_of(receiver.location().start_offset()) && dot_col == rhs_col
    }

    // ========================================================================
    // Hash pair indented style (for hash-type base receiver)
    // ========================================================================
    fn check_hash_pair_indented_style(
        &mut self,
        _node: &ruby_prism::CallNode,
        _receiver: &Node,
        _lhs_off: usize,
        rhs_off: usize,
        rhs_end: usize,
        pair_key_off: usize,
    ) {
        let rhs_col = self.col_of(rhs_off);
        let pair_key_col = self.col_of(pair_key_off);
        let double_indent = self.indentation_width * 2;
        let correct_col = pair_key_col + double_indent;
        let hash_pair_base_col = pair_key_col + self.indentation_width;

        if rhs_col == correct_col {
            return;
        }

        let used = rhs_col as isize - hash_pair_base_col as isize;
        let msg = format!(
            "Use {} (not {}) spaces for indenting an expression spanning multiple lines.",
            self.indentation_width, used,
        );
        self.offense(rhs_off, rhs_end, &msg, correct_col as isize - rhs_col as isize);
    }

    // ========================================================================
    // Aligned
    // ========================================================================
    fn check_aligned(
        &mut self,
        node: &ruby_prism::CallNode,
        receiver: &Node,
        lhs_off: usize,
        rhs_off: usize,
        rhs_end: usize,
        rhs_col: usize,
    ) {
        // 1. Semantic alignment
        if let Some(base) = self.semantic_base(node, receiver, rhs_off) {
            let bc = self.col_of(base.offset);
            if rhs_col == bc {
                return;
            }
            let msg = format!(
                "Align `{}` with `{}` on line {}.",
                self.text(rhs_off, rhs_end),
                self.first_line_text(base.offset, base.end_offset),
                self.line_of(base.offset),
            );
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        // 2. Syntactic alignment (keyword, assignment, operator)
        if let Some(base) = self.syntactic_base(node, receiver, lhs_off, rhs_off) {
            let bc = self.col_of(base.offset);
            if rhs_col == bc {
                return;
            }
            let msg = format!(
                "Align `{}` with `{}` on line {}.",
                self.text(rhs_off, rhs_end),
                self.first_line_text(base.offset, base.end_offset),
                self.line_of(base.offset),
            );
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        // 3. No base found - use indentation
        let li = self.statement_indentation(lhs_off);
        let expected = li + self.indentation_width;
        if rhs_col == expected {
            return;
        }
        let used = rhs_col as isize - li as isize;
        let what = self.op_desc(node, lhs_off, rhs_off);
        let msg = format!(
            "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
            self.indentation_width, used, what,
        );
        self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
    }

    // ========================================================================
    // Indented
    // ========================================================================
    fn check_indented(
        &mut self,
        node: &ruby_prism::CallNode,
        _receiver: &Node,
        lhs_off: usize,
        rhs_off: usize,
        rhs_end: usize,
        rhs_col: usize,
    ) {
        // Keyword special indentation
        if let Some(kw) = self.find_keyword(lhs_off) {
            let bi = self.indentation_of(kw.0);
            let expected = bi + self.indentation_width + 2; // cop_width + normal_width
            if rhs_col == expected {
                return;
            }
            let what = kw_message_tail(&kw.1);
            let msg = format!(
                "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
                expected, rhs_col, what,
            );
            self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
            return;
        }

        // Assignment context
        if let Some(assign_off) = self.find_assign(lhs_off) {
            let bi = self.indentation_of(assign_off);
            let expected = bi + self.indentation_width;
            if rhs_col == expected {
                return;
            }
            let used = rhs_col as isize - bi as isize;
            let msg = format!(
                "Use {} (not {}) spaces for indenting an expression in an assignment spanning multiple lines.",
                self.indentation_width, used,
            );
            self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
            return;
        }

        // Normal
        let li = self.statement_indentation(lhs_off);
        let expected = li + self.indentation_width;
        if rhs_col == expected {
            return;
        }
        let used = rhs_col as isize - li as isize;
        let what = self.op_desc(node, lhs_off, rhs_off);
        let msg = format!(
            "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
            self.indentation_width, used, what,
        );
        self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
    }

    // ========================================================================
    // Indented relative to receiver
    // ========================================================================
    fn check_relative(
        &mut self,
        node: &ruby_prism::CallNode,
        receiver: &Node,
        lhs_off: usize,
        rhs_off: usize,
        rhs_end: usize,
        rhs_col: usize,
    ) {
        if let Some(base) = self.receiver_base(node, receiver) {
            let bc = self.col_of(base.offset);
            // Compute extra indentation, accounting for splat/kwsplat prefix
            let extra = self.extra_indentation_for_relative(receiver);
            let expected = bc + extra;
            if rhs_col == expected {
                return;
            }
            let msg = format!(
                "Indent `{}` {} spaces more than `{}` on line {}.",
                self.text(rhs_off, rhs_end),
                self.indentation_width,
                self.first_line_text(base.offset, base.end_offset),
                self.line_of(base.offset),
            );
            self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
            return;
        }

        // Fallback
        let li = self.indentation_of(lhs_off);
        let expected = li + self.indentation_width;
        if rhs_col == expected {
            return;
        }
        let recv_start = receiver.location().start_offset();
        let recv_end = receiver.location().end_offset();
        let msg = format!(
            "Indent `{}` {} spaces more than `{}` on line {}.",
            self.text(rhs_off, rhs_end),
            self.indentation_width,
            self.first_line_text(recv_start, recv_end),
            self.line_of(recv_start),
        );
        self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
    }

    /// Compute extra indentation for indented_relative_to_receiver style.
    /// Normally this is indentation_width (2), but for splat/kwsplat contexts
    /// it's reduced by the operator length.
    fn extra_indentation_for_relative(&self, receiver: &Node) -> usize {
        let top_off = walk_up_chain(receiver);
        let s = self.src();
        // Check if preceded by * or ** (splat/kwsplat)
        if top_off > 0 && s[top_off - 1] == b'*' {
            if top_off > 1 && s[top_off - 2] == b'*' {
                // **kwsplat: reduce by 2
                self.indentation_width.saturating_sub(2)
            } else {
                // *splat: reduce by 1
                self.indentation_width.saturating_sub(1)
            }
        } else {
            self.indentation_width
        }
    }

    // ========================================================================
    // Semantic alignment base
    // ========================================================================
    fn semantic_base(&self, node: &ruby_prism::CallNode, receiver: &Node, rhs_off: usize) -> Option<AlignBase> {
        // Only if RHS starts with . or &.
        let b = *self.src().get(rhs_off)?;
        if b != b'.' && b != b'&' {
            return None;
        }

        // Skip if inside argument list parentheses (for semantic alignment)
        if self.is_argument_in_parenthesized_call(node) {
            return None;
        }

        // Dot right above
        if let Some(base) = self.dot_right_above(node, receiver) {
            return Some(base);
        }

        // Multiline block chain
        if let Some(base) = self.block_chain_base(node, receiver) {
            return Some(base);
        }

        // First call alignment
        self.first_call_base(node, receiver)
    }

    /// Check if node is an argument in a parenthesized method call
    fn is_argument_in_parenthesized_call(&self, node: &ruby_prism::CallNode) -> bool {
        // We need to check if this call is within the arguments of another call
        // that uses parentheses. This is tricky without parent pointers.
        // Use source scanning: check if there's an unmatched '(' before the LHS
        // that belongs to a method call.
        let recv = match node.receiver() {
            Some(r) => r,
            None => return false,
        };
        // Walk to top of chain
        let top_off = walk_up_chain(&recv);

        let s = self.src();
        let mut depth = 0i32;
        let mut i = top_off;
        while i > 0 {
            i -= 1;
            match s[i] {
                b')' => depth += 1,
                b'(' => {
                    if depth > 0 {
                        depth -= 1;
                    } else {
                        // Found unmatched open paren. Check if it's preceded by identifier
                        // (method call paren)
                        if i > 0 && (s[i-1].is_ascii_alphanumeric() || s[i-1] == b'_' || s[i-1] == b'!' || s[i-1] == b'?') {
                            return true;
                        }
                        return false;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn dot_right_above(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        let dot = node.call_operator_loc()?;
        let dot_off = dot.start_offset();
        let dot_col = self.col_of(dot_off);
        let dot_line = self.line_of(dot_off);

        // First try AST-based search through receiver chain
        if let Some(result) = search_dot_above(receiver, dot_line, dot_col, self) {
            // Validate: if the matched dot is on the same line as the end of a
            // parenthesized base receiver, the column match is coincidental.
            let (_, base_end, _, _, is_paren) = find_base_receiver_info(receiver);
            if is_paren && self.line_of(result.offset) == self.line_of(base_end) {
                return None;
            }
            return Some(result);
        }

        // Fallback: source-based search for a dot at the same column on the line above.
        // This handles cases where the dot is from an ancestor (parent) node rather than
        // the receiver chain (e.g., .and as argument to .to in RSpec code).
        if dot_line <= 1 {
            return None;
        }
        let prev_line_start = self.line_start(dot_off);
        if prev_line_start == 0 {
            return None;
        }
        let prev_ls = self.line_start(prev_line_start - 1);
        let prev_line = self.line_text(prev_ls);
        if dot_col < prev_line.len() {
            let ch = prev_line.as_bytes()[dot_col];
            if ch == b'.' || (ch == b'&' && dot_col + 1 < prev_line.len() && prev_line.as_bytes()[dot_col + 1] == b'.') {
                // Found a dot at the same column on the previous line
                let found_off = prev_ls + dot_col;
                // Find the end of the method name after the dot
                let s = self.src();
                let mut end = found_off + 1;
                if ch == b'&' { end += 1; } // skip &.
                // Skip the method name
                while end < s.len() && (s[end].is_ascii_alphanumeric() || s[end] == b'_' || s[end] == b'!' || s[end] == b'?') {
                    end += 1;
                }
                if end > found_off + 1 + (if ch == b'&' { 1 } else { 0 }) {
                    // Also include the rest of the line content for message purposes
                    let eol = self.end_of_line(found_off);
                    return Some(AlignBase { offset: found_off, end_offset: eol });
                }
            }
        }

        None
    }

    fn block_chain_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        // Check if the current node has a block_node (this handles the RSpec pattern)
        if node.block().is_some() {
            return self.find_continuation_node(node, receiver);
        }

        // Check for descendant blocks
        self.handle_descendant_block(node, receiver)
    }

    fn find_continuation_node(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        // If receiver is a single-line block, use its send_node's dot.selector
        if matches!(receiver, Node::CallNode { .. }) {
            let recv_call = receiver.as_call_node().unwrap();

            // Check if receiver has a single-line block
            if recv_call.block().is_some() && is_single_line(receiver, self) {
                return dot_sel_base(&recv_call, self);
            }

            // Check if receiver is a call with a dot whose receiver is a begin node
            // and the current node has a single-line block
            if let Some(recv_recv) = recv_call.receiver() {
                if matches!(recv_recv, Node::ParenthesesNode { .. }) && node.block().is_some() {
                    if let Some(block) = node.block() {
                        if is_single_line_node(&block, self) {
                            return dot_sel_base(&recv_call, self);
                        }
                    }
                }
            }

            // If receiver is a call whose dot is on a different line than receiver.receiver's last line
            if let Some(recv_dot) = recv_call.call_operator_loc() {
                if let Some(recv_recv) = recv_call.receiver() {
                    let recv_recv_end_line = self.line_of(recv_recv.location().end_offset());
                    let recv_dot_line = self.line_of(recv_dot.start_offset());
                    if recv_dot_line > recv_recv_end_line {
                        // But skip if the base receiver is a multiline paren expression
                        // and the dot is on the same line as the paren end - coincidental alignment
                        let (_, base_end, _, _, is_paren) = find_base_receiver_info(receiver);
                        if is_paren {
                            let base_end_line = self.line_of(base_end);
                            // If the first call's dot is on the same line as the paren's close,
                            // don't use this as alignment base
                            if let Some((first_dot, _, _, _)) = first_call_dot(receiver) {
                                if self.line_of(first_dot) == base_end_line {
                                    return None;
                                }
                            }
                        }
                        return dot_sel_base(&recv_call, self);
                    }
                }
            }
        }

        None
    }

    fn handle_descendant_block(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        // If receiver has a single-line block
        if matches!(receiver, Node::CallNode { .. }) {
            let recv_call = receiver.as_call_node().unwrap();
            if recv_call.block().is_some() && is_single_line(receiver, self) {
                return dot_sel_base(&recv_call, self);
            }
        }

        // Check if receiver's chain contains a multiline block
        if matches!(receiver, Node::CallNode { .. }) {
            let recv_call = receiver.as_call_node().unwrap();

            // Check if receiver itself has a multiline block
            if let Some(block) = recv_call.block() {
                if !is_single_line_node(&block, self) {
                    // The receiver has a multiline block
                    if recv_call.call_operator_loc().is_some() {
                        return dot_sel_base(&recv_call, self);
                    }
                }
            }

            // Check if receiver's receiver has a multiline block
            if let Some(recv_recv) = recv_call.receiver() {
                if matches!(recv_recv, Node::CallNode { .. }) {
                    let inner = recv_recv.as_call_node().unwrap();
                    if let Some(block) = inner.block() {
                        if !is_single_line_node(&block, self) {
                            return dot_sel_base(&recv_call, self);
                        }
                    }
                }
            }
        }

        None
    }

    fn first_call_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        let (first_dot, first_sel_end, rs, re) = first_call_dot(receiver)?;

        // Skip if the first call has no selector (implicit call like MyClass.(args))
        // The first call must have both dot AND selector for semantic alignment
        if !has_selector_at_first_call(receiver) {
            return None;
        }

        let dot_line = self.line_of(first_dot);
        let rs_line = self.line_of(rs);
        let re_line = self.line_of(re);

        // Check for method_on_receiver_last_line with :array type
        let (base_start, base_end, _is_hash, is_array, is_paren) = find_base_receiver_info(receiver);
        if is_array {
            if dot_line == re_line {
                // method_on_receiver_last_line is true for array
                // Return the first call's dot.selector
                let node_dot = node.call_operator_loc()?.start_offset();
                if first_dot == node_dot {
                    return None;
                }
                return Some(AlignBase { offset: first_dot, end_offset: first_sel_end });
            }
        }

        // Skip if method is on last line of multiline parenthesized receiver (begin type)
        if is_paren {
            if dot_line == self.line_of(base_end) {
                return None;
            }
        }

        // dot must be on first line of receiver
        if dot_line != rs_line {
            return None;
        }

        // Not the same dot as current node
        let node_dot = node.call_operator_loc()?.start_offset();
        if first_dot == node_dot {
            return None;
        }

        Some(AlignBase { offset: first_dot, end_offset: first_sel_end })
    }

    // ========================================================================
    // Syntactic / receiver bases
    // ========================================================================
    fn syntactic_base(&self, node: &ruby_prism::CallNode, receiver: &Node, lhs_off: usize, rhs_off: usize) -> Option<AlignBase> {
        // Keyword condition
        if let Some(kw) = self.find_keyword(lhs_off) {
            let kw_end = kw.0 + kw.1.len();
            let s = self.src();
            let mut expr_start = kw_end;
            while expr_start < s.len() && s[expr_start] == b' ' {
                expr_start += 1;
            }
            // Get the end of this expression on the first line
            let expr_eol = self.end_of_line(expr_start);
            return Some(AlignBase { offset: expr_start, end_offset: expr_eol });
        }

        // Return keyword: align with expression after 'return'
        if let Some((_kw_off, expr_start)) = self.find_return_keyword(lhs_off) {
            let eol = self.end_of_line(expr_start);
            return Some(AlignBase { offset: expr_start, end_offset: eol });
        }

        // Assignment (same line)
        if let Some(_assign_off) = self.find_assign(lhs_off) {
            // Get the RHS of the assignment (after = and whitespace)
            let ls = self.line_start(lhs_off);
            let line = self.line_text(ls);
            let before = &line[..(lhs_off - ls).min(line.len())];
            if let Some(eq_pos) = before.rfind('=') {
                let abs_eq_pos = ls + eq_pos;
                // Skip whitespace after =
                let s = self.src();
                let mut rhs_start = abs_eq_pos + 1;
                while rhs_start < s.len() && s[rhs_start] == b' ' {
                    rhs_start += 1;
                }
                // Get end of line from rhs_start for full base text
                let eol = self.end_of_line(rhs_start);
                return Some(AlignBase { offset: rhs_start, end_offset: eol });
            }
            return Some(AlignBase { offset: lhs_off, end_offset: self.end_of_line(lhs_off) });
        }

        // Assignment or return on previous line
        if let Some((_prev_ls, expr_start)) = self.find_assign_above(lhs_off) {
            // The alignment base is the expression start (e.g., the value after = or after return)
            // But we want to align with the lhs_off (the receiver), not the assignment
            return Some(AlignBase { offset: lhs_off, end_offset: self.end_of_line(lhs_off) });
        }

        // Operator
        self.find_operator(lhs_off)
    }

    fn receiver_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        // Check for hash method base
        if let Some(base) = self.hash_chain_base(receiver) {
            return Some(base);
        }

        // Default: first call's receiver
        if let Some((_, _, rs, re)) = first_call_dot(receiver) {
            return Some(AlignBase { offset: rs, end_offset: re });
        }

        // If receiver has no dot (e.g., simple variable `a`), use the receiver itself
        // This handles cases like `1 + a\n .b` where `a` should be the alignment base
        let rs = receiver.location().start_offset();
        let re = receiver.location().end_offset();
        Some(AlignBase { offset: rs, end_offset: re })
    }

    fn hash_chain_base(&self, receiver: &Node) -> Option<AlignBase> {
        if !matches!(receiver, Node::CallNode { .. }) {
            return None;
        }
        let mut call = receiver.as_call_node().unwrap();
        loop {
            if let Some(base_recv) = call.receiver() {
                if matches!(base_recv, Node::HashNode { .. })
                    || is_method_on_paren_end_line(&call, &base_recv, self)
                {
                    return dot_sel_base(&call, self);
                }
                if matches!(base_recv, Node::CallNode { .. }) {
                    call = base_recv.as_call_node().unwrap();
                    continue;
                }
            }
            break;
        }
        None
    }

    // ========================================================================
    // Keyword / assignment / operator helpers (source-based)
    // ========================================================================
    fn find_keyword(&self, lhs_off: usize) -> Option<(usize, String)> {
        let ls = self.line_start(lhs_off);
        if self.line_of(lhs_off) != self.line_of(ls) {
            return None;
        }
        let line = self.line_text(ls);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        for kw in &["if ", "unless ", "while ", "until "] {
            if trimmed.starts_with(kw) {
                let kw_name = kw.trim();
                let kw_offset = ls + indent;
                return Some((kw_offset, kw_name.to_string()));
            }
        }

        if trimmed.starts_with("for ") {
            return Some((ls + indent, "for".to_string()));
        }

        None
    }

    /// Find 'return' keyword on the same line as lhs_off.
    /// Returns (keyword_offset, expression_start_offset) if found.
    fn find_return_keyword(&self, lhs_off: usize) -> Option<(usize, usize)> {
        let ls = self.line_start(lhs_off);
        let line = self.line_text(ls);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if trimmed.starts_with("return ") {
            let kw_off = ls + indent;
            let expr_start_off = kw_off + 7; // "return ".len()
            let s = self.src();
            let mut start = expr_start_off;
            while start < s.len() && s[start] == b' ' {
                start += 1;
            }
            return Some((kw_off, start));
        }
        None
    }

    fn find_assign(&self, lhs_off: usize) -> Option<usize> {
        let ls = self.line_start(lhs_off);
        let line = self.line_text(ls);
        let before = &line[..(lhs_off - ls).min(line.len())];

        if let Some(eq_pos) = before.rfind('=') {
            if eq_pos > 0 {
                let prev = before.as_bytes()[eq_pos - 1];
                if prev == b'=' || prev == b'!' || prev == b'<' || prev == b'>' {
                    return None;
                }
            }
            return Some(ls);
        }
        None
    }

    /// Search for assignment context on lines above lhs_off.
    /// This handles cases like:
    ///   a +=
    ///     b
    ///     .c
    /// where lhs_off is at `b` but the assignment `+=` is on a previous line.
    fn find_assign_above(&self, lhs_off: usize) -> Option<(usize, usize)> {
        let ls = self.line_start(lhs_off);
        if ls == 0 {
            return None;
        }
        // Check the previous line(s)
        let mut search_off = ls;
        for _ in 0..5 {
            if search_off == 0 {
                break;
            }
            let prev_ls = self.line_start(search_off - 1);
            let prev_line = self.line_text(prev_ls);
            let prev_trimmed = prev_line.trim_end();

            // Check for trailing assignment operators
            if prev_trimmed.ends_with('=') && !prev_trimmed.ends_with("==")
                && !prev_trimmed.ends_with("!=") && !prev_trimmed.ends_with("<=")
                && !prev_trimmed.ends_with(">=") && !prev_trimmed.ends_with("=>") {
                return Some((prev_ls, lhs_off));
            }

            // Check for 'return expr' pattern
            let content = prev_trimmed.trim_start();
            if content.starts_with("return ") {
                let indent = prev_trimmed.len() - content.len();
                let kw_end = prev_ls + indent + 7; // "return ".len()
                // Find where the expression starts after 'return '
                let s = self.src();
                let mut expr_start = kw_end;
                while expr_start < s.len() && s[expr_start] == b' ' {
                    expr_start += 1;
                }
                return Some((prev_ls, expr_start));
            }

            search_off = prev_ls;
        }
        None
    }

    /// Check if the expression context includes an assignment on any enclosing line.
    /// Broader than find_assign_above - checks for inline assignments too.
    fn is_in_assignment_context(&self, lhs_off: usize) -> bool {
        // Check same line
        if self.find_assign(lhs_off).is_some() {
            return true;
        }
        // Check lines above (trailing = only)
        if self.find_assign_above(lhs_off).is_some() {
            return true;
        }
        // Check for inline assignment on previous lines (e.g., `a = b.call(`)
        let ls = self.line_start(lhs_off);
        let mut search_off = ls;
        for _ in 0..5 {
            if search_off == 0 {
                break;
            }
            let prev_ls = self.line_start(search_off - 1);
            let prev_line = self.line_text(prev_ls);
            let content = prev_line.trim();
            // Look for simple assignment pattern: `identifier = expr`
            // This is conservative - only matches clear assignment patterns
            for pat in &[" = ", " += ", " -= ", " *= ", " /= ", " ||= ", " &&= "] {
                if content.contains(pat) && !content.starts_with("if ") && !content.starts_with("unless ")
                    && !content.starts_with("while ") && !content.starts_with("until ") {
                    return true;
                }
            }
            search_off = prev_ls;
        }
        false
    }

    fn find_operator(&self, lhs_off: usize) -> Option<AlignBase> {
        let ls = self.line_start(lhs_off);
        let line = self.line_text(ls);
        let before = &line[..(lhs_off - ls).min(line.len())];
        let trimmed = before.trim_end();

        if trimmed.ends_with('+') || trimmed.ends_with("- ") || trimmed.ends_with('*')
            || trimmed.ends_with('/') || trimmed.ends_with('%') || trimmed.ends_with("<<")
        {
            return Some(AlignBase { offset: lhs_off, end_offset: self.end_of_line(lhs_off) });
        }
        None
    }

    fn op_desc(&self, _node: &ruby_prism::CallNode, lhs_off: usize, rhs_off: usize) -> String {
        if let Some(kw) = self.find_keyword(lhs_off) {
            return kw_message_tail(&kw.1);
        }
        if self.is_in_assignment_context(lhs_off) {
            return "an expression in an assignment".to_string();
        }
        "an expression".to_string()
    }

    // ========================================================================
    // Block/chain extra lines for correction
    // ========================================================================

    /// Collect byte offsets of lines that should also be shifted when correcting the
    /// offense line at `rhs_off`. This includes:
    /// 1. Block body lines and `end` line if the offense line contains `do`
    /// 2. Continuation chain lines (`.method`) immediately following at the same column
    fn collect_extra_correction_lines(&self, rhs_off: usize) -> Vec<usize> {
        let s = self.src();
        let ls = self.line_start(rhs_off);
        let eol = self.end_of_line(rhs_off);
        let line = &s[ls..eol];
        let line_str = std::str::from_utf8(line).unwrap_or("");
        let rhs_col = self.col_of(rhs_off);

        let mut extra = Vec::new();

        // Check if line contains a `do` block
        let has_do = line_str.contains(" do |")
            || line_str.contains(" do\n")
            || line_str.trim_end().ends_with(" do")
            || line_str.trim_end().ends_with(" do |");

        if has_do {
            // Find matching `end` by counting do/end nesting
            let mut depth = 1i32;
            let mut pos = eol;
            if pos < s.len() && s[pos] == b'\n' {
                pos += 1;
            }

            while pos < s.len() && depth > 0 {
                let line_start = pos;
                let line_end = self.end_of_line(pos);
                let ln = std::str::from_utf8(&s[line_start..line_end]).unwrap_or("");
                let trimmed = ln.trim();

                if trimmed == "end" || trimmed.starts_with("end ") || trimmed.starts_with("end.") {
                    depth -= 1;
                } else if trimmed.contains(" do |") || trimmed.ends_with(" do") {
                    depth += 1;
                }

                extra.push(line_start);

                pos = line_end;
                if pos < s.len() && s[pos] == b'\n' {
                    pos += 1;
                }
            }
        } else {
            // Check for continuation chain lines at the same column
            let mut pos = eol;
            if pos < s.len() && s[pos] == b'\n' {
                pos += 1;
            }

            while pos < s.len() {
                let line_start = pos;
                let line_end = self.end_of_line(pos);
                let ln = std::str::from_utf8(&s[line_start..line_end]).unwrap_or("");
                let trimmed = ln.trim();

                // Check if this line starts with . or &. at the same column as rhs
                let ln_col = ln.len() - ln.trim_start().len();
                if ln_col == rhs_col && (trimmed.starts_with('.') || trimmed.starts_with("&.")) {
                    extra.push(line_start);
                } else {
                    break;
                }

                pos = line_end;
                if pos < s.len() && s[pos] == b'\n' {
                    pos += 1;
                }
            }
        }

        extra
    }

    // ========================================================================
    // Offense
    // ========================================================================
    fn offense(&mut self, rhs_off: usize, rhs_end: usize, msg: &str, delta: isize) {
        let extra = self.collect_extra_correction_lines(rhs_off);
        self.offense_with_extra(rhs_off, rhs_end, msg, delta, &extra);
    }

    /// Create offense with correction. `extra_line_offsets` are byte offsets of
    /// additional lines (e.g. block body / end) whose leading whitespace should
    /// also be shifted by `delta`.
    fn offense_with_extra(
        &mut self,
        rhs_off: usize,
        rhs_end: usize,
        msg: &str,
        delta: isize,
        extra_line_offsets: &[usize],
    ) {
        let off = self.ctx.offense_with_range(
            "Layout/MultilineMethodCallIndentation",
            msg,
            Severity::Convention,
            rhs_off,
            rhs_end,
        );
        let ls = self.line_start(rhs_off);
        let cur = rhs_off - ls;
        let new = (cur as isize + delta).max(0) as usize;

        let mut edits = vec![Edit {
            start_offset: ls,
            end_offset: rhs_off,
            replacement: " ".repeat(new),
        }];

        // Add edits for extra lines (block body, end, chain continuation)
        let s = self.src();
        for &line_off in extra_line_offsets {
            let el_ls = self.line_start(line_off);
            // Find the first non-whitespace on this line
            let mut ws_end = el_ls;
            while ws_end < s.len() && (s[ws_end] == b' ' || s[ws_end] == b'\t') {
                ws_end += 1;
            }
            let el_cur = ws_end - el_ls;
            let el_new = (el_cur as isize + delta).max(0) as usize;
            edits.push(Edit {
                start_offset: el_ls,
                end_offset: ws_end,
                replacement: " ".repeat(el_new),
            });
        }

        let corr = Correction { edits };
        self.offenses.push(off.with_correction(corr));
    }
}

// ============================================================================
// Free functions for node traversal (avoid lifetime issues)
// ============================================================================

/// Walk up the method chain to the top receiver. Returns offset of the topmost node.
fn walk_up_chain(node: &Node) -> usize {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if call.call_operator_loc().is_some() && !is_assignment_method(&call) {
                if let Some(recv) = call.receiver() {
                    return walk_up_chain(&recv);
                }
            }
            node.location().start_offset()
        }
        _ => node.location().start_offset(),
    }
}

fn is_assignment_method(call: &ruby_prism::CallNode) -> bool {
    let name = String::from_utf8_lossy(call.name().as_slice());
    name.ends_with('=') && name.as_ref() != "==" && name.as_ref() != "!="
        && name.as_ref() != "<=" && name.as_ref() != ">="
}

fn is_base_hash(node: &Node) -> bool {
    base_receiver_matches(node, |n| matches!(n, Node::HashNode { .. }))
}

/// Walks to the base receiver of a call chain and applies `pred` to it.
fn base_receiver_matches(node: &Node, pred: impl Fn(&Node) -> bool) -> bool {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                base_receiver_matches(&recv, pred)
            } else {
                pred(node)
            }
        }
        _ => pred(node),
    }
}

/// Walk to the actual base receiver node. Returns (start_offset, end_offset, is_hash, is_array, is_paren).
fn find_base_receiver_info(node: &Node) -> (usize, usize, bool, bool, bool) {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                find_base_receiver_info(&recv)
            } else {
                let loc = node.location();
                (loc.start_offset(), loc.end_offset(), false, false, false)
            }
        }
        Node::HashNode { .. } => {
            let loc = node.location();
            (loc.start_offset(), loc.end_offset(), true, false, false)
        }
        Node::ArrayNode { .. } => {
            let loc = node.location();
            (loc.start_offset(), loc.end_offset(), false, true, false)
        }
        Node::ParenthesesNode { .. } => {
            let loc = node.location();
            (loc.start_offset(), loc.end_offset(), false, false, true)
        }
        _ => {
            let loc = node.location();
            (loc.start_offset(), loc.end_offset(), false, false, false)
        }
    }
}

fn is_single_line(node: &Node, vis: &MultilineVisitor) -> bool {
    vis.line_of(node.location().start_offset()) == vis.line_of(node.location().end_offset())
}

fn is_single_line_node(node: &Node, vis: &MultilineVisitor) -> bool {
    vis.line_of(node.location().start_offset()) == vis.line_of(node.location().end_offset())
}

fn is_method_on_paren_end_line(call: &ruby_prism::CallNode, base: &Node, vis: &MultilineVisitor) -> bool {
    if matches!(base, Node::ParenthesesNode { .. }) {
        if let Some(dot) = call.call_operator_loc() {
            return vis.line_of(dot.start_offset()) == vis.line_of(base.location().end_offset());
        }
    }
    false
}

fn dot_sel_base(call: &ruby_prism::CallNode, _vis: &MultilineVisitor) -> Option<AlignBase> {
    let dot = call.call_operator_loc()?;
    let end = call.message_loc().map(|s| s.end_offset()).unwrap_or(dot.end_offset());
    Some(AlignBase { offset: dot.start_offset(), end_offset: end })
}

fn search_dot_above(node: &Node, target_line: usize, target_col: usize, vis: &MultilineVisitor) -> Option<AlignBase> {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(d) = call.call_operator_loc() {
                let off = d.start_offset();
                if vis.line_of(off) == target_line - 1 && vis.col_of(off) == target_col {
                    let end = call.message_loc().map(|s| s.end_offset()).unwrap_or(d.end_offset());
                    return Some(AlignBase { offset: off, end_offset: end });
                }
            }
            if let Some(recv) = call.receiver() {
                return search_dot_above(&recv, target_line, target_col, vis);
            }
            None
        }
        _ => None,
    }
}

/// Navigate to the first call in the chain that has a dot.
/// Returns (dot_offset, selector_end, receiver_start, receiver_end).
fn first_call_dot(node: &Node) -> Option<(usize, usize, usize, usize)> {
    first_call_dot_recursive(node, None)
}

fn first_call_dot_recursive(
    node: &Node,
    acc: Option<(usize, usize, usize, usize)>,
) -> Option<(usize, usize, usize, usize)> {
    if !matches!(node, Node::CallNode { .. }) {
        return acc;
    }
    let call = node.as_call_node().unwrap();
    if let Some(dot) = call.call_operator_loc() {
        let sel_end = call.message_loc().map(|s| s.end_offset()).unwrap_or(dot.end_offset());
        let recv = call.receiver();
        let (rs, re) = recv.as_ref()
            .map(|r| (r.location().start_offset(), r.location().end_offset()))
            .unwrap_or((dot.start_offset(), dot.start_offset()));
        let new_result = Some((dot.start_offset(), sel_end, rs, re));
        if let Some(r) = recv {
            first_call_dot_recursive(&r, new_result)
        } else {
            new_result
        }
    } else {
        acc
    }
}

/// Like first_call_dot but for alignment check in hash pair context
fn first_call_dot_for_alignment(node: &ruby_prism::CallNode, receiver: &Node, vis: &MultilineVisitor) -> Option<(usize, usize)> {
    let (dot, sel_end, _, _) = first_call_dot(receiver)?;
    Some((dot, sel_end))
}

/// Find the pair ancestor of a node by scanning source
/// Returns the offset of the pair key if found
fn find_pair_ancestor(node: &ruby_prism::CallNode, vis: &MultilineVisitor) -> Option<usize> {
    // Walk up receiver chain to find the base receiver
    let recv = node.receiver()?;
    let top_off = walk_up_chain(&recv);

    // Scan backwards from top_off looking for `key:` or `key =>` pattern
    let s = vis.src();
    let ls = vis.line_start(top_off);
    let before = &vis.source_str()[ls..top_off];
    let trimmed = before.trim_end();

    // Check for `key: value` pattern (symbol key with colon)
    // The line before lhs_off might have something like `key: ` or `"key" =>`
    if let Some(colon_pos) = trimmed.rfind(':') {
        // Make sure it's a hash key colon (preceded by identifier)
        if colon_pos > 0 {
            let before_colon = &trimmed[..colon_pos];
            let before_trimmed = before_colon.trim_end();
            if !before_trimmed.is_empty() {
                let last_ch = before_trimmed.as_bytes()[before_trimmed.len() - 1];
                if last_ch.is_ascii_alphanumeric() || last_ch == b'_' || last_ch == b'"' || last_ch == b'\'' {
                    // Looks like a hash pair key
                    // Return the offset of the key
                    let key_start = ls + before.len() - before.trim_start().len();
                    return Some(key_start);
                }
            }
        }
    }

    // Check for `key =>` pattern (hash rocket)
    if let Some(arrow_pos) = trimmed.rfind("=>") {
        if arrow_pos > 0 {
            let key_start = ls + before.len() - before.trim_start().len();
            return Some(key_start);
        }
    }

    // Also check the previous line(s) for multiline hash pair values
    // Walk backwards to check enclosing lines
    let mut check_off = ls;
    while check_off > 0 {
        check_off -= 1;
        // Find start of this line
        let prev_ls = vis.line_start(check_off);
        let prev_line = vis.line_text(prev_ls);
        let prev_trimmed = prev_line.trim();

        // Check for hash pair pattern on this line
        if prev_trimmed.contains(':') || prev_trimmed.contains("=>") {
            // Check if our lhs_off is within the value of this pair
            // This is a heuristic - check if the line starts with something like `key:`
            let indent_part = &prev_line[..prev_line.len() - prev_line.trim_start().len()];
            let content = prev_line.trim_start();

            // Look for `key: value` where value starts on this line or next
            if let Some(colon_pos) = content.find(": ") {
                let before_colon = &content[..colon_pos];
                if !before_colon.is_empty() && before_colon.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let key_start = prev_ls + indent_part.len();
                    return Some(key_start);
                }
            }

            // Look for `"key" =>` or `"key" =>\n` pattern
            if content.contains(" => ") || content.ends_with(" =>") {
                let key_start = prev_ls + indent_part.len();
                return Some(key_start);
            }
        }

        // Don't search too many lines back
        if prev_ls == 0 || (ls - prev_ls > 200) {
            break;
        }
        check_off = prev_ls;
    }

    None
}

/// Check if the first call in the chain has a selector (not an implicit call)
fn has_selector_at_first_call(node: &Node) -> bool {
    has_selector_recursive(node, true)
}

fn has_selector_recursive(node: &Node, has_sel: bool) -> bool {
    if !matches!(node, Node::CallNode { .. }) {
        return has_sel;
    }
    let call = node.as_call_node().unwrap();
    if call.call_operator_loc().is_some() {
        let current_has_sel = call.message_loc().is_some();
        if let Some(recv) = call.receiver() {
            has_selector_recursive(&recv, current_has_sel)
        } else {
            current_has_sel
        }
    } else {
        has_sel
    }
}

fn kw_message_tail(keyword: &str) -> String {
    let kind = if keyword == "for" { "collection" } else { "condition" };
    let article = if keyword.starts_with('i') || keyword.starts_with('u') { "an" } else { "a" };
    format!("a {} in {} `{}` statement", kind, article, keyword)
}

impl<'a> Visit<'_> for MultilineVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}
