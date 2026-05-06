//! Shared helper for the MultilineLiteralBraceLayout mixin.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/mixin/multiline_literal_brace_layout.rb
//!
//! Used by:
//! - Layout/MultilineArrayBraceLayout
//! - Layout/MultilineHashBraceLayout
//! - Layout/MultilineMethodCallBraceLayout

use crate::cops::CheckContext;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BraceLayoutStyle {
    Symmetrical,
    NewLine,
    SameLine,
}

impl BraceLayoutStyle {
    pub fn from_str(s: &str) -> Self {
        match s {
            "new_line" => BraceLayoutStyle::NewLine,
            "same_line" => BraceLayoutStyle::SameLine,
            _ => BraceLayoutStyle::Symmetrical,
        }
    }
}

pub struct Messages {
    pub same_line: &'static str,
    pub new_line: &'static str,
    pub always_new_line: &'static str,
    pub always_same_line: &'static str,
}

/// Parameters for a brace-layout check.
pub struct BraceCheck<'a> {
    pub cop_name: &'static str,
    pub style: BraceLayoutStyle,
    pub messages: &'a Messages,
    /// Opening brace byte range.
    pub open_start: usize,
    pub open_end: usize,
    /// Closing brace byte range.
    pub close_start: usize,
    pub close_end: usize,
    /// First child byte range start.
    pub first_child_start: usize,
    /// Last child end offset (exclusive).
    pub last_child_end: usize,
    /// True if the containing node has a chained method call (`.foo`) after close brace.
    pub is_chained: bool,
    /// True if the containing node is a direct argument of a send-type call.
    pub is_argument: bool,
    /// For the heredoc-argument-method-chain correction:
    /// If the first argument is a heredoc AND there is a chained method call immediately
    /// after the close brace, this is `Some((chain_start, chain_end))` where chain_start/end
    /// are the byte offsets of `.do_something` (or `&.do_something`).
    pub heredoc_chain: Option<(usize, usize)>,
}

// ── Byte-level helpers ──────────────────────────────────────────────────────

fn line_of(src: &str, offset: usize) -> usize {
    1 + src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

pub fn opening_on_same_line(src: &str, open_start: usize, first_child_start: usize) -> bool {
    line_of(src, open_start) == line_of(src, first_child_start)
}

pub fn closing_on_same_line(src: &str, close_start: usize, last_child_end: usize) -> bool {
    let last_byte = last_child_end.saturating_sub(1);
    line_of(src, last_byte) == line_of(src, close_start)
}

/// Advance `pos` past any trailing comma (skipping spaces/tabs only, not newlines).
/// Returns the position after the comma, or `pos` if none found.
fn skip_trailing_comma(src: &[u8], pos: usize) -> usize {
    let mut p = pos;
    while p < src.len() && (src[p] == b' ' || src[p] == b'\t') {
        p += 1;
    }
    if p < src.len() && src[p] == b',' { p + 1 } else { pos }
}

/// Find position of `#` on the same line as `from_offset`, searching forward.
/// Returns `None` if no `#` before end-of-line.
fn find_comment_on_line(src: &[u8], from_offset: usize) -> Option<usize> {
    let mut p = from_offset;
    while p < src.len() && src[p] != b'\n' {
        if src[p] == b'#' {
            return Some(p);
        }
        p += 1;
    }
    None
}

/// Return end-of-line offset (inclusive of the `\n`, exclusive of next char).
fn end_of_line(src: &[u8], pos: usize) -> usize {
    let mut p = pos;
    while p < src.len() && src[p] != b'\n' {
        p += 1;
    }
    if p < src.len() { p + 1 } else { p }
}

// ── Correction builders ─────────────────────────────────────────────────────

/// Build correction to move closing brace onto same line as last element.
///
/// Ports RuboCop's `correct_next_line_brace` from `MultilineLiteralBraceCorrector`.
///
/// Strategy:
/// - If last element has a trailing comment AND node is chained/argument: skip (no correction).
/// - No-comment: single replace `src[end_range..close_end]` → brace_char.
///   (Removes the `\n<whitespace>` between last child and close brace.)
/// - Comment: single replace `src[end_range..eol_of_close_line]` →
///   `close_content_without_newline + comment_text + "\n"`.
///   close_content = everything from close_start to end-of-close-line (excl `\n`).
///   This atomically removes the comment from the last-elem line and moves it after the braces.
fn correct_to_same_line(
    src: &[u8],
    close_start: usize,
    close_end: usize,
    last_child_end: usize,
    is_chained: bool,
    is_argument: bool,
    heredoc_chain: Option<(usize, usize)>,
) -> Option<Correction> {
    let end_range = skip_trailing_comma(src, last_child_end);
    let brace_char = std::str::from_utf8(&src[close_start..close_end]).unwrap_or(")");

    // Look for comment on last-element's last line (starting from last_child_end - 1).
    let last_elem_last_byte = last_child_end.saturating_sub(1);
    let comment_pos = find_comment_on_line(src, last_elem_last_byte);

    // new_line_needed_before_closing_brace? — emit offense but no correction.
    if comment_pos.is_some() && (is_chained || is_argument) {
        return None;
    }

    if let Some((chain_start, chain_end)) = heredoc_chain {
        // Heredoc-argument-method-chain correction (RuboCop's correct_heredoc_argument_method_chain).
        // The first arg is a heredoc whose body appears between end_range and close_start.
        // We must NOT delete that body; instead:
        //   Edit A: Insert brace_char + chain_source at end_range
        //   Edit B: Remove the whole close-brace-line (close_start..eol_of_close), which contains
        //           the close brace + chain.
        let chain_source = std::str::from_utf8(&src[chain_start..chain_end]).unwrap_or("");
        let eol_close = end_of_line(src, close_start);
        return Some(Correction {
            edits: vec![
                Edit {
                    start_offset: end_range,
                    end_offset: end_range,
                    replacement: format!("{}{}", brace_char, chain_source),
                },
                Edit {
                    start_offset: close_start,
                    end_offset: eol_close,
                    replacement: String::new(),
                },
            ],
        });
    }

    if let Some(cpos) = comment_pos {
        // Comment present on last-element line.
        // Single replace: src[end_range .. eol_of_close_line] →
        //   close_content_without_newline + comment_text + "\n"
        //
        // close_content = src[close_start..eol_of_close_line] includes everything on close-brace
        // line plus the trailing \n.  We strip that \n and append comment_text + \n instead.
        let eol_close = end_of_line(src, close_start);
        // close_content without trailing \n
        let close_no_nl_end = if eol_close > close_start && src[eol_close - 1] == b'\n' {
            eol_close - 1
        } else {
            eol_close
        };
        let close_content = std::str::from_utf8(&src[close_start..close_no_nl_end])
            .unwrap_or(brace_char);

        // comment_text = everything from space before '#' to end of last-elem line (excl \n).
        let space_start = if cpos > 0 && src[cpos - 1] == b' ' { cpos - 1 } else { cpos };
        let eol_last = end_of_line(src, cpos);
        let comment_no_nl_end = if eol_last > 0 && src[eol_last - 1] == b'\n' {
            eol_last - 1
        } else {
            eol_last
        };
        let comment_text = std::str::from_utf8(&src[space_start..comment_no_nl_end])
            .unwrap_or("");

        let replacement = format!("{}{}\n", close_content, comment_text);
        Some(Correction {
            edits: vec![Edit {
                start_offset: end_range,
                end_offset: eol_close,
                replacement,
            }],
        })
    } else {
        // No comment: simple replace — delete everything between end_range and close_end,
        // inserting just the brace char. This removes the "\n<whitespace>" before close.
        Some(Correction {
            edits: vec![Edit {
                start_offset: end_range,
                end_offset: close_end,
                replacement: brace_char.to_string(),
            }],
        })
    }
}

/// Build correction to move closing brace to a new line (insert `\n` before it).
fn correct_to_new_line(close_start: usize) -> Correction {
    Correction::insert(close_start, "\n".to_string())
}

// ── Main check function ──────────────────────────────────────────────────────

/// Check brace layout for a literal and produce offense + correction if violated.
///
/// Caller must have already filtered out:
/// - implicit literals (no opening brace)
/// - empty literals (no children)
/// - single-line literals
/// - literals whose last child contains a trailing heredoc
pub fn check(ctx: &CheckContext, params: &BraceCheck) -> Vec<Offense> {
    let src = ctx.source;
    let bytes = src.as_bytes();
    let opening_same = opening_on_same_line(src, params.open_start, params.first_child_start);
    let closing_same = closing_on_same_line(src, params.close_start, params.last_child_end);

    let (message, needs_same_line) = match params.style {
        BraceLayoutStyle::Symmetrical => {
            if opening_same {
                if closing_same { return vec![]; }
                (params.messages.same_line, true)
            } else {
                if !closing_same { return vec![]; }
                (params.messages.new_line, false)
            }
        }
        BraceLayoutStyle::NewLine => {
            if !closing_same { return vec![]; }
            (params.messages.always_new_line, false)
        }
        BraceLayoutStyle::SameLine => {
            if closing_same { return vec![]; }
            (params.messages.always_same_line, true)
        }
    };

    let correction = if needs_same_line {
        correct_to_same_line(
            bytes,
            params.close_start,
            params.close_end,
            params.last_child_end,
            params.is_chained,
            params.is_argument,
            params.heredoc_chain,
        )
    } else {
        Some(correct_to_new_line(params.close_start))
    };

    let mut offense = ctx.offense_with_range(
        params.cop_name,
        message,
        Severity::Convention,
        params.close_start,
        params.close_end,
    );
    if let Some(corr) = correction {
        offense = offense.with_correction(corr);
    }
    vec![offense]
}

// ── Heredoc detection ────────────────────────────────────────────────────────

/// Detect if `last_child` (or its descendants) contains a heredoc whose terminator
/// falls on the outermost-parent's last line or later.
pub fn last_line_heredoc(src: &str, last_child: &Node, parent_last_line: usize) -> bool {
    let mut finder = HeredocFinder { src, parent_last_line, found: false };
    finder.visit(last_child);
    finder.found
}

struct HeredocFinder<'a> {
    src: &'a str,
    parent_last_line: usize,
    found: bool,
}

impl HeredocFinder<'_> {
    fn check_heredoc(&mut self, opening_text_start: usize, closing_end: usize) {
        if opening_text_start + 2 > self.src.len() { return; }
        let bytes = self.src.as_bytes();
        if bytes[opening_text_start] != b'<' || bytes[opening_text_start + 1] != b'<' { return; }
        let last_byte = closing_end.saturating_sub(1);
        let heredoc_end_line = line_of(self.src, last_byte);
        if heredoc_end_line >= self.parent_last_line {
            self.found = true;
        }
    }
}

impl Visit<'_> for HeredocFinder<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if self.found { return; }
        if let (Some(open), Some(close)) = (node.opening_loc(), node.closing_loc()) {
            self.check_heredoc(open.start_offset(), close.end_offset());
        }
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if self.found { return; }
        if let (Some(open), Some(close)) = (node.opening_loc(), node.closing_loc()) {
            self.check_heredoc(open.start_offset(), close.end_offset());
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode) {
        if self.found { return; }
        let open = node.opening_loc();
        let close = node.closing_loc();
        self.check_heredoc(open.start_offset(), close.end_offset());
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        if self.found { return; }
        let open = node.opening_loc();
        let close = node.closing_loc();
        self.check_heredoc(open.start_offset(), close.end_offset());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}
