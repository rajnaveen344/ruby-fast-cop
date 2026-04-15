//! Shared logic for Style/TrailingCommaIn{Arguments,ArrayLiteral,HashLiteral}.
//!
//! Mirrors RuboCop's `RuboCop::Cop::Style::TrailingComma` mixin
//! (lib/rubocop/cop/mixin/trailing_comma.rb) plus the per-cop messages.

use crate::cops::CheckContext;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnforcedStyleForMultiline {
    NoComma,
    Comma,
    ConsistentComma,
    DiffComma,
}

/// Per-cop labels used to build offense messages and noun phrasing.
#[derive(Clone, Copy)]
pub struct TrailingCommaKind {
    pub cop_name: &'static str,
    /// long noun used in "Avoid comma after the last X." — e.g. "parameter of a method call",
    /// "item of an array", "item of a hash"
    pub item_noun: &'static str,
    /// short noun used in "Put a comma after the last X of a multiline Y." — e.g. "parameter", "item"
    pub short_noun: &'static str,
    /// container noun used in "Put a comma after the last X of a multiline Y."
    pub container_noun: &'static str,
}

pub const ARGS: TrailingCommaKind = TrailingCommaKind {
    cop_name: "Style/TrailingCommaInArguments",
    item_noun: "parameter of a method call",
    short_noun: "parameter",
    container_noun: "method call",
};
pub const ARRAY: TrailingCommaKind = TrailingCommaKind {
    cop_name: "Style/TrailingCommaInArrayLiteral",
    item_noun: "item of an array",
    short_noun: "item",
    container_noun: "array",
};
pub const HASH: TrailingCommaKind = TrailingCommaKind {
    cop_name: "Style/TrailingCommaInHashLiteral",
    item_noun: "item of a hash",
    short_noun: "item",
    container_noun: "hash",
};

/// Core check. Returns offenses (with corrections) for one container literal
/// or call argument list.
///
/// * `items`               — logical elements (args, array elements, or hash assocs)
/// * `open_offset`         — byte offset of `(` or `[` or `{` (or delimiter)
/// * `close_offset`        — byte offset of matching close delimiter
/// * `last_is_block_pass`  — if true (block-pass last arg), skip comma enforcement
pub fn check(
    ctx: &CheckContext,
    kind: TrailingCommaKind,
    style: EnforcedStyleForMultiline,
    items: &[Node],
    open_offset: usize,
    close_offset: usize,
    last_is_block_pass: bool,
) -> Vec<Offense> {
    if items.is_empty() {
        return vec![];
    }
    let last = &items[items.len() - 1];
    let source = ctx.source;

    // For array/hash literals the trailing comma after the last item always
    // appears between the item's raw syntactic end and the closing bracket
    // (any heredoc body is inside an element's source range, not after it).
    let search_start = literal_item_search_start(last, source);
    let any_heredoc = items.iter().any(|i| contains_heredoc(i, source));
    let trailing_comma_pos = if any_heredoc {
        find_trailing_comma_no_newlines(source, search_start, close_offset)
    } else {
        find_trailing_comma(source, search_start, close_offset)
    };

    let should_have = should_have_comma(
        style, ctx, items, last, open_offset, close_offset, source,
    );

    if let Some(c) = trailing_comma_pos {
        // Comma present.
        if should_have {
            return vec![];
        }
        let msg = format!(
            "Avoid comma after the last {}{}.",
            kind.item_noun,
            extra_avoid_info(style),
        );
        let o = ctx
            .offense_with_range(kind.cop_name, &msg, Severity::Convention, c, c + 1)
            .with_correction(Correction::delete(c, c + 1));
        vec![o]
    } else if should_have && !last_is_block_pass {
        missing_comma_offense(ctx, kind, last, source)
    } else {
        vec![]
    }
}

fn extra_avoid_info(style: EnforcedStyleForMultiline) -> &'static str {
    match style {
        EnforcedStyleForMultiline::NoComma => "",
        EnforcedStyleForMultiline::Comma => ", unless each item is on its own line",
        EnforcedStyleForMultiline::ConsistentComma => {
            ", unless items are split onto multiple lines"
        }
        EnforcedStyleForMultiline::DiffComma => {
            ", unless that item immediately precedes a newline"
        }
    }
}

/// Mirror of RuboCop's `should_have_comma?(style, node)`.
fn should_have_comma(
    style: EnforcedStyleForMultiline,
    ctx: &CheckContext,
    items: &[Node],
    last: &Node,
    open_offset: usize,
    close_offset: usize,
    source: &str,
) -> bool {
    if !is_multiline(ctx, items, last, open_offset, close_offset, source) {
        return false;
    }
    match style {
        EnforcedStyleForMultiline::NoComma => false,
        EnforcedStyleForMultiline::Comma => no_elements_on_same_line(ctx, items, close_offset, source),
        EnforcedStyleForMultiline::ConsistentComma => {
            // For array/hash literals there is no "method name and arguments
            // on same line" concept — so always require a comma once multiline.
            // For arguments, we treat last arg being a braced hash whose closing
            // `}` is on the same line as the call's close as "same line".
            !braced_hash_last_arg_on_close_line(last, close_offset, ctx, source)
        }
        EnforcedStyleForMultiline::DiffComma => last_item_precedes_newline(last, close_offset, source),
    }
}

/// Mirrors RuboCop's `multiline?(node)` = `node.multiline? && !allowed_multiline_argument?`.
fn is_multiline(
    ctx: &CheckContext,
    items: &[Node],
    _last: &Node,
    open_offset: usize,
    close_offset: usize,
    _source: &str,
) -> bool {
    if ctx.same_line(open_offset, close_offset) {
        return false;
    }
    // allowed_multiline_argument: single element, and the closing bracket
    // does not begin its line.
    if items.len() == 1 && !ctx.begins_its_line(close_offset) {
        return false;
    }
    true
}

/// Consecutive items (+ close_loc) must not share any line.
fn no_elements_on_same_line(
    ctx: &CheckContext,
    items: &[Node],
    close_offset: usize,
    source: &str,
) -> bool {
    let mut last_line: Option<usize> = None;
    for item in items {
        let start = item.location().start_offset();
        let end = effective_end_offset(item, source);
        let start_line = ctx.line_of(start);
        let end_line = end_line_of(ctx, end);
        if let Some(ll) = last_line {
            if ll == start_line {
                return false;
            }
        }
        last_line = Some(end_line);
    }
    let close_line = ctx.line_of(close_offset);
    if let Some(ll) = last_line {
        if ll == close_line {
            return false;
        }
    }
    true
}

/// Line of the last consumed byte in a half-open range ending at `end`.
/// Accounts for the common Prism convention where `end_offset` points just
/// past a trailing newline (e.g. heredoc `closing_loc`).
fn end_line_of(ctx: &CheckContext, end: usize) -> usize {
    if end == 0 {
        return 1;
    }
    ctx.line_of(end - 1)
}

fn braced_hash_last_arg_on_close_line(
    last: &Node,
    close_offset: usize,
    ctx: &CheckContext,
    source: &str,
) -> bool {
    if !matches!(last, Node::HashNode { .. }) {
        return false;
    }
    let last_end = effective_end_offset(last, source);
    ctx.same_line(last_end, close_offset)
}

fn last_item_precedes_newline(last: &Node, close_offset: usize, source: &str) -> bool {
    // After the last item, optionally skip one comma, whitespace and an
    // optional comment, and check that a newline follows before close.
    // Use the syntactic end (before any heredoc body) so e.g. `<<-HEREDOC,`
    // on the same line as the heredoc opener is correctly identified.
    let start = literal_item_search_start(last, source);
    let slice = &source[start..close_offset];
    let bytes = slice.as_bytes();
    let mut i = 0;
    if i < bytes.len() && bytes[i] == b',' {
        i += 1;
    }
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'#' {
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
    }
    i < bytes.len() && bytes[i] == b'\n'
}

fn missing_comma_offense(
    ctx: &CheckContext,
    kind: TrailingCommaKind,
    last: &Node,
    source: &str,
) -> Vec<Offense> {
    let msg = format!(
        "Put a comma after the last {} of a multiline {}.",
        kind.short_noun, kind.container_noun
    );
    let (offense_start, offense_end) = offense_range(last, source);
    let insert_pos = insertion_point_for_comma(last, source);
    let o = ctx
        .offense_with_range(
            kind.cop_name,
            &msg,
            Severity::Convention,
            offense_start,
            offense_end,
        )
        .with_correction(Correction::insert(insert_pos, ","));
    vec![o]
}

// ─── Node-walking helpers (shared) ─────────────────────────────────────

/// Where to start scanning for a trailing comma after the last item of an
/// array or hash literal. Unlike `trailing_comma_search_start` (which is for
/// argument lists and skips past heredoc bodies), this uses the item's
/// syntactic end so the comma between e.g. `<<-HEREDOC.chomp` and the
/// following newline/heredoc body is found.
fn literal_item_search_start(node: &Node, source: &str) -> usize {
    match node {
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            literal_item_search_start(&n.value(), source)
        }
        _ => node.location().end_offset(),
    }
}

/// Where to start scanning for a trailing comma after `last`.
pub fn trailing_comma_search_start(node: &Node, source: &str) -> usize {
    match node {
        Node::InterpolatedStringNode { .. } | Node::StringNode { .. } => {
            effective_end_offset(node, source)
        }
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                if contains_heredoc(&receiver, source) {
                    if let Some(close_loc) = call.closing_loc() {
                        return close_loc.end_offset();
                    }
                    return effective_end_offset(node, source);
                }
            }
            call.location().end_offset()
        }
        Node::KeywordHashNode { .. } => {
            let n = node.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = n.elements().iter().collect();
            if let Some(last) = elements.last() {
                return trailing_comma_search_start(last, source);
            }
            node.location().end_offset()
        }
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            trailing_comma_search_start(&n.value(), source)
        }
        _ => node.location().end_offset(),
    }
}

/// End offset accounting for heredoc bodies.
pub fn effective_end_offset(node: &Node, source: &str) -> usize {
    match node {
        Node::InterpolatedStringNode { .. } => {
            let n = node.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                if source[s..e].starts_with("<<") {
                    if let Some(close_loc) = n.closing_loc() {
                        return close_loc.end_offset();
                    }
                }
            }
            n.location().end_offset()
        }
        Node::StringNode { .. } => {
            let n = node.as_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                if source[s..e].starts_with("<<") {
                    if let Some(close_loc) = n.closing_loc() {
                        return close_loc.end_offset();
                    }
                }
            }
            n.location().end_offset()
        }
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                if contains_heredoc(&receiver, source) {
                    let recv_end = effective_end_offset(&receiver, source);
                    let call_end = call.location().end_offset();
                    return std::cmp::max(recv_end, call_end);
                }
            }
            call.location().end_offset()
        }
        Node::KeywordHashNode { .. } => {
            let n = node.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = n.elements().iter().collect();
            if let Some(last) = elements.last() {
                return effective_end_offset(last, source);
            }
            n.location().end_offset()
        }
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            let value_end = effective_end_offset(&n.value(), source);
            let node_end = n.location().end_offset();
            std::cmp::max(value_end, node_end)
        }
        _ => node.location().end_offset(),
    }
}

fn contains_heredoc(node: &Node, source: &str) -> bool {
    match node {
        Node::InterpolatedStringNode { .. } => {
            let n = node.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                return source[s..e].starts_with("<<");
            }
            false
        }
        Node::StringNode { .. } => {
            let n = node.as_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                return source[s..e].starts_with("<<");
            }
            false
        }
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                return contains_heredoc(&receiver, source);
            }
            false
        }
        Node::KeywordHashNode { .. } => {
            let n = node.as_keyword_hash_node().unwrap();
            for elem in n.elements().iter() {
                if contains_heredoc(&elem, source) {
                    return true;
                }
            }
            false
        }
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            contains_heredoc(&n.value(), source)
        }
        _ => false,
    }
}

/// Scan for a trailing comma between `search_start` and `close_offset`,
/// skipping whitespace and line comments.
/// Like `find_trailing_comma` but stops at the first newline. Used when a
/// heredoc body lives between the last item and the closing bracket — we must
/// not treat a comma inside the heredoc body as the list's trailing comma.
fn find_trailing_comma_no_newlines(
    source: &str,
    search_start: usize,
    close_offset: usize,
) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = search_start;
    while i < close_offset {
        match bytes[i] {
            b' ' | b'\t' | b'\r' => i += 1,
            b'\n' => return None,
            b',' => return Some(i),
            _ => return None,
        }
    }
    None
}

pub fn find_trailing_comma(source: &str, search_start: usize, close_offset: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = search_start;
    while i < close_offset {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'#' => {
                while i < close_offset && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b',' => return Some(i),
            _ => return None,
        }
    }
    None
}

fn has_multiple_items_on_same_line(items: &[Node], ctx: &CheckContext) -> bool {
    let mut lines: Vec<usize> = Vec::new();
    for item in items {
        match item {
            Node::KeywordHashNode { .. } => {
                let kh = item.as_keyword_hash_node().unwrap();
                for elem in kh.elements().iter() {
                    lines.push(ctx.line_of(elem.location().start_offset()));
                }
            }
            _ => {
                lines.push(ctx.line_of(item.location().start_offset()));
            }
        }
    }
    for i in 1..lines.len() {
        if lines[i] == lines[i - 1] {
            return true;
        }
    }
    false
}

fn is_multiline_single_arg_needing_comma(
    arg: &Node,
    ctx: &CheckContext,
    close_offset: usize,
) -> bool {
    match arg {
        Node::KeywordHashNode { .. } => {
            let kh = arg.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = kh.elements().iter().collect();
            if elements.len() >= 2 {
                return !ctx.same_line(
                    elements[0].location().start_offset(),
                    elements.last().unwrap().location().start_offset(),
                );
            }
            false
        }
        Node::HashNode { .. } => false,
        _ => !ctx.same_line(arg.location().end_offset(), close_offset),
    }
}

pub fn offense_range(last: &Node, source: &str) -> (usize, usize) {
    match last {
        Node::KeywordHashNode { .. } => {
            let kh = last.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = kh.elements().iter().collect();
            if let Some(l) = elements.last() {
                return offense_range(l, source);
            }
            (last.location().start_offset(), last.location().end_offset())
        }
        Node::CallNode { .. } => {
            let call = last.as_call_node().unwrap();
            (call.location().start_offset(), call.location().end_offset())
        }
        Node::InterpolatedStringNode { .. } => {
            let n = last.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                if source[s..e].starts_with("<<") {
                    return (s, e);
                }
            }
            (n.location().start_offset(), n.location().end_offset())
        }
        Node::StringNode { .. } => {
            let n = last.as_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                if source[s..e].starts_with("<<") {
                    return (s, e);
                }
            }
            (n.location().start_offset(), n.location().end_offset())
        }
        _ => (last.location().start_offset(), last.location().end_offset()),
    }
}

fn is_braced_hash_arg(node: &Node) -> bool {
    matches!(node, Node::HashNode { .. })
}

pub fn insertion_point_for_comma(last: &Node, source: &str) -> usize {
    match last {
        Node::KeywordHashNode { .. } => {
            let kh = last.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = kh.elements().iter().collect();
            if let Some(l) = elements.last() {
                return insertion_point_for_comma(l, source);
            }
            last.location().end_offset()
        }
        Node::AssocNode { .. } => {
            let n = last.as_assoc_node().unwrap();
            insertion_point_for_comma(&n.value(), source)
        }
        Node::CallNode { .. } => {
            let call = last.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                if contains_heredoc(&receiver, source) {
                    if let Some(close_loc) = call.closing_loc() {
                        return close_loc.end_offset();
                    }
                    if let Some(args) = call.arguments() {
                        let a: Vec<Node> = args.arguments().iter().collect();
                        if let Some(l) = a.last() {
                            return l.location().end_offset();
                        }
                    }
                    if let Some(msg_loc) = call.message_loc() {
                        return msg_loc.end_offset();
                    }
                }
            }
            call.location().end_offset()
        }
        Node::InterpolatedStringNode { .. } => {
            let n = last.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                if source[s..e].starts_with("<<") {
                    return e;
                }
            }
            n.location().end_offset()
        }
        Node::StringNode { .. } => {
            let n = last.as_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                if source[s..e].starts_with("<<") {
                    return e;
                }
            }
            n.location().end_offset()
        }
        _ => last.location().end_offset(),
    }
}
