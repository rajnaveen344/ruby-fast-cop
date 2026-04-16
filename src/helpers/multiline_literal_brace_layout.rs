//! Shared helper for the MultilineLiteralBraceLayout mixin.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/mixin/multiline_literal_brace_layout.rb
//!
//! Used by:
//! - Layout/MultilineArrayBraceLayout
//! - Layout/MultilineHashBraceLayout
//! - Layout/MultilineMethodCallBraceLayout

use crate::cops::CheckContext;
use crate::offense::{Offense, Severity};
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
    /// First child byte range.
    pub first_child_start: usize,
    /// Last child end offset (end_offset of last child node).
    pub last_child_end: usize,
}

/// Compute line number (1-indexed) for a byte offset.
fn line_of(src: &str, offset: usize) -> usize {
    1 + src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// Opening brace on same line as first child.
pub fn opening_on_same_line(src: &str, open_start: usize, first_child_start: usize) -> bool {
    line_of(src, open_start) == line_of(src, first_child_start)
}

/// Closing brace on same line as last child end.
pub fn closing_on_same_line(src: &str, close_start: usize, last_child_end: usize) -> bool {
    // RuboCop uses last_child.last_line which is the line of the last byte of the node.
    // last_child_end is the exclusive end offset; the last byte is at last_child_end - 1.
    let last_byte = last_child_end.saturating_sub(1);
    line_of(src, last_byte) == line_of(src, close_start)
}

/// Check brace layout for a literal and produce offense if violated.
///
/// Caller must have already filtered out:
/// - implicit literals (no opening brace)
/// - empty literals (no children)
/// - single-line literals
/// - literals whose last child contains a trailing heredoc
pub fn check(ctx: &CheckContext, params: &BraceCheck) -> Vec<Offense> {
    let src = ctx.source;
    let opening_same = opening_on_same_line(src, params.open_start, params.first_child_start);
    let closing_same = closing_on_same_line(src, params.close_start, params.last_child_end);

    let message = match params.style {
        BraceLayoutStyle::Symmetrical => {
            if opening_same {
                if closing_same {
                    return vec![];
                }
                params.messages.same_line
            } else {
                if !closing_same {
                    return vec![];
                }
                params.messages.new_line
            }
        }
        BraceLayoutStyle::NewLine => {
            if !closing_same {
                return vec![];
            }
            params.messages.always_new_line
        }
        BraceLayoutStyle::SameLine => {
            if closing_same {
                return vec![];
            }
            params.messages.always_same_line
        }
    };

    vec![ctx.offense_with_range(
        params.cop_name,
        message,
        Severity::Convention,
        params.close_start,
        params.close_end,
    )]
}

/// Detect if `last_child` (or its descendants) contains a heredoc
/// whose terminator falls on the outermost-parent's last line or later.
///
/// `parent_last_line` = 1-indexed line of the outermost literal being checked.
pub fn last_line_heredoc(src: &str, last_child: &Node, parent_last_line: usize) -> bool {
    let mut finder = HeredocFinder {
        src,
        parent_last_line,
        found: false,
    };
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
        // Heredoc openings start with "<<" (possibly "<<-", "<<~", etc).
        if opening_text_start + 2 > self.src.len() {
            return;
        }
        let bytes = self.src.as_bytes();
        if bytes[opening_text_start] != b'<' || bytes[opening_text_start + 1] != b'<' {
            return;
        }
        // heredoc_end line = line of last byte of closing_loc
        let last_byte = closing_end.saturating_sub(1);
        let heredoc_end_line = line_of(self.src, last_byte);
        if heredoc_end_line >= self.parent_last_line {
            self.found = true;
        }
    }
}

impl Visit<'_> for HeredocFinder<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if self.found {
            return;
        }
        if let (Some(open), Some(close)) = (node.opening_loc(), node.closing_loc()) {
            self.check_heredoc(open.start_offset(), close.end_offset());
        }
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if self.found {
            return;
        }
        if let (Some(open), Some(close)) = (node.opening_loc(), node.closing_loc()) {
            self.check_heredoc(open.start_offset(), close.end_offset());
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode) {
        if self.found {
            return;
        }
        let open = node.opening_loc();
        let close = node.closing_loc();
        self.check_heredoc(open.start_offset(), close.end_offset());
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        if self.found {
            return;
        }
        let open = node.opening_loc();
        let close = node.closing_loc();
        self.check_heredoc(open.start_offset(), close.end_offset());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}
