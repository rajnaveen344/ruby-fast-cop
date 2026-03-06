use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnforcedStyleForMultiline {
    NoComma,
    Comma,
    ConsistentComma,
    DiffComma,
}

pub struct TrailingCommaInArguments {
    style: EnforcedStyleForMultiline,
}

impl TrailingCommaInArguments {
    pub fn new(style: EnforcedStyleForMultiline) -> Self {
        Self { style }
    }

    fn check_call_node(
        &self,
        node: &ruby_prism::CallNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let source = ctx.source;

        // Need arguments
        let arguments = match node.arguments() {
            Some(args) => args,
            None => return vec![],
        };

        let args: Vec<Node> = arguments.arguments().iter().collect();
        if args.is_empty() {
            return vec![];
        }

        // Need opening/closing delimiters (parens or brackets)
        let (open_offset, close_offset) = match self.find_delimiters(node, source) {
            Some(pair) => pair,
            None => return vec![],
        };

        // Check if last arg is a block pass (&block) - skip comma enforcement
        let last_arg = &args[args.len() - 1];
        let last_is_block_pass = matches!(last_arg, Node::BlockArgumentNode { .. });

        // Determine if this is a multiline call
        let is_multiline = self.is_multiline_args(&args, open_offset, close_offset, source);

        // Find trailing comma position between last arg and close delimiter.
        let search_start = trailing_comma_search_start(last_arg, source);
        let trailing_comma_pos = find_trailing_comma(source, search_start, close_offset);

        if is_multiline {
            self.check_multiline(
                ctx,
                &args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
            )
        } else {
            self.check_single_line(ctx, trailing_comma_pos)
        }
    }

    fn find_delimiters(
        &self,
        node: &ruby_prism::CallNode,
        source: &str,
    ) -> Option<(usize, usize)> {
        if let (Some(open_loc), Some(close_loc)) = (node.opening_loc(), node.closing_loc()) {
            let open_offset = open_loc.start_offset();
            let close_offset = close_loc.start_offset();
            let open_char = source.as_bytes().get(open_offset).copied();
            if matches!(open_char, Some(b'(') | Some(b'[')) {
                return Some((open_offset, close_offset));
            }
        }
        None
    }

    fn is_multiline_args(
        &self,
        _args: &[Node],
        open_offset: usize,
        close_offset: usize,
        source: &str,
    ) -> bool {
        // Multiline if the opening and closing delimiters are on different lines.
        // Heredocs within the args don't affect this - the `)` or `]` position
        // is what determines single vs multi line.
        let open_line = line_of(source, open_offset);
        let close_line = line_of(source, close_offset);
        open_line != close_line
    }

    fn check_single_line(
        &self,
        ctx: &CheckContext,
        trailing_comma_pos: Option<usize>,
    ) -> Vec<Offense> {
        if let Some(comma_offset) = trailing_comma_pos {
            let msg = self.single_line_message();
            let offense = ctx
                .offense_with_range(
                    self.name(),
                    msg,
                    Severity::Convention,
                    comma_offset,
                    comma_offset + 1,
                )
                .with_correction(Correction::delete(comma_offset, comma_offset + 1));
            vec![offense]
        } else {
            vec![]
        }
    }

    fn check_multiline(
        &self,
        ctx: &CheckContext,
        args: &[Node],
        last_arg: &Node,
        last_is_block_pass: bool,
        close_offset: usize,
        trailing_comma_pos: Option<usize>,
        source: &str,
    ) -> Vec<Offense> {
        match self.style {
            EnforcedStyleForMultiline::NoComma => {
                if let Some(comma_offset) = trailing_comma_pos {
                    let msg = "Avoid comma after the last parameter of a method call.";
                    let offense = ctx
                        .offense_with_range(
                            self.name(),
                            msg,
                            Severity::Convention,
                            comma_offset,
                            comma_offset + 1,
                        )
                        .with_correction(Correction::delete(
                            comma_offset,
                            comma_offset + 1,
                        ));
                    vec![offense]
                } else {
                    vec![]
                }
            }
            EnforcedStyleForMultiline::Comma => self.check_multiline_comma_style(
                ctx,
                args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
                false,
            ),
            EnforcedStyleForMultiline::ConsistentComma => self.check_multiline_comma_style(
                ctx,
                args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
                true,
            ),
            EnforcedStyleForMultiline::DiffComma => self.check_multiline_diff_comma_style(
                ctx,
                args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
            ),
        }
    }

    fn check_multiline_comma_style(
        &self,
        ctx: &CheckContext,
        args: &[Node],
        last_arg: &Node,
        last_is_block_pass: bool,
        close_offset: usize,
        trailing_comma_pos: Option<usize>,
        source: &str,
        consistent: bool,
    ) -> Vec<Offense> {
        if last_is_block_pass {
            return vec![];
        }

        let close_line = line_of(source, close_offset);
        let last_end = effective_end_offset(last_arg, source);
        let last_end_line = line_of(source, last_end);

        if !consistent {
            // "comma" style: only require comma when closing bracket is on a
            // different line than the last argument AND each item is on its own line
            if last_end_line == close_line {
                return vec![];
            }
            if has_multiple_items_on_same_line(args, source) {
                return vec![];
            }
            if args.len() == 1 {
                return vec![];
            }
        } else {
            // "consistent_comma" style: always require comma in multiline, but
            // not for a single argument that doesn't span in the right way
            if args.len() == 1
                && !is_multiline_single_arg_needing_comma(last_arg, source, close_offset)
            {
                return vec![];
            }

            // If the last arg is a braced HashNode whose closing `}` is on the
            // same line as the call's closing bracket, no comma needed
            if last_end_line == close_line && is_braced_hash_arg(last_arg) {
                return vec![];
            }
        }

        if trailing_comma_pos.is_some() {
            vec![]
        } else {
            self.missing_comma_offense(ctx, last_arg, source)
        }
    }

    fn check_multiline_diff_comma_style(
        &self,
        ctx: &CheckContext,
        args: &[Node],
        last_arg: &Node,
        last_is_block_pass: bool,
        close_offset: usize,
        trailing_comma_pos: Option<usize>,
        source: &str,
    ) -> Vec<Offense> {
        if last_is_block_pass {
            return vec![];
        }

        let close_line = line_of(source, close_offset);
        let last_end = effective_end_offset(last_arg, source);
        let last_end_line = line_of(source, last_end);
        let close_on_same_line = last_end_line == close_line;

        if close_on_same_line {
            // Comma should NOT be present
            if let Some(comma_offset) = trailing_comma_pos {
                let msg = "Avoid comma after the last parameter of a method call, unless that item immediately precedes a newline.";
                let offense = ctx
                    .offense_with_range(
                        self.name(),
                        msg,
                        Severity::Convention,
                        comma_offset,
                        comma_offset + 1,
                    )
                    .with_correction(Correction::delete(comma_offset, comma_offset + 1));
                vec![offense]
            } else {
                vec![]
            }
        } else {
            // Comma should be present
            if trailing_comma_pos.is_some() {
                return vec![];
            }
            if has_multiple_items_on_same_line(args, source) {
                return vec![];
            }
            if args.len() == 1 {
                return vec![];
            }
            self.missing_comma_offense(ctx, last_arg, source)
        }
    }

    fn missing_comma_offense(
        &self,
        ctx: &CheckContext,
        last_arg: &Node,
        source: &str,
    ) -> Vec<Offense> {
        let msg = "Put a comma after the last parameter of a multiline method call.";
        // For KeywordHashNode, the offense should point at the LAST element,
        // not the entire keyword hash
        let (offense_start, offense_end) = offense_range(last_arg, source);
        let insert_pos = insertion_point_for_comma(last_arg, source);
        let offense = ctx
            .offense_with_range(self.name(), msg, Severity::Convention, offense_start, offense_end)
            .with_correction(Correction::insert(insert_pos, ","));
        vec![offense]
    }

    fn single_line_message(&self) -> &'static str {
        match self.style {
            EnforcedStyleForMultiline::NoComma => {
                "Avoid comma after the last parameter of a method call."
            }
            EnforcedStyleForMultiline::Comma => {
                "Avoid comma after the last parameter of a method call, unless each item is on its own line."
            }
            EnforcedStyleForMultiline::ConsistentComma => {
                "Avoid comma after the last parameter of a method call, unless items are split onto multiple lines."
            }
            EnforcedStyleForMultiline::DiffComma => {
                "Avoid comma after the last parameter of a method call, unless that item immediately precedes a newline."
            }
        }
    }
}

impl Cop for TrailingCommaInArguments {
    fn name(&self) -> &'static str {
        "Style/TrailingCommaInArguments"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_call_node(node, ctx)
    }
}

// ─── Helper functions ───────────────────────────────────────────────

/// Get the 1-indexed line number of a byte offset
fn line_of(source: &str, offset: usize) -> u32 {
    let mut line = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

/// Get the offset from which to start searching for trailing commas.
/// For plain heredoc args (no method call), scans from after the heredoc body.
/// For heredoc method calls like `<<-HEREDOC.method(args)`, scans from after
/// the method call's closing paren (comma may appear before heredoc body).
fn trailing_comma_search_start(node: &Node, source: &str) -> usize {
    match node {
        Node::InterpolatedStringNode { .. } | Node::StringNode { .. } => {
            // Plain heredoc: scan from after the heredoc body (closing terminator)
            effective_end_offset(node, source)
        }
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                if contains_heredoc(&receiver, source) {
                    // Heredoc method call with parens: comma can appear after closing paren
                    // e.g. <<-HEREDOC.delete("\n"),  -> scan from after )
                    if let Some(close_loc) = call.closing_loc() {
                        return close_loc.end_offset();
                    }
                    // Heredoc method call without parens (e.g. <<-HELP.chomp):
                    // any comma after the method name is inside the heredoc body,
                    // so scan from after the heredoc terminator
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

/// Get the effective end offset of a node, accounting for heredocs.
/// For heredocs, we need to skip past the heredoc body (which is between
/// the opening marker and the closing terminator).
fn effective_end_offset(node: &Node, source: &str) -> usize {
    match node {
        Node::InterpolatedStringNode { .. } => {
            let n = node.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                let open_text = &source[s..e];
                if open_text.starts_with("<<") {
                    // Heredoc: use closing_loc end to skip past body
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
                let open_text = &source[s..e];
                if open_text.starts_with("<<") {
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
                    // For heredoc.method calls, the receiver's effective end
                    // includes the heredoc body
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

/// Check if a node contains a heredoc
fn contains_heredoc(node: &Node, source: &str) -> bool {
    match node {
        Node::InterpolatedStringNode { .. } => {
            let n = node.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                let open_text = &source[s..e];
                return open_text.starts_with("<<");
            }
            false
        }
        Node::StringNode { .. } => {
            let n = node.as_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                let open_text = &source[s..e];
                return open_text.starts_with("<<");
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

/// Find trailing comma between search_start and close_offset,
/// skipping whitespace and comments.
fn find_trailing_comma(source: &str, search_start: usize, close_offset: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = search_start;
    while i < close_offset {
        let ch = bytes[i];
        match ch {
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

/// Check if any two logical items share the same starting line.
fn has_multiple_items_on_same_line(args: &[Node], source: &str) -> bool {
    let mut all_lines: Vec<u32> = Vec::new();

    for arg in args {
        match arg {
            Node::KeywordHashNode { .. } => {
                let kh = arg.as_keyword_hash_node().unwrap();
                for elem in kh.elements().iter() {
                    all_lines.push(line_of(source, elem.location().start_offset()));
                }
            }
            _ => {
                all_lines.push(line_of(source, arg.location().start_offset()));
            }
        }
    }

    // Check for duplicate lines
    for i in 1..all_lines.len() {
        if all_lines[i] == all_lines[i - 1] {
            return true;
        }
    }
    false
}

/// For consistent_comma with a single argument, determine if it needs a comma.
fn is_multiline_single_arg_needing_comma(arg: &Node, source: &str, close_offset: usize) -> bool {
    match arg {
        Node::KeywordHashNode { .. } => {
            let kh = arg.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = kh.elements().iter().collect();
            if elements.len() >= 2 {
                let first_line = line_of(source, elements[0].location().start_offset());
                let last_line = line_of(
                    source,
                    elements.last().unwrap().location().start_offset(),
                );
                return first_line != last_line;
            }
            false
        }
        Node::HashNode { .. } => false,
        _ => {
            let arg_end_line = line_of(source, arg.location().end_offset());
            let close_line = line_of(source, close_offset);
            arg_end_line != close_line
        }
    }
}

/// Get (start, end) offsets for the offense range when comma is missing.
/// For KeywordHashNode, points at the last element rather than the whole hash.
fn offense_range(last_arg: &Node, source: &str) -> (usize, usize) {
    match last_arg {
        Node::KeywordHashNode { .. } => {
            let kh = last_arg.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = kh.elements().iter().collect();
            if let Some(last) = elements.last() {
                return offense_range(&last, source);
            }
            (last_arg.location().start_offset(), last_arg.location().end_offset())
        }
        Node::CallNode { .. } => {
            let call = last_arg.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                if contains_heredoc(&receiver, source) {
                    return (call.location().start_offset(), call.location().end_offset());
                }
            }
            (call.location().start_offset(), call.location().end_offset())
        }
        Node::InterpolatedStringNode { .. } => {
            let n = last_arg.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                let open_text = &source[s..e];
                if open_text.starts_with("<<") {
                    return (s, e);
                }
            }
            (n.location().start_offset(), n.location().end_offset())
        }
        Node::StringNode { .. } => {
            let n = last_arg.as_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                let open_text = &source[s..e];
                if open_text.starts_with("<<") {
                    return (s, e);
                }
            }
            (n.location().start_offset(), n.location().end_offset())
        }
        _ => (last_arg.location().start_offset(), last_arg.location().end_offset()),
    }
}

/// Check if a node is a braced hash literal (HashNode with `{}`).
fn is_braced_hash_arg(node: &Node) -> bool {
    matches!(node, Node::HashNode { .. })
}

/// Where to insert a comma for a missing-comma offense.
fn insertion_point_for_comma(last_arg: &Node, source: &str) -> usize {
    match last_arg {
        Node::KeywordHashNode { .. } => {
            let kh = last_arg.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = kh.elements().iter().collect();
            if let Some(last) = elements.last() {
                return insertion_point_for_comma(&last, source);
            }
            last_arg.location().end_offset()
        }
        Node::AssocNode { .. } => {
            let n = last_arg.as_assoc_node().unwrap();
            insertion_point_for_comma(&n.value(), source)
        }
        Node::CallNode { .. } => {
            let call = last_arg.as_call_node().unwrap();
            if let Some(receiver) = call.receiver() {
                if contains_heredoc(&receiver, source) {
                    // Insert after the method call part (e.g., after ".chomp")
                    // Find end of method name + args on the opening line
                    if let Some(close_loc) = call.closing_loc() {
                        return close_loc.end_offset();
                    }
                    if let Some(args) = call.arguments() {
                        let a: Vec<Node> = args.arguments().iter().collect();
                        if let Some(last) = a.last() {
                            return last.location().end_offset();
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
            let n = last_arg.as_interpolated_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                let open_text = &source[s..e];
                if open_text.starts_with("<<") {
                    return e;
                }
            }
            n.location().end_offset()
        }
        Node::StringNode { .. } => {
            let n = last_arg.as_string_node().unwrap();
            if let Some(open_loc) = n.opening_loc() {
                let s = open_loc.start_offset();
                let e = open_loc.end_offset();
                let open_text = &source[s..e];
                if open_text.starts_with("<<") {
                    return e;
                }
            }
            n.location().end_offset()
        }
        _ => last_arg.location().end_offset(),
    }
}
