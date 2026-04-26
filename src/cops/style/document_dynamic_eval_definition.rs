//! Style/DocumentDynamicEvalDefinition - Require comment docs for interpolated `eval` calls.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/document_dynamic_eval_definition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};
use crate::node_name;
use regex::Regex;
use ruby_prism::Node;

const MSG: &str = "Add a comment block showing its appearance if interpolated.";
const EVAL_METHODS: &[&str] = &["eval", "class_eval", "module_eval", "instance_eval"];

#[derive(Default)]
pub struct DocumentDynamicEvalDefinition;

impl DocumentDynamicEvalDefinition {
    pub fn new() -> Self { Self }
}

impl Cop for DocumentDynamicEvalDefinition {
    fn name(&self) -> &'static str { "Style/DocumentDynamicEvalDefinition" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if !EVAL_METHODS.contains(&method.as_ref()) {
            return vec![];
        }
        let Some(args) = node.arguments() else { return vec![]; };
        let mut iter = args.arguments().iter();
        let Some(first_arg) = iter.next() else { return vec![]; };

        let Some(istr) = first_arg.as_interpolated_string_node() else { return vec![]; };

        // Collect embedded statements (interpolations).
        let mut embeds: Vec<Node> = Vec::new();
        for part in istr.parts().iter() {
            if matches!(part, Node::EmbeddedStatementsNode { .. }) {
                embeds.push(part);
            }
        }
        if embeds.is_empty() {
            return vec![];
        }

        // Inline comment docs check: every embed's source line contains `#` not followed by `{`.
        let inline_ok = embeds.iter().all(|e| {
            let line_text = source_line_for(ctx.source, e.location().start_offset());
            line_has_non_interp_hash(line_text)
        });
        if inline_ok {
            return vec![];
        }

        // Heredoc check
        let is_heredoc = istr.opening_loc()
            .map(|l| {
                let s = &ctx.source[l.start_offset()..l.end_offset()];
                s.starts_with("<<")
            })
            .unwrap_or(false);

        if is_heredoc && comment_block_docs(ctx.source, &istr, &embeds, node) {
            return vec![];
        }

        // Offense at selector
        let sel = node.message_loc().unwrap_or(node.location());
        let location = Location::from_offsets(ctx.source, sel.start_offset(), sel.end_offset());
        vec![Offense::new(
            "Style/DocumentDynamicEvalDefinition",
            MSG,
            Severity::Convention,
            location,
            ctx.filename,
        )]
    }
}

/// Get the entire line text containing the given byte offset.
fn source_line_for(source: &str, offset: usize) -> &str {
    let bytes = source.as_bytes();
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let mut end = offset;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    &source[start..end]
}

/// Check if comments (in heredoc body or preceding the heredoc) match the
/// eval'd content shape.
fn comment_block_docs(
    source: &str,
    istr: &ruby_prism::InterpolatedStringNode,
    embeds: &[Node],
    call_node: &ruby_prism::CallNode,
) -> bool {
    // Get heredoc body line range. Heredoc body = content between opening
    // line and closing tag.
    let opening = istr.opening_loc();
    let closing = istr.closing_loc();
    let (Some(opening), Some(closing)) = (opening, closing) else { return false; };
    let _ = opening;

    // Heredoc body spans from line after opener-line to line before closing tag.
    // istr.location() encompasses opener position; the body lives between the
    // line after the opener and the line of the closing tag.
    let body_start_line = line_at_offset(source, istr.location().end_offset()) + 1;
    // Actually: location.end is end of opener tag like `<<-EOT`. Body starts
    // on the next line. closing.start = column of closing tag (last line).
    let body_end_line = line_at_offset(source, closing.start_offset()).saturating_sub(1);

    // Wait — for InterpolatedStringNode heredoc, parts include the body
    // strings; closing_loc is the closing tag. We can compute body lines from
    // parts' first/last offsets too. Use a simpler approach:
    // - body_start_line = line of first part
    // - body_end_line = line of last part (or one before closing)
    let parts_first = istr.parts().iter().next().map(|p| line_at_offset(source, p.location().start_offset()));
    let parts_last_end = istr.parts().iter().last().map(|p| line_at_offset(source, p.location().end_offset()));
    let body_start_line = parts_first.unwrap_or(body_start_line);
    let body_end_line = parts_last_end.unwrap_or(body_end_line);

    // Collect heredoc body lines.
    let mut blocks = collect_comment_blocks_in_lines(source, body_start_line, body_end_line);

    // Collect comments preceding (in the call's source range, but outside heredoc body).
    // Call expression spans from call.start to call.end. Comments outside heredoc body but inside call range count.
    let call_start_line = line_at_offset(source, call_node.location().start_offset());
    let call_end_line = line_at_offset(source, call_node.location().end_offset());
    let outside_blocks = collect_comment_blocks_outside_range(
        source,
        call_start_line,
        call_end_line,
        body_start_line,
        body_end_line,
    );
    blocks.extend(outside_blocks);

    if blocks.is_empty() {
        return false;
    }

    // Build a regex from istr parts: literals → escaped + flexible whitespace,
    // interpolations → `.+`.
    let regex = match build_arg_regex(source, istr, embeds) {
        Some(r) => r,
        None => return false,
    };

    // Check if any single block matches OR concatenation matches.
    if blocks.iter().any(|b| regex.is_match(b)) {
        return true;
    }
    let joined = blocks.join("");
    regex.is_match(&joined)
}

/// Collect comment blocks in a line range (1-indexed inclusive).
/// Adjacent `#`-prefixed lines are joined with `\n`.
fn collect_comment_blocks_in_lines(source: &str, start_line: usize, end_line: usize) -> Vec<String> {
    if start_line > end_line {
        return vec![];
    }
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let block_re = Regex::new(r"^\s*#").unwrap();

    let mut blocks: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut last_idx: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1; // 1-indexed
        if line_num < start_line { continue; }
        if line_num > end_line { break; }
        let line_str = line.trim_end_matches('\n');
        if let Some(m) = block_re.find(line_str) {
            // Reject if `#` is followed by `{` (interpolation).
            let after = &line_str[m.end()..];
            if after.starts_with('{') {
                if let Some(prev) = current.take() { blocks.push(prev); }
                last_idx = None;
                continue;
            }
            let stripped = after;
            match (&mut current, last_idx) {
                (Some(buf), Some(li)) if li + 1 == line_num => {
                    buf.push('\n');
                    buf.push_str(stripped);
                }
                _ => {
                    if let Some(prev) = current.take() {
                        blocks.push(prev);
                    }
                    current = Some(stripped.to_string());
                }
            }
            last_idx = Some(line_num);
        }
    }
    if let Some(prev) = current.take() {
        blocks.push(prev);
    }
    blocks
}

/// Collect comment blocks in [outer_start..outer_end], skipping [inner_start..inner_end].
fn collect_comment_blocks_outside_range(
    source: &str,
    outer_start: usize,
    outer_end: usize,
    inner_start: usize,
    inner_end: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    if outer_start < inner_start {
        out.extend(collect_comment_blocks_in_lines(source, outer_start, inner_start - 1));
    }
    if inner_end < outer_end {
        out.extend(collect_comment_blocks_in_lines(source, inner_end + 1, outer_end));
    }
    out
}

/// Build a regex from the interpolated string parts. Interpolations → `.+`,
/// literal text → escaped with comment-stripping.
fn build_arg_regex(
    source: &str,
    istr: &ruby_prism::InterpolatedStringNode,
    _embeds: &[Node],
) -> Option<Regex> {
    let mut pat = String::new();
    for part in istr.parts().iter() {
        if matches!(part, Node::EmbeddedStatementsNode { .. }) {
            pat.push_str(".+");
        } else {
            // Literal: get source, strip comments. Then split into lines and
            // emit `\s*<escaped>` per non-blank line (mimicking parser's
            // per-line str-node splitting in RuboCop).
            let loc = part.location();
            let src = &source[loc.start_offset()..loc.end_offset()];
            let cleaned = strip_comment_not_interp(src);
            let mut prev_blank = false;
            for line in cleaned.lines() {
                let t = line.trim();
                if t.is_empty() {
                    if !prev_blank {
                        pat.push_str(r"\s+");
                        prev_blank = true;
                    }
                } else {
                    pat.push_str(r"\s*");
                    pat.push_str(&regex::escape(t));
                    prev_blank = false;
                }
            }
        }
    }
    let full = format!("(?s){}", pat);
    Regex::new(&full).ok()
}

/// True if the line contains a `#` not followed by `{`.
fn line_has_non_interp_hash(line: &str) -> bool {
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'#' && (i + 1 >= bytes.len() || bytes[i + 1] != b'{') {
            return true;
        }
    }
    false
}

/// Strip `# ...` comments, but skip `#{` (interpolation marker).
fn strip_comment_not_interp(src: &str) -> String {
    let mut out = String::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' && (i + 1 >= bytes.len() || bytes[i + 1] != b'{') {
            // Skip rest of line up to newline (consume comment).
            // Also strip preceding whitespace.
            while !out.is_empty() && out.as_bytes().last().map_or(false, |b| *b == b' ' || *b == b'\t') {
                out.pop();
            }
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn line_at_offset(source: &str, offset: usize) -> usize {
    let mut line = 1usize;
    for (i, b) in source.as_bytes().iter().enumerate() {
        if i >= offset { break; }
        if *b == b'\n' { line += 1; }
    }
    line
}

crate::register_cop!("Style/DocumentDynamicEvalDefinition", |_cfg| Some(Box::new(
    DocumentDynamicEvalDefinition::new()
)));
