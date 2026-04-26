//! Layout/HeredocArgumentClosingParenthesis - place `)` of a method call with a heredoc arg
//! on the same line as the heredoc opening tag.
//!
//! Ported from: https://raw.githubusercontent.com/rubocop/rubocop/v1.85.0/lib/rubocop/cop/layout/heredoc_argument_closing_parenthesis.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP: &str = "Layout/HeredocArgumentClosingParenthesis";
const MSG: &str = "Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.";

#[derive(Default)]
pub struct HeredocArgumentClosingParenthesis;

impl HeredocArgumentClosingParenthesis {
    pub fn new() -> Self { Self }
}

impl Cop for HeredocArgumentClosingParenthesis {
    fn name(&self) -> &'static str { COP }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = HVisitor {
            ctx,
            stack: Vec::new(),
            offenses: Vec::new(),
            reported: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Clone)]
struct CallFrame {
    /// start_offset of the CallNode (used as identity)
    id: usize,
    /// the call has parenthesised arg-list (closing_loc Some)
    has_parens: bool,
    closing_start: usize,
    /// last argument's end offset (or 0 if none)
    last_arg_end: usize,
    #[allow(dead_code)]
    contains_end_keyword: bool,
    /// (start, end) byte ranges of this call's direct arguments (used to verify child-is-an-argument)
    arg_ranges: Vec<(usize, usize)>,
}

struct HVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    stack: Vec<CallFrame>,
    offenses: Vec<Offense>,
    /// dedup so the same outer call isn't reported twice (multi-heredoc case)
    reported: Vec<usize>,
}

impl<'a> HVisitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        // Need a heredoc somewhere in the immediate args (or hash values)
        let heredoc_last_line = match find_heredoc_last_line(self.ctx, node) {
            Some(l) => l,
            None => return,
        };

        // Walk current stack to find outermost-send on same line whose closing
        // parens line != heredoc terminator line.
        // Algorithm: start with `node` itself, ascend.
        // Stack contains *ancestors* including the CURRENT node (we push before recursing into args).
        // The current call node may not yet be on the stack — it depends on visit order.
        // We perform the walk from the current call upward.
        // RuboCop:
        //   previous = heredoc; current = previous.parent
        //   until send_missing_closing_parens?(current, previous, heredoc): ascend.
        //   send_missing? = parent.call_type? && parent.arguments.include?(child)
        //                   && parent.loc.begin && parent.loc.end.line != heredoc.last_line
        // i.e. keep ascending while parent qualifies; return last `current` that qualifies.

        // Build a frame for `node`:
        let cur = match call_frame(self.ctx, node) {
            Some(f) => f,
            None => return, // no parens → not applicable
        };

        // Walk: innermost (cur) → outermost. Return FIRST whose `)` line > heredoc_last_line.
        // Mirrors RuboCop's `until send_missing_closing_parens?(current, previous, heredoc)`.
        // Must verify child is in parent's *arguments* (not e.g. receiver).
        let qualifies = |f: &CallFrame| f.has_parens && self.ctx.line_of(f.closing_start) > heredoc_last_line;
        let mut chosen: Option<CallFrame> = None;
        let mut prev_range = (cur.id, cur.id); // first child = cur
        if qualifies(&cur) {
            chosen = Some(cur.clone());
        } else {
            prev_range = (cur.id, cur.id + 1); // identity-ish; replaced below
            // Better: prev's full extent. Use cur's location range:
            let n_loc = node.location();
            prev_range = (n_loc.start_offset(), n_loc.end_offset());
            for frame in self.stack.iter().rev() {
                // Verify prev is in frame's arguments
                let is_arg = frame.arg_ranges.iter().any(|(s, _e)| *s == prev_range.0);
                if !is_arg { break; }
                if qualifies(frame) {
                    chosen = Some(frame.clone());
                    break;
                }
                prev_range = (frame.id, frame.id + 1);
                // For frame's "id" alone we need the call node's location to match parent's arg range.
                // Use frame.id (start_offset) as the key — parent.arg_ranges starts list contains it.
            }
        }

        let outer = match chosen { Some(c) => c, None => return };
        let _ = prev_range;

        // Skip if already reported
        if self.reported.contains(&outer.id) { return; }

        // Guard: end keyword between heredoc and closing_paren (e.g. `do...end)`)
        if has_end_between(self.ctx, &outer) {
            return;
        }

        // Guard: subsequent_closing_parentheses_in_same_line — if last arg's end
        // is immediately followed (col+1) by the outer `)`, skip.
        if subsequent_closing_paren_same_line(self.ctx, &outer) {
            return;
        }

        // Guard: argument between heredoc end and `)`.
        if argument_between_heredoc_end_and_closing(self.ctx, &outer, heredoc_last_line) {
            return;
        }

        self.reported.push(outer.id);

        // Offense at the outer `)`.
        let close = outer.closing_start;
        let mut off = self.ctx.offense_with_range(COP, MSG, Severity::Convention, close, close + 1);
        off.correction = build_correction(self.ctx, &outer);
        self.offenses.push(off);
    }
}

fn call_frame(ctx: &CheckContext, node: &ruby_prism::CallNode) -> Option<CallFrame> {
    let id = node.location().start_offset();
    let closing = node.closing_loc();
    let has_parens = closing
        .as_ref()
        .map(|l| ctx.src(l.start_offset(), l.end_offset()) == ")")
        .unwrap_or(false);
    let arg_ranges: Vec<(usize, usize)> = if let Some(args) = node.arguments() {
        args.arguments().iter().map(|a| (a.location().start_offset(), a.location().end_offset())).collect()
    } else { Vec::new() };
    if !has_parens {
        return Some(CallFrame { id, has_parens: false, closing_start: 0, last_arg_end: 0, contains_end_keyword: false, arg_ranges });
    }
    let closing = closing.unwrap();
    let last_arg_end = arg_ranges.last().map(|(_, e)| *e).unwrap_or(0);
    Some(CallFrame {
        id,
        has_parens: true,
        closing_start: closing.start_offset(),
        last_arg_end,
        contains_end_keyword: false,
        arg_ranges,
    })
}

/// If any heredoc among the call's args (recursing into hash values), return
/// the 1-indexed terminator line of the *bottom-most* heredoc.
fn find_heredoc_last_line(ctx: &CheckContext, node: &ruby_prism::CallNode) -> Option<usize> {
    let args = node.arguments()?;
    let mut max_line: Option<usize> = None;
    for arg in args.arguments().iter() {
        if let Some(line) = heredoc_terminator_line(ctx, &arg) {
            max_line = Some(max_line.unwrap_or(0).max(line));
        }
        // Also descend into hashes
        if let Some(h) = arg.as_hash_node() {
            for el in h.elements().iter() {
                if let Some(assoc) = el.as_assoc_node() {
                    let v = assoc.value();
                    if let Some(line) = heredoc_terminator_line(ctx, &v) {
                        max_line = Some(max_line.unwrap_or(0).max(line));
                    }
                }
            }
        }
        // KeywordHashNode (implicit hash)
        if let Some(kh) = arg.as_keyword_hash_node() {
            for el in kh.elements().iter() {
                if let Some(assoc) = el.as_assoc_node() {
                    let v = assoc.value();
                    if let Some(line) = heredoc_terminator_line(ctx, &v) {
                        max_line = Some(max_line.unwrap_or(0).max(line));
                    }
                }
            }
        }
        // single_line_send_with_heredoc_receiver — receiver is a heredoc and its
        // terminator end > send range end.
        if let Some(call) = arg.as_call_node() {
            if let Some(recv) = call.receiver() {
                if let Some(line) = heredoc_terminator_line(ctx, &recv) {
                    let term_end = line_end_offset(ctx.source, line);
                    if term_end > call.location().end_offset() {
                        max_line = Some(max_line.unwrap_or(0).max(line));
                    }
                }
            }
        }
    }
    max_line
}

fn heredoc_terminator_line(ctx: &CheckContext, node: &Node) -> Option<usize> {
    let opener_text = if let Some(s) = node.as_string_node() {
        let o = s.opening_loc()?;
        let bytes = o.as_slice();
        if !bytes.starts_with(b"<<") { return None; }
        std::str::from_utf8(bytes).ok()?.to_string()
    } else if let Some(s) = node.as_interpolated_string_node() {
        let o = s.opening_loc()?;
        let bytes = o.as_slice();
        if !bytes.starts_with(b"<<") { return None; }
        std::str::from_utf8(bytes).ok()?.to_string()
    } else if let Some(s) = node.as_x_string_node() {
        let o = s.opening_loc();
        let bytes = o.as_slice();
        if !bytes.starts_with(b"<<") { return None; }
        std::str::from_utf8(bytes).ok()?.to_string()
    } else if let Some(s) = node.as_interpolated_x_string_node() {
        let o = s.opening_loc();
        let bytes = o.as_slice();
        if !bytes.starts_with(b"<<") { return None; }
        std::str::from_utf8(bytes).ok()?.to_string()
    } else {
        return None;
    };

    let delim = parse_delimiter(&opener_text)?;
    // Find first line starting at or after node range that, when trimmed, equals delim.
    let after = node.location().end_offset();
    find_terminator_line(ctx.source, after, &delim)
}

fn parse_delimiter(opening: &str) -> Option<String> {
    let s = opening.trim_start_matches('<');
    let s = s.trim_start_matches(['-', '~']);
    let (s, q) = if s.starts_with('"') || s.starts_with('\'') || s.starts_with('`') {
        (&s[1..], Some(&s[..1]))
    } else { (s, None) };
    let d: String = if let Some(qc) = q {
        s.split(qc).next().unwrap_or("").to_string()
    } else {
        s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
    };
    if d.is_empty() { None } else { Some(d) }
}

fn find_terminator_line(source: &str, after_offset: usize, delim: &str) -> Option<usize> {
    // Start scanning from line containing `after_offset` (so we can look at the *next* line).
    let mut byte_pos = line_start(source, after_offset);
    let bytes = source.as_bytes();
    let mut line_no = 1 + bytes[..byte_pos].iter().filter(|&&b| b == b'\n').count();
    // skip current line content first if our after_offset is mid-line
    // Heredoc terminator is on its own line, so move to next line
    let first_nl = source[byte_pos..].find('\n').map(|p| byte_pos + p + 1);
    if let Some(p) = first_nl { byte_pos = p; line_no += 1; } else { return None; }
    while byte_pos <= source.len() {
        let line_end = source[byte_pos..].find('\n').map(|p| byte_pos + p).unwrap_or(source.len());
        let line = &source[byte_pos..line_end];
        if line.trim() == delim { return Some(line_no); }
        if line_end >= source.len() { break; }
        byte_pos = line_end + 1;
        line_no += 1;
    }
    None
}

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |p| p + 1)
}

fn line_end_offset(source: &str, line: usize) -> usize {
    let mut count = 0;
    for (i, &b) in source.as_bytes().iter().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == line { return i; }
        }
    }
    source.len()
}

fn has_end_between(ctx: &CheckContext, outer: &CallFrame) -> bool {
    // Check if there's a literal `end` keyword between last_arg_end and closing_start (in source).
    // RuboCop checks `ancestor.loc_is?(:end, 'end')`, i.e. a block/def/class with `end` enclosed.
    // Heuristic: scan source between last_arg_end and closing_start for the word `end` outside strings.
    if outer.last_arg_end == 0 { return false; }
    let s = ctx.src(outer.last_arg_end, outer.closing_start);
    contains_end_keyword(s)
}

fn contains_end_keyword(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i+3] == b"end" {
            let before_ok = i == 0 || !is_ident_char(bytes[i-1]);
            let after = bytes.get(i+3).copied();
            let after_ok = match after { None => true, Some(c) => !is_ident_char(c) };
            if before_ok && after_ok { return true; }
        }
        i += 1;
    }
    false
}
fn is_ident_char(b: u8) -> bool { b.is_ascii_alphanumeric() || b == b'_' }

fn subsequent_closing_paren_same_line(ctx: &CheckContext, outer: &CallFrame) -> bool {
    if outer.last_arg_end == 0 { return false; }
    // last arg end and closing on same line, and column of `)` == col of last_arg_end+1?
    // The Ruby code checks last_argument's `loc.end` (which for nested calls is *its* `)`).
    // Heuristic: scan source from last_arg_end forward, skipping whitespace; if we hit `)` at pos==closing_start, true.
    let bytes = ctx.source.as_bytes();
    let mut i = outer.last_arg_end;
    if i == 0 || i > outer.closing_start { return false; }
    // last arg's last char must be `)` for this guard to apply
    if bytes.get(i.saturating_sub(1)) != Some(&b')') { return false; }
    // Same line check
    if ctx.line_of(i.saturating_sub(1)) != ctx.line_of(outer.closing_start) { return false; }
    // Outer `)` immediately follows last_arg_end (column+1)?
    // RuboCop: end_of_outer_send.column == end_of_last_arg.column + 1.
    // last_arg.end column == column of char AFTER last char. That is col_of(last_arg_end).
    // outer.closing column == col_of(closing_start). They're equal-side-by-side if closing_start == last_arg_end.
    while i < outer.closing_start && bytes[i] == b' ' { i += 1; }
    i == outer.closing_start
}

fn argument_between_heredoc_end_and_closing(ctx: &CheckContext, outer: &CallFrame, heredoc_last_line: usize) -> bool {
    // RuboCop: bottom-most heredoc_end among args; if heredoc_end < `)`-pos and
    // stripped source between heredoc_end and `)` is non-empty → return true.
    // We compute heredoc_end_offset = end-of-line offset for `heredoc_last_line`.
    let close = outer.closing_start;
    let heredoc_end_offset = end_of_line_offset(ctx.source, heredoc_last_line);
    if heredoc_end_offset >= close { return false; }
    let between = ctx.src(heredoc_end_offset, close);
    !between.trim().is_empty()
}

fn end_of_line_offset(source: &str, line: usize) -> usize {
    let mut count = 1usize;
    for (i, &b) in source.as_bytes().iter().enumerate() {
        if count == line {
            // find the \n on this line
            for (j, &b2) in source.as_bytes().iter().enumerate().skip(i) {
                if b2 == b'\n' { return j; }
            }
            return source.len();
        }
        if b == b'\n' { count += 1; }
    }
    source.len()
}

fn build_correction(ctx: &CheckContext, outer: &CallFrame) -> Option<Correction> {
    let mut edits: Vec<Edit> = Vec::new();

    // 1) fix_closing_parenthesis: remove old `)` and add `)` after last_argument
    let close = outer.closing_start;
    let bytes = ctx.source.as_bytes();
    let close_line = ctx.line_of(close);
    let close_line_start = line_start(ctx.source, close);
    let close_line_end = ctx.source[close_line_start..]
        .find('\n')
        .map(|p| close_line_start + p)
        .unwrap_or(ctx.source.len());
    // safe_to_remove_line_containing_closing_paren?: line matches /^ *\) {0,20},{0,1} *$/
    let line_text = &ctx.source[close_line_start..close_line_end];
    let safe_full_line = matches_safe_line(line_text);
    let removal_begin = if safe_full_line {
        // include the preceding newline
        if close_line_start > 0 { close_line_start - 1 } else { close_line_start }
    } else {
        close // just the `)`
    };
    // incorrect_parenthesis_removal_end: end after `)` and possibly the comma right after
    let mut removal_end = close + 1;
    if removal_end < bytes.len() && bytes[removal_end] == b',' {
        removal_end += 1;
    }
    edits.push(Edit { start_offset: removal_begin, end_offset: removal_end, replacement: String::new() });

    // 2) Insert `)` after last_argument
    if outer.last_arg_end > 0 {
        edits.push(Edit { start_offset: outer.last_arg_end, end_offset: outer.last_arg_end, replacement: ")".to_string() });
    }

    // 3) internal_trailing_comma: remove from last_arg_end through the comma (inclusive).
    if outer.last_arg_end > 0 && outer.last_arg_end < close {
        let segment = ctx.src(outer.last_arg_end, close);
        let comma_idx = segment.find(',');
        let nl_idx = segment.find('\n');
        if let (Some(ci), Some(ni)) = (comma_idx, nl_idx) {
            if ci < ni {
                let from = outer.last_arg_end;
                let to = outer.last_arg_end + ci + 1; // include the comma
                edits.push(Edit { start_offset: from, end_offset: to, replacement: String::new() });
            }
        }
    }

    // 4) external_trailing_comma: skip up to 20 spaces past `)`; if next char is `,`, remove
    //    those spaces+comma and add `,` immediately after last_argument.
    let mut offset = 0usize;
    let limit = 20usize;
    while offset < limit && bytes.get(close + 1 + offset).copied() == Some(b' ') {
        offset += 1;
    }
    if bytes.get(close + 1 + offset).copied() == Some(b',') {
        // remove [close+1, close+1+offset+1)
        edits.push(Edit { start_offset: close + 1, end_offset: close + 1 + offset + 1, replacement: String::new() });
        if outer.last_arg_end > 0 {
            edits.push(Edit { start_offset: outer.last_arg_end, end_offset: outer.last_arg_end, replacement: ",".to_string() });
        }
    }

    let _ = close_line; // silence
    if edits.is_empty() { None } else { Some(Correction { edits }) }
}

fn matches_safe_line(line: &str) -> bool {
    // /^ *\) {0,20},{0,1} *$/
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' { i += 1; }
    if i >= bytes.len() || bytes[i] != b')' { return false; }
    i += 1;
    let mut sp = 0;
    while i < bytes.len() && bytes[i] == b' ' && sp <= 20 { i += 1; sp += 1; }
    if i < bytes.len() && bytes[i] == b',' { i += 1; }
    while i < bytes.len() && bytes[i] == b' ' { i += 1; }
    i == bytes.len()
}

// ---- Visitor ----

impl<'a, 'b> Visit<'b> for HVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // First, run check on this call (may report)
        self.check_call(node);
        // Then push frame and recurse
        if let Some(frame) = call_frame(self.ctx, node) {
            self.stack.push(frame);
            ruby_prism::visit_call_node(self, node);
            self.stack.pop();
        } else {
            ruby_prism::visit_call_node(self, node);
        }
    }
}

crate::register_cop!("Layout/HeredocArgumentClosingParenthesis", |_cfg| Some(Box::new(
    HeredocArgumentClosingParenthesis::new()
)));
