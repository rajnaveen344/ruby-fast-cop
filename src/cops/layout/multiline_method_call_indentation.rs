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
        Self { style, indentation_width: width.unwrap_or(2) }
    }
}

impl Cop for MultilineMethodCallIndentation {
    fn name(&self) -> &'static str { "Layout/MultilineMethodCallIndentation" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
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

impl<'a> MultilineVisitor<'a> {
    fn statement_indentation(&self, offset: usize) -> usize {
        let mut ls = self.ctx.line_start(offset);
        loop {
            if ls == 0 { return self.ctx.indentation_of(ls); }
            let prev_ls = self.ctx.line_start(ls - 1);
            let prev_trimmed = self.ctx.line_text(prev_ls).trim_end();
            if prev_trimmed.ends_with('.') || prev_trimmed.ends_with("&.") {
                ls = prev_ls;
                continue;
            }
            let cur_trimmed = self.ctx.line_text(ls).trim_start();
            if cur_trimmed.starts_with('.') || cur_trimmed.starts_with("&.") {
                ls = prev_ls;
                continue;
            }
            return self.ctx.indentation_of(ls);
        }
    }

    fn text(&self, start: usize, end: usize) -> &str { self.ctx.src(start, end) }

    fn first_line_text(&self, start: usize, end: usize) -> &str {
        self.text(start, end).lines().next().unwrap_or("")
    }

    fn end_of_line(&self, offset: usize) -> usize {
        let s = self.ctx.bytes();
        let mut i = offset;
        while i < s.len() && s[i] != b'\n' { i += 1; }
        i
    }
}

// Core check
impl<'a> MultilineVisitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        let name = node_name!(node);
        if name.as_ref() == "[]" || name.as_ref() == "[]=" { return; }

        let dot = match node.call_operator_loc() {
            Some(d) => d,
            None => return,
        };

        let dot_off = dot.start_offset();
        let dot_end = dot.end_offset();
        let sel_loc = node.message_loc();

        let (rhs_off, rhs_end) = if let Some(ref sel) = sel_loc {
            if self.ctx.line_of(dot_off) == self.ctx.line_of(sel.start_offset()) {
                (dot_off, sel.end_offset())
            } else {
                (sel.start_offset(), sel.end_offset())
            }
        } else if let Some(open) = node.opening_loc() {
            (dot_off, open.end_offset())
        } else {
            (dot_off, dot_end)
        };

        if !self.ctx.begins_its_line(rhs_off) { return; }
        if self.ctx.line_of(receiver.location().start_offset()) == self.ctx.line_of(rhs_off) { return; }

        let lhs_off = walk_up_chain(&receiver);
        let pair_ancestor = find_pair_ancestor(node, self);

        if pair_ancestor.is_some() && self.style == Style::Aligned {
            if is_base_hash(&receiver) {
                self.check_hash_pair_indentation(node, &receiver, lhs_off, rhs_off, rhs_end);
            } else {
                self.check_hash_pair_non_hash_aligned(node, &receiver, rhs_off, rhs_end);
            }
            return;
        }

        if pair_ancestor.is_some() && self.style == Style::Indented {
            if is_base_hash(&receiver) {
                self.check_hash_pair_indented_style(rhs_off, rhs_end, pair_ancestor.unwrap());
                return;
            }
        }

        if pair_ancestor.is_none() && self.not_for_this_cop(node) { return; }

        let rhs_col = self.ctx.col_of(rhs_off);

        match self.style {
            Style::Aligned => self.check_aligned(node, &receiver, lhs_off, rhs_off, rhs_end, rhs_col),
            Style::Indented => self.check_indented(node, lhs_off, rhs_off, rhs_end, rhs_col),
            Style::IndentedRelativeToReceiver => {
                self.check_relative(node, &receiver, lhs_off, rhs_off, rhs_end, rhs_col)
            }
        }
    }

    fn not_for_this_cop(&self, node: &ruby_prism::CallNode) -> bool {
        self.is_inside_enclosing_paren(node, false) || self.is_inside_enclosing_paren(node, true)
    }

    /// Scan backwards from dot for unmatched open paren.
    /// If `require_method_call` is true, only matches parens preceded by identifier (method call).
    /// If false, only matches grouping parens (not preceded by identifier).
    fn is_inside_enclosing_paren(&self, node: &ruby_prism::CallNode, require_method_call: bool) -> bool {
        let dot_off = match node.call_operator_loc() {
            Some(d) => d.start_offset(),
            None => return false,
        };
        let s = self.ctx.bytes();
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
                        let is_method = i > 0 && (s[i-1].is_ascii_alphanumeric() || s[i-1] == b'_'
                            || s[i-1] == b'!' || s[i-1] == b'?');
                        if is_method != require_method_call { return false; }
                        // Find matching close paren
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
                        return j - 1 > dot_off;
                    }
                }
                _ => {}
            }
        }
        false
    }

    // Hash pair indentation (aligned style)
    fn check_hash_pair_indentation(
        &mut self, node: &ruby_prism::CallNode, receiver: &Node,
        lhs_off: usize, rhs_off: usize, rhs_end: usize,
    ) {
        let rhs_col = self.ctx.col_of(rhs_off);

        if let Some(base) = self.find_hash_pair_alignment_base(receiver) {
            if self.aligned_with_first_line_dot(node, receiver, rhs_off, rhs_col) { return; }
            let bc = self.ctx.col_of(base.offset);
            if rhs_col == bc { return; }
            let msg = self.align_msg(rhs_off, rhs_end, &base);
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        if let Some(base) = self.hash_pair_receiver_base(node, receiver) {
            if self.aligned_with_first_line_dot(node, receiver, rhs_off, rhs_col) { return; }
            let bc = self.ctx.col_of(base.offset);
            if rhs_col == bc { return; }
            let msg = self.align_msg(rhs_off, rhs_end, &base);
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        self.check_aligned(node, receiver, lhs_off, rhs_off, rhs_end, rhs_col);
    }

    fn check_hash_pair_non_hash_aligned(
        &mut self, node: &ruby_prism::CallNode, receiver: &Node,
        rhs_off: usize, rhs_end: usize,
    ) {
        let rhs_col = self.ctx.col_of(rhs_off);

        if let Some(base) = self.hash_pair_doend_block_base(receiver) {
            let bc = self.ctx.col_of(base.offset);
            if rhs_col == bc { return; }
            let msg = self.align_msg(rhs_off, rhs_end, &base);
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        let chain_start = walk_up_chain(receiver);
        let chain_col = self.ctx.col_of(chain_start);

        if self.aligned_with_first_line_dot(node, receiver, rhs_off, rhs_col) { return; }

        let recv_start = receiver.location().start_offset();
        let recv_end = receiver.location().end_offset();
        let recv_first_line_end = self.end_of_line(recv_start);
        let s = self.ctx.bytes();
        let text_end = if recv_first_line_end < s.len() && s[recv_first_line_end - 1] == b'.' {
            recv_first_line_end
        } else if recv_first_line_end <= recv_end {
            recv_first_line_end
        } else {
            recv_end
        };

        if rhs_col == chain_col { return; }
        let msg = format!(
            "Align `{}` with `{}` on line {}.",
            self.text(rhs_off, rhs_end),
            self.first_line_text(recv_start, text_end),
            self.ctx.line_of(recv_start),
        );
        self.offense(rhs_off, rhs_end, &msg, chain_col as isize - rhs_col as isize);
    }

    fn hash_pair_doend_block_base(&self, receiver: &Node) -> Option<AlignBase> {
        if let Node::CallNode { .. } = receiver {
            let recv_call = receiver.as_call_node().unwrap();
            if let Some(recv_recv) = recv_call.receiver() {
                if let Node::CallNode { .. } = recv_recv {
                    let inner = recv_recv.as_call_node().unwrap();
                    if let Some(block) = inner.block() {
                        if !is_single_line_node(&block, self) {
                            if let Some(ir) = inner.receiver() {
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

    fn find_hash_pair_alignment_base(&self, receiver: &Node) -> Option<AlignBase> {
        if !is_base_hash(receiver) { return None; }
        let (dot, sel_end, _, _) = first_call_dot(receiver)?;
        Some(AlignBase { offset: dot, end_offset: sel_end })
    }

    fn hash_pair_receiver_base(&self, _node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        let (base_start, base_end, _, _, _) = find_base_receiver_info(receiver);

        if let Some(block_base) = self.find_block_chain_base_for_hash_pair(receiver) {
            return Some(block_base);
        }

        Some(AlignBase { offset: base_start, end_offset: base_end })
    }

    fn find_block_chain_base_for_hash_pair(&self, receiver: &Node) -> Option<AlignBase> {
        if let Node::CallNode { .. } = receiver {
            let recv_call = receiver.as_call_node().unwrap();
            if let Some(recv_recv) = recv_call.receiver() {
                if let Node::CallNode { .. } = recv_recv {
                    if recv_recv.as_call_node().unwrap().block().is_some() {
                        return dot_sel_base(&recv_call);
                    }
                }
            }
        }
        None
    }

    fn aligned_with_first_line_dot(&self, node: &ruby_prism::CallNode, receiver: &Node, rhs_off: usize, rhs_col: usize) -> bool {
        let b = *self.ctx.bytes().get(rhs_off).unwrap_or(&0);
        if b != b'.' && b != b'&' { return false; }
        let first = match first_call_dot_for_alignment(receiver) {
            Some(f) => f,
            None => return false,
        };
        let dot_col = self.ctx.col_of(first.0);
        let node_dot = node.call_operator_loc().map(|d| d.start_offset()).unwrap_or(0);
        if first.0 == node_dot { return false; }
        if let Node::CallNode { .. } = receiver {
            if let Some(recv_dot) = receiver.as_call_node().unwrap().call_operator_loc() {
                if recv_dot.start_offset() == first.0 { return false; }
            }
        }
        self.ctx.line_of(first.0) == self.ctx.line_of(receiver.location().start_offset()) && dot_col == rhs_col
    }

    fn check_hash_pair_indented_style(&mut self, rhs_off: usize, rhs_end: usize, pair_key_off: usize) {
        let rhs_col = self.ctx.col_of(rhs_off);
        let pair_key_col = self.ctx.col_of(pair_key_off);
        let correct_col = pair_key_col + self.indentation_width * 2;
        let hash_pair_base_col = pair_key_col + self.indentation_width;

        if rhs_col == correct_col { return; }

        let used = rhs_col as isize - hash_pair_base_col as isize;
        let msg = format!(
            "Use {} (not {}) spaces for indenting an expression spanning multiple lines.",
            self.indentation_width, used,
        );
        self.offense(rhs_off, rhs_end, &msg, correct_col as isize - rhs_col as isize);
    }

    // Aligned style
    fn check_aligned(
        &mut self, node: &ruby_prism::CallNode, receiver: &Node,
        lhs_off: usize, rhs_off: usize, rhs_end: usize, rhs_col: usize,
    ) {
        if let Some(base) = self.semantic_base(node, receiver, rhs_off) {
            let bc = self.ctx.col_of(base.offset);
            if rhs_col == bc { return; }
            let msg = self.align_msg(rhs_off, rhs_end, &base);
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        if let Some(base) = self.syntactic_base(node, receiver, lhs_off) {
            let bc = self.ctx.col_of(base.offset);
            if rhs_col == bc { return; }
            let msg = self.align_msg(rhs_off, rhs_end, &base);
            self.offense(rhs_off, rhs_end, &msg, bc as isize - rhs_col as isize);
            return;
        }

        let li = self.statement_indentation(lhs_off);
        let expected = li + self.indentation_width;
        if rhs_col == expected { return; }
        let used = rhs_col as isize - li as isize;
        let what = self.op_desc(lhs_off);
        let msg = format!(
            "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
            self.indentation_width, used, what,
        );
        self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
    }

    // Indented style
    fn check_indented(
        &mut self, _node: &ruby_prism::CallNode,
        lhs_off: usize, rhs_off: usize, rhs_end: usize, rhs_col: usize,
    ) {
        if let Some(kw) = self.find_keyword(lhs_off) {
            let bi = self.ctx.indentation_of(kw.0);
            let expected = bi + self.indentation_width + 2;
            if rhs_col == expected { return; }
            let what = kw_message_tail(&kw.1);
            let msg = format!(
                "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
                expected, rhs_col, what,
            );
            self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
            return;
        }

        if let Some(assign_off) = self.find_assign(lhs_off) {
            let bi = self.ctx.indentation_of(assign_off);
            let expected = bi + self.indentation_width;
            if rhs_col == expected { return; }
            let used = rhs_col as isize - bi as isize;
            let msg = format!(
                "Use {} (not {}) spaces for indenting an expression in an assignment spanning multiple lines.",
                self.indentation_width, used,
            );
            self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
            return;
        }

        let li = self.statement_indentation(lhs_off);
        let expected = li + self.indentation_width;
        if rhs_col == expected { return; }
        let used = rhs_col as isize - li as isize;
        let what = self.op_desc(lhs_off);
        let msg = format!(
            "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
            self.indentation_width, used, what,
        );
        self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
    }

    // Indented relative to receiver
    fn check_relative(
        &mut self, node: &ruby_prism::CallNode, receiver: &Node,
        lhs_off: usize, rhs_off: usize, rhs_end: usize, rhs_col: usize,
    ) {
        if let Some(base) = self.receiver_base(node, receiver) {
            let bc = self.ctx.col_of(base.offset);
            let extra = self.extra_indentation_for_relative(receiver);
            let expected = bc + extra;
            if rhs_col == expected { return; }
            let msg = format!(
                "Indent `{}` {} spaces more than `{}` on line {}.",
                self.text(rhs_off, rhs_end),
                self.indentation_width,
                self.first_line_text(base.offset, base.end_offset),
                self.ctx.line_of(base.offset),
            );
            self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
            return;
        }

        let li = self.ctx.indentation_of(lhs_off);
        let expected = li + self.indentation_width;
        if rhs_col == expected { return; }
        let rs = receiver.location().start_offset();
        let re = receiver.location().end_offset();
        let msg = format!(
            "Indent `{}` {} spaces more than `{}` on line {}.",
            self.text(rhs_off, rhs_end), self.indentation_width,
            self.first_line_text(rs, re), self.ctx.line_of(rs),
        );
        self.offense(rhs_off, rhs_end, &msg, expected as isize - rhs_col as isize);
    }

    fn extra_indentation_for_relative(&self, receiver: &Node) -> usize {
        let top_off = walk_up_chain(receiver);
        let s = self.ctx.bytes();
        if top_off > 0 && s[top_off - 1] == b'*' {
            if top_off > 1 && s[top_off - 2] == b'*' {
                self.indentation_width.saturating_sub(2)
            } else {
                self.indentation_width.saturating_sub(1)
            }
        } else {
            self.indentation_width
        }
    }

    // Semantic alignment base
    fn semantic_base(&self, node: &ruby_prism::CallNode, receiver: &Node, rhs_off: usize) -> Option<AlignBase> {
        let b = *self.ctx.bytes().get(rhs_off)?;
        if b != b'.' && b != b'&' { return None; }
        if self.is_argument_in_parenthesized_call(node) { return None; }

        self.dot_right_above(node, receiver)
            .or_else(|| self.block_chain_base(node, receiver))
            .or_else(|| self.first_call_base(node, receiver))
    }

    fn is_argument_in_parenthesized_call(&self, node: &ruby_prism::CallNode) -> bool {
        let recv = match node.receiver() {
            Some(r) => r,
            None => return false,
        };
        let top_off = walk_up_chain(&recv);
        let s = self.ctx.bytes();
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
                        return i > 0 && (s[i-1].is_ascii_alphanumeric() || s[i-1] == b'_'
                            || s[i-1] == b'!' || s[i-1] == b'?');
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
        let dot_col = self.ctx.col_of(dot_off);
        let dot_line = self.ctx.line_of(dot_off);

        if let Some(result) = search_dot_above(receiver, dot_line, dot_col, self) {
            let (_, base_end, _, _, is_paren) = find_base_receiver_info(receiver);
            if is_paren && self.ctx.line_of(result.offset) == self.ctx.line_of(base_end) {
                return None;
            }
            return Some(result);
        }

        // Fallback: source-based search for a dot at the same column on the line above
        if dot_line <= 1 { return None; }
        let prev_line_start = self.ctx.line_start(dot_off);
        if prev_line_start == 0 { return None; }
        let prev_ls = self.ctx.line_start(prev_line_start - 1);
        let prev_line = self.ctx.line_text(prev_ls);
        if dot_col < prev_line.len() {
            let ch = prev_line.as_bytes()[dot_col];
            if ch == b'.' || (ch == b'&' && dot_col + 1 < prev_line.len() && prev_line.as_bytes()[dot_col + 1] == b'.') {
                let found_off = prev_ls + dot_col;
                let s = self.ctx.bytes();
                let mut end = found_off + 1;
                if ch == b'&' { end += 1; }
                while end < s.len() && (s[end].is_ascii_alphanumeric() || s[end] == b'_' || s[end] == b'!' || s[end] == b'?') {
                    end += 1;
                }
                if end > found_off + 1 + (if ch == b'&' { 1 } else { 0 }) {
                    let eol = self.end_of_line(found_off);
                    return Some(AlignBase { offset: found_off, end_offset: eol });
                }
            }
        }
        None
    }

    fn block_chain_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        if node.block().is_some() {
            return self.find_continuation_node(node, receiver);
        }
        self.handle_descendant_block(node, receiver)
    }

    fn find_continuation_node(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        if let Node::CallNode { .. } = receiver {
            let recv_call = receiver.as_call_node().unwrap();

            if recv_call.block().is_some() && is_single_line_node(receiver, self) {
                return dot_sel_base(&recv_call);
            }

            if let Some(recv_recv) = recv_call.receiver() {
                if matches!(recv_recv, Node::ParenthesesNode { .. }) && node.block().is_some() {
                    if let Some(block) = node.block() {
                        if is_single_line_node(&block, self) {
                            return dot_sel_base(&recv_call);
                        }
                    }
                }
            }

            if let Some(recv_dot) = recv_call.call_operator_loc() {
                if let Some(recv_recv) = recv_call.receiver() {
                    let recv_recv_end_line = self.ctx.line_of(recv_recv.location().end_offset());
                    if self.ctx.line_of(recv_dot.start_offset()) > recv_recv_end_line {
                        let (_, base_end, _, _, is_paren) = find_base_receiver_info(receiver);
                        if is_paren {
                            if let Some((first_dot, _, _, _)) = first_call_dot(receiver) {
                                if self.ctx.line_of(first_dot) == self.ctx.line_of(base_end) {
                                    return None;
                                }
                            }
                        }
                        return dot_sel_base(&recv_call);
                    }
                }
            }
        }
        None
    }

    fn handle_descendant_block(&self, _node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        if let Node::CallNode { .. } = receiver {
            let recv_call = receiver.as_call_node().unwrap();
            if recv_call.block().is_some() && is_single_line_node(receiver, self) {
                return dot_sel_base(&recv_call);
            }

            if let Some(block) = recv_call.block() {
                if !is_single_line_node(&block, self) && recv_call.call_operator_loc().is_some() {
                    return dot_sel_base(&recv_call);
                }
            }

            if let Some(recv_recv) = recv_call.receiver() {
                if let Node::CallNode { .. } = recv_recv {
                    if let Some(block) = recv_recv.as_call_node().unwrap().block() {
                        if !is_single_line_node(&block, self) {
                            return dot_sel_base(&recv_call);
                        }
                    }
                }
            }
        }
        None
    }

    fn first_call_base(&self, node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        let (first_dot, first_sel_end, rs, re) = first_call_dot(receiver)?;

        if !has_selector_at_first_call(receiver) { return None; }

        let dot_line = self.ctx.line_of(first_dot);
        let rs_line = self.ctx.line_of(rs);
        let re_line = self.ctx.line_of(re);

        let (_, base_end, _, is_array, is_paren) = find_base_receiver_info(receiver);
        if is_array && dot_line == re_line {
            let node_dot = node.call_operator_loc()?.start_offset();
            if first_dot == node_dot { return None; }
            return Some(AlignBase { offset: first_dot, end_offset: first_sel_end });
        }

        if is_paren && dot_line == self.ctx.line_of(base_end) { return None; }
        if dot_line != rs_line { return None; }

        let node_dot = node.call_operator_loc()?.start_offset();
        if first_dot == node_dot { return None; }

        Some(AlignBase { offset: first_dot, end_offset: first_sel_end })
    }

    // Syntactic / receiver bases
    fn syntactic_base(&self, _node: &ruby_prism::CallNode, _receiver: &Node, lhs_off: usize) -> Option<AlignBase> {
        if let Some(kw) = self.find_keyword(lhs_off) {
            let kw_end = kw.0 + kw.1.len();
            let s = self.ctx.bytes();
            let mut expr_start = kw_end;
            while expr_start < s.len() && s[expr_start] == b' ' { expr_start += 1; }
            let expr_eol = self.end_of_line(expr_start);
            return Some(AlignBase { offset: expr_start, end_offset: expr_eol });
        }

        if let Some((_kw_off, expr_start)) = self.find_return_keyword(lhs_off) {
            return Some(AlignBase { offset: expr_start, end_offset: self.end_of_line(expr_start) });
        }

        if let Some(_assign_off) = self.find_assign(lhs_off) {
            let ls = self.ctx.line_start(lhs_off);
            let line = self.ctx.line_text(ls);
            let before = &line[..(lhs_off - ls).min(line.len())];
            if let Some(eq_pos) = before.rfind('=') {
                let abs_eq_pos = ls + eq_pos;
                let s = self.ctx.bytes();
                let mut rhs_start = abs_eq_pos + 1;
                while rhs_start < s.len() && s[rhs_start] == b' ' { rhs_start += 1; }
                return Some(AlignBase { offset: rhs_start, end_offset: self.end_of_line(rhs_start) });
            }
            return Some(AlignBase { offset: lhs_off, end_offset: self.end_of_line(lhs_off) });
        }

        if self.find_assign_above(lhs_off).is_some() {
            return Some(AlignBase { offset: lhs_off, end_offset: self.end_of_line(lhs_off) });
        }

        self.find_operator(lhs_off)
    }

    fn receiver_base(&self, _node: &ruby_prism::CallNode, receiver: &Node) -> Option<AlignBase> {
        if let Some(base) = self.hash_chain_base(receiver) {
            return Some(base);
        }
        if let Some((_, _, rs, re)) = first_call_dot(receiver) {
            return Some(AlignBase { offset: rs, end_offset: re });
        }
        let rs = receiver.location().start_offset();
        let re = receiver.location().end_offset();
        Some(AlignBase { offset: rs, end_offset: re })
    }

    fn hash_chain_base(&self, receiver: &Node) -> Option<AlignBase> {
        if !matches!(receiver, Node::CallNode { .. }) { return None; }
        let mut call = receiver.as_call_node().unwrap();
        loop {
            if let Some(base_recv) = call.receiver() {
                if matches!(base_recv, Node::HashNode { .. })
                    || is_method_on_paren_end_line(&call, &base_recv, self)
                {
                    return dot_sel_base(&call);
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

    // Keyword / assignment / operator helpers
    fn find_keyword(&self, lhs_off: usize) -> Option<(usize, String)> {
        let ls = self.ctx.line_start(lhs_off);
        if self.ctx.line_of(lhs_off) != self.ctx.line_of(ls) { return None; }
        let line = self.ctx.line_text(ls);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        for kw in &["if ", "unless ", "while ", "until "] {
            if trimmed.starts_with(kw) {
                return Some((ls + indent, kw.trim().to_string()));
            }
        }
        if trimmed.starts_with("for ") {
            return Some((ls + indent, "for".to_string()));
        }
        None
    }

    fn find_return_keyword(&self, lhs_off: usize) -> Option<(usize, usize)> {
        let ls = self.ctx.line_start(lhs_off);
        let line = self.ctx.line_text(ls);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if trimmed.starts_with("return ") {
            let kw_off = ls + indent;
            let s = self.ctx.bytes();
            let mut start = kw_off + 7;
            while start < s.len() && s[start] == b' ' { start += 1; }
            return Some((kw_off, start));
        }
        None
    }

    fn find_assign(&self, lhs_off: usize) -> Option<usize> {
        let ls = self.ctx.line_start(lhs_off);
        let line = self.ctx.line_text(ls);
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

    fn find_assign_above(&self, lhs_off: usize) -> Option<(usize, usize)> {
        let ls = self.ctx.line_start(lhs_off);
        if ls == 0 { return None; }
        let mut search_off = ls;
        for _ in 0..5 {
            if search_off == 0 { break; }
            let prev_ls = self.ctx.line_start(search_off - 1);
            let prev_line = self.ctx.line_text(prev_ls);
            let prev_trimmed = prev_line.trim_end();

            if prev_trimmed.ends_with('=') && !prev_trimmed.ends_with("==")
                && !prev_trimmed.ends_with("!=") && !prev_trimmed.ends_with("<=")
                && !prev_trimmed.ends_with(">=") && !prev_trimmed.ends_with("=>") {
                return Some((prev_ls, lhs_off));
            }

            let content = prev_trimmed.trim_start();
            if content.starts_with("return ") {
                let indent = prev_trimmed.len() - content.len();
                let kw_end = prev_ls + indent + 7;
                let s = self.ctx.bytes();
                let mut expr_start = kw_end;
                while expr_start < s.len() && s[expr_start] == b' ' { expr_start += 1; }
                return Some((prev_ls, expr_start));
            }

            search_off = prev_ls;
        }
        None
    }

    fn is_in_assignment_context(&self, lhs_off: usize) -> bool {
        if self.find_assign(lhs_off).is_some() || self.find_assign_above(lhs_off).is_some() {
            return true;
        }
        let ls = self.ctx.line_start(lhs_off);
        let mut search_off = ls;
        for _ in 0..5 {
            if search_off == 0 { break; }
            let prev_ls = self.ctx.line_start(search_off - 1);
            let content = self.ctx.line_text(prev_ls).trim();
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
        let ls = self.ctx.line_start(lhs_off);
        let line = self.ctx.line_text(ls);
        let before = &line[..(lhs_off - ls).min(line.len())];
        let trimmed = before.trim_end();

        if trimmed.ends_with('+') || trimmed.ends_with("- ") || trimmed.ends_with('*')
            || trimmed.ends_with('/') || trimmed.ends_with('%') || trimmed.ends_with("<<")
        {
            return Some(AlignBase { offset: lhs_off, end_offset: self.end_of_line(lhs_off) });
        }
        None
    }

    fn op_desc(&self, lhs_off: usize) -> String {
        if let Some(kw) = self.find_keyword(lhs_off) {
            return kw_message_tail(&kw.1);
        }
        if self.is_in_assignment_context(lhs_off) {
            return "an expression in an assignment".to_string();
        }
        "an expression".to_string()
    }

    fn align_msg(&self, rhs_off: usize, rhs_end: usize, base: &AlignBase) -> String {
        format!(
            "Align `{}` with `{}` on line {}.",
            self.text(rhs_off, rhs_end),
            self.first_line_text(base.offset, base.end_offset),
            self.ctx.line_of(base.offset),
        )
    }

    // Correction helpers
    fn collect_extra_correction_lines(&self, rhs_off: usize) -> Vec<usize> {
        let s = self.ctx.bytes();
        let ls = self.ctx.line_start(rhs_off);
        let eol = self.end_of_line(rhs_off);
        let line_str = std::str::from_utf8(&s[ls..eol]).unwrap_or("");
        let rhs_col = self.ctx.col_of(rhs_off);

        let mut extra = Vec::new();

        let has_do = line_str.contains(" do |")
            || line_str.contains(" do\n")
            || line_str.trim_end().ends_with(" do")
            || line_str.trim_end().ends_with(" do |");

        if has_do {
            let mut depth = 1i32;
            let mut pos = eol;
            if pos < s.len() && s[pos] == b'\n' { pos += 1; }

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
                if pos < s.len() && s[pos] == b'\n' { pos += 1; }
            }
        } else {
            let mut pos = eol;
            if pos < s.len() && s[pos] == b'\n' { pos += 1; }

            while pos < s.len() {
                let line_start = pos;
                let line_end = self.end_of_line(pos);
                let ln = std::str::from_utf8(&s[line_start..line_end]).unwrap_or("");
                let trimmed = ln.trim();
                let ln_col = ln.len() - ln.trim_start().len();

                if ln_col == rhs_col && (trimmed.starts_with('.') || trimmed.starts_with("&.")) {
                    extra.push(line_start);
                } else {
                    break;
                }

                pos = line_end;
                if pos < s.len() && s[pos] == b'\n' { pos += 1; }
            }
        }

        extra
    }

    fn offense(&mut self, rhs_off: usize, rhs_end: usize, msg: &str, delta: isize) {
        let extra = self.collect_extra_correction_lines(rhs_off);
        self.offense_with_extra(rhs_off, rhs_end, msg, delta, &extra);
    }

    fn offense_with_extra(&mut self, rhs_off: usize, rhs_end: usize, msg: &str, delta: isize, extra_line_offsets: &[usize]) {
        let off = self.ctx.offense_with_range(
            "Layout/MultilineMethodCallIndentation", msg,
            Severity::Convention, rhs_off, rhs_end,
        );
        let ls = self.ctx.line_start(rhs_off);
        let cur = rhs_off - ls;
        let new = (cur as isize + delta).max(0) as usize;

        let mut edits = vec![Edit {
            start_offset: ls, end_offset: rhs_off, replacement: " ".repeat(new),
        }];

        let s = self.ctx.bytes();
        for &line_off in extra_line_offsets {
            let el_ls = self.ctx.line_start(line_off);
            let mut ws_end = el_ls;
            while ws_end < s.len() && (s[ws_end] == b' ' || s[ws_end] == b'\t') { ws_end += 1; }
            let el_new = ((ws_end - el_ls) as isize + delta).max(0) as usize;
            edits.push(Edit {
                start_offset: el_ls, end_offset: ws_end, replacement: " ".repeat(el_new),
            });
        }

        self.offenses.push(off.with_correction(Correction { edits }));
    }
}

// Free functions for node traversal

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
    let name = node_name!(call);
    name.ends_with('=') && name.as_ref() != "==" && name.as_ref() != "!="
        && name.as_ref() != "<=" && name.as_ref() != ">="
}

fn is_base_hash(node: &Node) -> bool {
    base_receiver_matches(node, |n| matches!(n, Node::HashNode { .. }))
}

fn base_receiver_matches(node: &Node, pred: impl Fn(&Node) -> bool) -> bool {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            match call.receiver() {
                Some(recv) => base_receiver_matches(&recv, pred),
                None => pred(node),
            }
        }
        _ => pred(node),
    }
}

fn find_base_receiver_info(node: &Node) -> (usize, usize, bool, bool, bool) {
    match node {
        Node::CallNode { .. } => {
            match node.as_call_node().unwrap().receiver() {
                Some(recv) => find_base_receiver_info(&recv),
                None => { let loc = node.location(); (loc.start_offset(), loc.end_offset(), false, false, false) }
            }
        }
        Node::HashNode { .. } => { let loc = node.location(); (loc.start_offset(), loc.end_offset(), true, false, false) }
        Node::ArrayNode { .. } => { let loc = node.location(); (loc.start_offset(), loc.end_offset(), false, true, false) }
        Node::ParenthesesNode { .. } => { let loc = node.location(); (loc.start_offset(), loc.end_offset(), false, false, true) }
        _ => { let loc = node.location(); (loc.start_offset(), loc.end_offset(), false, false, false) }
    }
}

fn is_single_line_node(node: &Node, vis: &MultilineVisitor) -> bool {
    vis.ctx.line_of(node.location().start_offset()) == vis.ctx.line_of(node.location().end_offset())
}

fn is_method_on_paren_end_line(call: &ruby_prism::CallNode, base: &Node, vis: &MultilineVisitor) -> bool {
    if matches!(base, Node::ParenthesesNode { .. }) {
        if let Some(dot) = call.call_operator_loc() {
            return vis.ctx.line_of(dot.start_offset()) == vis.ctx.line_of(base.location().end_offset());
        }
    }
    false
}

fn dot_sel_base(call: &ruby_prism::CallNode) -> Option<AlignBase> {
    let dot = call.call_operator_loc()?;
    let end = call.message_loc().map(|s| s.end_offset()).unwrap_or(dot.end_offset());
    Some(AlignBase { offset: dot.start_offset(), end_offset: end })
}

fn search_dot_above(node: &Node, target_line: usize, target_col: usize, vis: &MultilineVisitor) -> Option<AlignBase> {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        if let Some(d) = call.call_operator_loc() {
            let off = d.start_offset();
            if vis.ctx.line_of(off) == target_line - 1 && vis.ctx.col_of(off) == target_col {
                let end = call.message_loc().map(|s| s.end_offset()).unwrap_or(d.end_offset());
                return Some(AlignBase { offset: off, end_offset: end });
            }
        }
        if let Some(recv) = call.receiver() {
            return search_dot_above(&recv, target_line, target_col, vis);
        }
    }
    None
}

fn first_call_dot(node: &Node) -> Option<(usize, usize, usize, usize)> {
    first_call_dot_recursive(node, None)
}

fn first_call_dot_recursive(node: &Node, acc: Option<(usize, usize, usize, usize)>) -> Option<(usize, usize, usize, usize)> {
    if !matches!(node, Node::CallNode { .. }) { return acc; }
    let call = node.as_call_node().unwrap();
    if let Some(dot) = call.call_operator_loc() {
        let sel_end = call.message_loc().map(|s| s.end_offset()).unwrap_or(dot.end_offset());
        let recv = call.receiver();
        let (rs, re) = recv.as_ref()
            .map(|r| (r.location().start_offset(), r.location().end_offset()))
            .unwrap_or((dot.start_offset(), dot.start_offset()));
        let new_result = Some((dot.start_offset(), sel_end, rs, re));
        match recv {
            Some(r) => first_call_dot_recursive(&r, new_result),
            None => new_result,
        }
    } else {
        acc
    }
}

fn first_call_dot_for_alignment(receiver: &Node) -> Option<(usize, usize)> {
    let (dot, sel_end, _, _) = first_call_dot(receiver)?;
    Some((dot, sel_end))
}

fn find_pair_ancestor(node: &ruby_prism::CallNode, vis: &MultilineVisitor) -> Option<usize> {
    let recv = node.receiver()?;
    let top_off = walk_up_chain(&recv);

    let ls = vis.ctx.line_start(top_off);
    let before = &vis.ctx.source[ls..top_off];
    let trimmed = before.trim_end();

    if let Some(colon_pos) = trimmed.rfind(':') {
        if colon_pos > 0 {
            let before_colon = trimmed[..colon_pos].trim_end();
            if !before_colon.is_empty() {
                let last_ch = before_colon.as_bytes()[before_colon.len() - 1];
                if last_ch.is_ascii_alphanumeric() || last_ch == b'_' || last_ch == b'"' || last_ch == b'\'' {
                    let key_start = ls + before.len() - before.trim_start().len();
                    return Some(key_start);
                }
            }
        }
    }

    if let Some(arrow_pos) = trimmed.rfind("=>") {
        if arrow_pos > 0 {
            let key_start = ls + before.len() - before.trim_start().len();
            return Some(key_start);
        }
    }

    let mut check_off = ls;
    while check_off > 0 {
        check_off -= 1;
        let prev_ls = vis.ctx.line_start(check_off);
        let prev_line = vis.ctx.line_text(prev_ls);
        let prev_trimmed = prev_line.trim();

        if prev_trimmed.contains(':') || prev_trimmed.contains("=>") {
            let indent_part = &prev_line[..prev_line.len() - prev_line.trim_start().len()];
            let content = prev_line.trim_start();

            if let Some(colon_pos) = content.find(": ") {
                let before_colon = &content[..colon_pos];
                if !before_colon.is_empty() && before_colon.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return Some(prev_ls + indent_part.len());
                }
            }

            if content.contains(" => ") || content.ends_with(" =>") {
                return Some(prev_ls + indent_part.len());
            }
        }

        if prev_ls == 0 || (ls - prev_ls > 200) { break; }
        check_off = prev_ls;
    }

    None
}

fn has_selector_at_first_call(node: &Node) -> bool {
    has_selector_recursive(node, true)
}

fn has_selector_recursive(node: &Node, has_sel: bool) -> bool {
    if !matches!(node, Node::CallNode { .. }) { return has_sel; }
    let call = node.as_call_node().unwrap();
    if call.call_operator_loc().is_some() {
        let current_has_sel = call.message_loc().is_some();
        match call.receiver() {
            Some(recv) => has_selector_recursive(&recv, current_has_sel),
            None => current_has_sel,
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

crate::register_cop!("Layout/MultilineMethodCallIndentation", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/MultilineMethodCallIndentation");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "indented" => Style::Indented,
            "indented_relative_to_receiver" => Style::IndentedRelativeToReceiver,
            _ => Style::Aligned,
        })
        .unwrap_or(Style::Aligned);
    let width = cop_config
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_i64())
        .map(|v| v as usize);
    Some(Box::new(MultilineMethodCallIndentation::new(style, width)))
});
