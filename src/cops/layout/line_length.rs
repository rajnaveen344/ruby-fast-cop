//! Layout/LineLength - Checks the length of lines in the source code.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/line_length.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;
use std::collections::{HashMap, VecDeque};

/// How AllowHeredoc is configured
#[derive(Debug, Clone)]
pub enum AllowHeredoc {
    /// AllowHeredoc: false (default)
    Disabled,
    /// AllowHeredoc: true — all heredocs permitted
    All,
    /// AllowHeredoc: ["SQL", "HEREDOC"] — only specific delimiters permitted
    Specific(Vec<String>),
}

pub struct LineLength {
    max: usize,
    allow_uri: bool,
    allow_heredoc: AllowHeredoc,
    allow_qualified_name: bool,
    allow_cop_directives: bool,
    allow_rbs_inline_annotation: bool,
    uri_schemes: Vec<String>,
    allowed_patterns: Vec<String>,
    /// Width to use for tab characters (default: IndentationWidth, typically 2)
    tab_width: usize,
    /// Whether to split long strings (default: false)
    split_strings: bool,
}

impl LineLength {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            allow_uri: true,
            allow_heredoc: AllowHeredoc::Disabled,
            allow_qualified_name: false,
            allow_cop_directives: false,
            allow_rbs_inline_annotation: false,
            uri_schemes: vec!["http".to_string(), "https".to_string()],
            allowed_patterns: Vec::new(),
            tab_width: 2,
            split_strings: false,
        }
    }

    pub fn with_config(
        max: usize,
        allow_uri: bool,
        allow_heredoc: AllowHeredoc,
        allow_qualified_name: bool,
        allow_cop_directives: bool,
        allow_rbs_inline_annotation: bool,
        uri_schemes: Vec<String>,
        allowed_patterns: Vec<String>,
        tab_width: usize,
        split_strings: bool,
    ) -> Self {
        Self {
            max,
            allow_uri,
            allow_heredoc,
            allow_qualified_name,
            allow_cop_directives,
            allow_rbs_inline_annotation,
            uri_schemes,
            allowed_patterns,
            tab_width,
            split_strings,
        }
    }

    pub fn default_max() -> usize {
        120
    }

    // ── Line length computation (matches RuboCop) ──────────────────────

    /// Line length = raw character count + indentation_difference.
    /// Only leading tabs are expanded; mid-line tabs count as 1 char.
    fn line_length(&self, line: &str) -> usize {
        line.chars().count() + self.indentation_difference(line)
    }

    /// Extra visual width from leading tab characters.
    /// Each leading tab adds (tab_width - 1) extra visual positions.
    fn indentation_difference(&self, line: &str) -> usize {
        if self.tab_width <= 1 {
            return 0;
        }
        // If line doesn't start with a tab, no difference
        if !line.starts_with('\t') {
            return 0;
        }
        let n_leading_tabs = line.chars().take_while(|&c| c == '\t').count();
        n_leading_tabs * (self.tab_width - 1)
    }

    /// Character position where `max` falls, accounting for tab indentation.
    /// This is the default offense column_start.
    fn highlight_start(&self, line: &str) -> usize {
        let diff = self.indentation_difference(line);
        if self.max > diff {
            self.max - diff
        } else {
            0
        }
    }

    // ── Allowed-line checks ────────────────────────────────────────────

    fn is_shebang(&self, line: &str, line_index: usize) -> bool {
        line_index == 0 && line.starts_with("#!")
    }

    fn matches_allowed_pattern(&self, line: &str) -> bool {
        for pattern in &self.allowed_patterns {
            let pat = pattern.trim_matches('/');
            if let Ok(re) = Regex::new(pat) {
                if re.is_match(line) {
                    return true;
                }
            }
        }
        false
    }

    // ── Heredoc detection (text-based) ─────────────────────────────────

    /// Detect all heredoc body lines in the source.
    /// Returns Vec of (0-indexed line number, all enclosing heredoc delimiters).
    /// Tracks nesting so a line inside XXX nested inside SQL records both delimiters.
    fn find_heredoc_body_lines(source: &str) -> Vec<(usize, Vec<String>)> {
        let lines: Vec<&str> = source.lines().collect();
        let heredoc_re = Regex::new(r#"<<[-~]?['"]?(\w+)['"]?"#).unwrap();
        let mut result: Vec<(usize, Vec<String>)> = Vec::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        // Stack of heredocs whose bodies we're currently inside of
        let mut nesting: Vec<String> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if let Some(current_delim) = queue.front().cloned() {
                // If this heredoc isn't yet on the nesting stack, push it
                // (happens when transitioning from a sibling or entering a nested heredoc)
                if nesting.last().map_or(true, |d| *d != current_delim) {
                    nesting.push(current_delim.clone());
                }

                let trimmed = line.trim();
                if trimmed == current_delim {
                    // Closing delimiter line — pop from queue and nesting stack
                    queue.pop_front();
                    nesting.pop();
                } else {
                    // Body line — record with ALL enclosing heredoc delimiters
                    result.push((i, nesting.clone()));

                    // Check for nested heredoc openings (e.g. #{<<-OK})
                    let openings: Vec<String> = heredoc_re
                        .captures_iter(line)
                        .map(|c| c[1].to_string())
                        .collect();
                    // Push nested openings to front (they must complete before parent resumes)
                    for delim in openings.into_iter().rev() {
                        queue.push_front(delim);
                    }
                }
            } else {
                // Not inside any heredoc — clear nesting and check for openings
                nesting.clear();
                for cap in heredoc_re.captures_iter(line) {
                    queue.push_back(cap[1].to_string());
                }
            }
        }

        result
    }

    /// Check if a line is in a permitted heredoc body.
    fn is_in_permitted_heredoc(
        &self,
        line_index: usize,
        heredoc_lines: &[(usize, Vec<String>)],
    ) -> bool {
        match &self.allow_heredoc {
            AllowHeredoc::Disabled => false,
            AllowHeredoc::All => heredoc_lines.iter().any(|(idx, _)| *idx == line_index),
            AllowHeredoc::Specific(delimiters) => heredoc_lines.iter().any(|(idx, enclosing)| {
                *idx == line_index
                    && enclosing.iter().any(|d| delimiters.contains(d))
            }),
        }
    }

    // ── RBS inline annotation ──────────────────────────────────────────

    /// Check if line contains an RBS inline annotation (#:, #[...], #|).
    fn is_rbs_annotation(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.starts_with("#:") || trimmed.starts_with("#|") {
            return true;
        }
        // Check for trailing RBS annotation after code: ' #:' or ' #|'
        if let Some(pos) = line.rfind(" #:").or_else(|| line.rfind("\t#:")) {
            // Make sure it's not inside a string (heuristic: after some code)
            return pos > 0;
        }
        if let Some(pos) = line.rfind(" #|").or_else(|| line.rfind("\t#|")) {
            return pos > 0;
        }
        false
    }

    // ── Cop directive handling ─────────────────────────────────────────

    /// Regex pattern for rubocop directives: # rubocop:(disable|enable|todo)
    fn cop_directive_regex() -> Regex {
        Regex::new(r"#\s*rubocop\s*:\s*(?:disable|enable|todo)\b").unwrap()
    }

    /// Check if a line contains a cop directive comment.
    fn has_cop_directive(line: &str) -> bool {
        Self::cop_directive_regex().is_match(line)
    }

    /// Get the line length excluding the cop directive portion.
    /// Returns the visual length of the code before the directive.
    fn line_length_without_directive(&self, line: &str) -> usize {
        if let Some(m) = Self::cop_directive_regex().find(line) {
            let before = &line[..m.start()];
            let trimmed = before.trim_end();
            trimmed.len() + self.indentation_difference(trimmed)
        } else {
            self.line_length(line)
        }
    }

    // ── URI / Qualified Name matching (RuboCop approach) ───────────────

    /// Find the last URI match in the line. Returns raw char (start, end) positions.
    fn find_last_uri_match(&self, line: &str) -> Option<(usize, usize)> {
        if self.uri_schemes.is_empty() {
            return None;
        }

        let mut last_match: Option<(usize, usize)> = None;

        for scheme in &self.uri_schemes {
            let needle = format!("{}://", scheme);
            let mut search_from = 0;
            while let Some(byte_pos) = line[search_from..].find(&needle) {
                let abs_byte_start = search_from + byte_pos;

                // Find end of URI: stop at whitespace
                let uri_part = &line[abs_byte_start..];
                let uri_byte_end = uri_part
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(uri_part.len());
                let abs_byte_end = abs_byte_start + uri_byte_end;

                // Convert to char positions
                let char_start = line[..abs_byte_start].chars().count();
                let char_end = line[..abs_byte_end].chars().count();

                if last_match.map_or(true, |(prev_start, _)| char_start > prev_start) {
                    last_match = Some((char_start, char_end));
                }
                search_from = abs_byte_end;
            }
        }

        last_match
    }

    /// Find the last qualified name match. Returns raw char (start, end) positions.
    /// Pattern: \b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b
    fn find_last_qn_match(line: &str) -> Option<(usize, usize)> {
        let re =
            Regex::new(r"\b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b").unwrap();
        let mut last_match: Option<(usize, usize)> = None;

        for m in re.find_iter(line) {
            let char_start = line[..m.start()].chars().count();
            let char_end = line[..m.end()].chars().count();
            last_match = Some((char_start, char_end));
        }

        last_match
    }

    /// Extend match end position to include trailing non-whitespace characters.
    /// This handles URIs/QNs wrapped in quotes or parens: ("https://...") → includes ")
    /// Also handles YARD comments with linked URLs of the form {<uri> <title>}
    fn extend_end_position(line: &str, char_end: usize) -> usize {
        let chars: Vec<char> = line.chars().collect();
        let mut end = char_end;

        // Extend for YARD comments: {<uri> <title>} at end of line
        // If the line contains {...} ending at line end, extend through to closing }
        if Self::has_yard_braces(line) {
            // Find the closing } from end_position forward
            if let Some(brace_pos) = chars[end..].iter().rposition(|&c| c == '}') {
                end += brace_pos + 1;
            }
        }

        // Extend past trailing non-whitespace (handles closing quotes, parens, etc.)
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }

        end
    }

    /// Check if a line has YARD-style braces: {<something>} at end of line
    fn has_yard_braces(line: &str) -> bool {
        let trimmed = line.trim_end();
        if !trimmed.ends_with('}') {
            return false;
        }
        // Check there's a matching { somewhere in the line
        trimmed.contains('{')
    }

    /// Find the "excessive range" for a URI or QN match.
    /// Returns adjusted (begin, end) positions (with indentation_difference applied),
    /// or None if the match is entirely before max.
    fn find_excessive_uri_range(&self, line: &str) -> Option<(usize, usize)> {
        let (begin, end) = self.find_last_uri_match(line)?;
        let end = Self::extend_end_position(line, end);

        let indent_diff = self.indentation_difference(line);
        let adj_begin = begin + indent_diff;
        let adj_end = end + indent_diff;

        // If both positions are before max, the match doesn't overlap with excess
        if adj_begin < self.max && adj_end < self.max {
            return None;
        }

        Some((adj_begin, adj_end))
    }

    fn find_excessive_qn_range(&self, line: &str) -> Option<(usize, usize)> {
        let (begin, end) = Self::find_last_qn_match(line)?;
        let end = Self::extend_end_position(line, end);

        let indent_diff = self.indentation_difference(line);
        let adj_begin = begin + indent_diff;
        let adj_end = end + indent_diff;

        if adj_begin < self.max && adj_end < self.max {
            return None;
        }

        Some((adj_begin, adj_end))
    }

    /// Check if a range is in an "allowed position":
    /// starts before max AND extends to end of line.
    fn allowed_position(&self, range: (usize, usize), line: &str) -> bool {
        range.0 < self.max && range.1 == self.line_length(line)
    }

    /// Check if the combination of URI and QN ranges allows the line.
    fn allowed_combination(
        &self,
        line: &str,
        uri_range: &Option<(usize, usize)>,
        qn_range: &Option<(usize, usize)>,
    ) -> bool {
        match (uri_range, qn_range) {
            (Some(ur), Some(qr)) => {
                self.allowed_position(*ur, line) && self.allowed_position(*qr, line)
            }
            (Some(ur), None) => self.allowed_position(*ur, line),
            (None, Some(qr)) => self.allowed_position(*qr, line),
            (None, None) => false,
        }
    }

    /// Get the excessive position (column_start) given a URI/QN range.
    fn excess_position(&self, line: &str, range: &Option<(usize, usize)>) -> usize {
        if let Some((begin, end)) = range {
            if *begin < self.max {
                // Range straddles max: highlight starts after the range
                let indent_diff = self.indentation_difference(line);
                return end.saturating_sub(indent_diff);
            }
        }
        self.highlight_start(line)
    }
}

// ── Autocorrect types and AST visitor ─────────────────────────────────

/// What kind of break to insert for autocorrect.
enum BreakKind {
    /// Insert "\n" before the given byte offset.
    Newline { offset: usize },
    /// Insert "\n" before multiple byte offsets (for iterative-like corrections).
    MultiNewline { offsets: Vec<usize> },
    /// Replace whitespace [start..end) with "\n " (for blocks: after {/do/|params|).
    BlockBreak { start: usize, end: usize },
    /// Replace bytes [start..end) with "\n" + preserved indent (for semicolons).
    SemicolonBreak { start: usize, end: usize },
    /// Split a string: replace bytes [offset..offset) with `delim' \\\n'delim`
    StringSplit { offset: usize, delimiter: char },
    /// Split a string at multiple points (iterative correction)
    MultiStringSplit { offsets: Vec<usize>, delimiter: char },
}

/// Collects one breakable range per line by walking the AST.
struct BreakableRangeFinder<'a> {
    source: &'a str,
    max: usize,
    split_strings: bool,
    /// line_index (0-based) → BreakKind
    ranges: HashMap<usize, BreakKind>,
    /// Tracks nesting depth so inner collections don't overwrite outer breaks.
    depth: usize,
    /// Byte offset where each line starts
    line_starts: Vec<usize>,
}

impl<'a> BreakableRangeFinder<'a> {
    fn new(source: &'a str, max: usize, split_strings: bool) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            source,
            max,
            split_strings,
            ranges: HashMap::new(),
            depth: 0,
            line_starts,
        }
    }

    /// Convert byte offset to 0-based line index.
    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// Get column (0-based, in chars) for a byte offset.
    fn col_of(&self, offset: usize) -> usize {
        let line_idx = self.line_of(offset);
        let line_start = self.line_starts[line_idx];
        self.source[line_start..offset].chars().count()
    }

    /// Check if a node spans a single line.
    fn is_single_line(&self, start_offset: usize, end_offset: usize) -> bool {
        self.line_of(start_offset) == self.line_of(end_offset.saturating_sub(1).max(start_offset))
    }

    /// Try to register a break for a line. Only the first (outermost) break wins.
    fn register(&mut self, line_idx: usize, kind: BreakKind) {
        self.ranges.entry(line_idx).or_insert(kind);
    }

    /// Find the breakable element in a list of child offsets.
    /// RuboCop strategy: find the last element that fits entirely within the max column
    /// (including the trailing `, ` separator), and break before the *next* element.
    fn find_break_in_elements(
        &self,
        elements: &[(usize, usize)], // (start_offset, end_offset) pairs
        open_paren_offset: Option<usize>,
    ) -> Option<usize> {
        if elements.is_empty() {
            return None;
        }

        // Find the last element index where breaking after it keeps the first line ≤ max.
        // The first line extends to the start of the next element (including the `, ` separator).
        let mut last_fitting_idx: Option<usize> = None;
        for (i, &(_start, end)) in elements.iter().enumerate() {
            let fits = if i + 1 < elements.len() {
                // The first line would include text up to the start of the next element.
                self.col_of(elements[i + 1].0) <= self.max
            } else {
                // Last element: check if the element's end fits.
                self.col_of(end) <= self.max
            };

            if fits {
                last_fitting_idx = Some(i);
            } else {
                break;
            }
        }

        match last_fitting_idx {
            Some(idx) if idx + 1 < elements.len() => {
                // Break before the element after the last fitting one
                Some(elements[idx + 1].0)
            }
            Some(_) => {
                // All elements fit (shouldn't happen if line is too long, but just in case)
                None
            }
            None => {
                // No element fits on the first line.
                if open_paren_offset.is_some() && !elements.is_empty() {
                    // Parenthesized: break before the first element
                    Some(elements[0].0)
                } else if elements.len() >= 2 {
                    // Unparenthesized with multiple elements: break before the second element
                    Some(elements[1].0)
                } else {
                    None
                }
            }
        }
    }

    /// Check if any argument in a call is a heredoc (<<~FOO, <<-FOO, <<FOO).
    fn has_heredoc_arg(&self, args: &ruby_prism::NodeList) -> bool {
        for arg in args.iter() {
            match arg {
                ruby_prism::Node::StringNode { .. } => {
                    let n = arg.as_string_node().unwrap();
                    // Heredocs have opening_loc but the content starts on the next line
                    if let Some(open_loc) = n.opening_loc() {
                        let open = &self.source[open_loc.start_offset()..open_loc.end_offset()];
                        if open.starts_with("<<") {
                            return true;
                        }
                    }
                }
                ruby_prism::Node::InterpolatedStringNode { .. } => {
                    let n = arg.as_interpolated_string_node().unwrap();
                    if let Some(open_loc) = n.opening_loc() {
                        let open = &self.source[open_loc.start_offset()..open_loc.end_offset()];
                        if open.starts_with("<<") {
                            return true;
                        }
                    }
                }
                ruby_prism::Node::XStringNode { .. } => {
                    let n = arg.as_x_string_node().unwrap();
                    let open = &self.source[n.opening_loc().start_offset()..n.opening_loc().end_offset()];
                    if open.starts_with("<<") {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Check if a specific argument node is a heredoc.
    fn is_heredoc_node(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::StringNode { .. } => {
                let n = node.as_string_node().unwrap();
                if let Some(open_loc) = n.opening_loc() {
                    let open = &self.source[open_loc.start_offset()..open_loc.end_offset()];
                    return open.starts_with("<<");
                }
                false
            }
            ruby_prism::Node::InterpolatedStringNode { .. } => {
                let n = node.as_interpolated_string_node().unwrap();
                if let Some(open_loc) = n.opening_loc() {
                    let open = &self.source[open_loc.start_offset()..open_loc.end_offset()];
                    return open.starts_with("<<");
                }
                false
            }
            ruby_prism::Node::XStringNode { .. } => {
                let n = node.as_x_string_node().unwrap();
                let open = &self.source[n.opening_loc().start_offset()..n.opening_loc().end_offset()];
                open.starts_with("<<")
            }
            _ => false,
        }
    }

    /// Process call arguments (works for both regular and safe navigation calls).
    fn process_call_args(
        &mut self,
        node_start: usize,
        node_end: usize,
        arguments: Option<&ruby_prism::ArgumentsNode>,
        opening_loc: Option<ruby_prism::Location>,
        line_idx: usize,
    ) {
        let args = match arguments {
            Some(a) => a,
            None => return,
        };

        let arg_list = args.arguments();
        if arg_list.is_empty() {
            return;
        }

        // Don't autocorrect if already multi-line
        if !self.is_single_line(node_start, node_end) {
            return;
        }

        let is_parenthesized = opening_loc.is_some();

        // Check for heredoc arguments
        if self.has_heredoc_arg(&arg_list) {
            // If there's a heredoc, only break if there are args before the first heredoc
            let mut non_heredoc_elements = Vec::new();
            for arg in arg_list.iter() {
                if self.is_heredoc_node(&arg) {
                    break; // Stop before heredoc — can't break after it
                }
                non_heredoc_elements.push((arg.location().start_offset(), arg.location().end_offset()));
            }
            if non_heredoc_elements.is_empty() {
                return; // First arg is heredoc, can't break
            }
            // Only break if there are at least 2 args before the heredoc (or 1 + heredoc with parens)
            if let Some(offset) = self.find_break_in_elements(
                &non_heredoc_elements,
                opening_loc.as_ref().map(|l| l.start_offset()),
            ) {
                self.register(line_idx, BreakKind::Newline { offset });
            }
            return;
        }

        // Special case: single HashNode argument (e.g., `foo({ a: 1, b: 2 })`)
        // Break inside the hash (after `{ `) rather than after the call's paren.
        if arg_list.len() == 1 {
            let first_arg = arg_list.iter().next().unwrap();
            if let ruby_prism::Node::HashNode { .. } = first_arg {
                let hash = first_arg.as_hash_node().unwrap();
                let hash_elements: Vec<(usize, usize)> = hash.elements().iter()
                    .map(|e| (e.location().start_offset(), e.location().end_offset()))
                    .collect();
                if hash_elements.len() >= 2 {
                    let open_offset = Some(hash.opening_loc().start_offset());
                    if let Some(offset) = self.find_break_in_elements(&hash_elements, open_offset) {
                        self.register(line_idx, BreakKind::Newline { offset });
                    }
                    return;
                }
            }
        }

        // Flatten keyword hash arguments: when a call like `foo(a: 1, b: 2)` is parsed,
        // Prism wraps keyword args in a single KeywordHashNode. We need the individual pairs.
        let mut elements: Vec<(usize, usize)> = Vec::new();
        for arg in arg_list.iter() {
            match arg {
                ruby_prism::Node::KeywordHashNode { .. } => {
                    let kh = arg.as_keyword_hash_node().unwrap();
                    for elem in kh.elements().iter() {
                        elements.push((elem.location().start_offset(), elem.location().end_offset()));
                    }
                }
                _ => {
                    elements.push((arg.location().start_offset(), arg.location().end_offset()));
                }
            }
        }

        // For unparenthesized calls with only 1 effective argument, don't autocorrect
        if !is_parenthesized && elements.len() == 1 {
            return;
        }

        if let Some(offset) = self.find_break_in_elements(
            &elements,
            opening_loc.as_ref().map(|l| l.start_offset()),
        ) {
            self.register(line_idx, BreakKind::Newline { offset });
        }
    }

    /// Process hash/array elements.
    fn process_collection_elements(
        &mut self,
        node_start: usize,
        node_end: usize,
        elements: &[(usize, usize)],
        open_offset: Option<usize>,
        line_idx: usize,
    ) {
        if elements.is_empty() {
            return;
        }
        if !self.is_single_line(node_start, node_end) {
            return;
        }
        if let Some(offset) = self.find_break_in_elements(elements, open_offset) {
            self.register(line_idx, BreakKind::Newline { offset });
        }
    }

    /// Check if the receiver chain of a block contains a heredoc.
    fn receiver_has_heredoc(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                // Check arguments for heredocs
                if let Some(args) = call.arguments() {
                    if self.has_heredoc_arg(&args.arguments()) {
                        return true;
                    }
                }
                // Check receiver recursively
                if let Some(recv) = call.receiver() {
                    return self.receiver_has_heredoc(&recv);
                }
                false
            }
            ruby_prism::Node::StringNode { .. } => {
                let n = node.as_string_node().unwrap();
                if let Some(open_loc) = n.opening_loc() {
                    let open = &self.source[open_loc.start_offset()..open_loc.end_offset()];
                    return open.starts_with("<<");
                }
                false
            }
            ruby_prism::Node::InterpolatedStringNode { .. } => {
                let n = node.as_interpolated_string_node().unwrap();
                if let Some(open_loc) = n.opening_loc() {
                    let open = &self.source[open_loc.start_offset()..open_loc.end_offset()];
                    return open.starts_with("<<");
                }
                false
            }
            ruby_prism::Node::XStringNode { .. } => {
                let n = node.as_x_string_node().unwrap();
                let open = &self.source[n.opening_loc().start_offset()..n.opening_loc().end_offset()];
                open.starts_with("<<")
            }
            ruby_prism::Node::ArrayNode { .. } => {
                let arr = node.as_array_node().unwrap();
                for elem in arr.elements().iter() {
                    if self.receiver_has_heredoc(&elem) {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Process a block node (brace or do-end).
    /// `params_end_offset` is the end offset of the block parameters (after `|`), or None.
    fn process_block(
        &mut self,
        node_start: usize,
        node_end: usize,
        params_end_offset: Option<usize>,
        opening_end_offset: usize,
        has_body: bool,
        call_node: Option<&ruby_prism::Node>,
        line_idx: usize,
    ) {
        if !self.is_single_line(node_start, node_end) {
            return;
        }

        // Check if receiver chain contains a heredoc
        if let Some(call) = call_node {
            if self.receiver_has_heredoc(call) {
                return;
            }
        }

        // Find where to break: after |params| if present, else after { or do
        let break_after = params_end_offset.unwrap_or(opening_end_offset);

        // Find the actual body start (skip whitespace after break point)
        let after_break = &self.source[break_after..];
        let skip_ws = after_break.bytes().take_while(|&b| b == b' ').count();
        let body_offset = break_after + skip_ws;

        // Only break if there's actually body content
        if has_body || body_offset < node_end {
            // Replace whitespace between keyword/params and body with "\n "
            self.register(line_idx, BreakKind::BlockBreak {
                start: break_after,
                end: body_offset,
            });
        }
    }

    /// Find semicolons in a line that can be used as break points.
    fn find_semicolon_breaks(&mut self) {
        let heredoc_re = Regex::new(r#"<<[-~]?['"`]?(\w+)['"`]?"#).unwrap();
        let lines: Vec<&str> = self.source.lines().collect();

        // Track heredoc body lines
        let mut in_heredoc = false;
        let mut heredoc_delim: Option<String> = None;

        let mut byte_offset = 0usize;
        for (line_idx, line) in lines.iter().enumerate() {
            let line_len_chars = line.chars().count();

            if in_heredoc {
                if line.trim() == heredoc_delim.as_deref().unwrap_or("") {
                    in_heredoc = false;
                    heredoc_delim = None;
                }
                byte_offset += line.len() + 1; // +1 for \n
                continue;
            }

            // Check for heredoc openers
            if let Some(cap) = heredoc_re.captures(line) {
                in_heredoc = true;
                heredoc_delim = Some(cap[1].to_string());
            }

            // Only process lines that are too long
            if line_len_chars <= self.max {
                byte_offset += line.len() + 1;
                continue;
            }

            // Skip if line is a comment
            if line.trim_start().starts_with('#') {
                byte_offset += line.len() + 1;
                continue;
            }

            // Find semicolons not inside strings
            if let Some(semi_break) = self.find_semicolon_in_line(line, byte_offset) {
                // Only register if no AST-based break already exists
                self.ranges.entry(line_idx).or_insert(semi_break);
            }

            byte_offset += line.len() + 1;
        }
    }

    /// Find the best semicolon break point in a line.
    fn find_semicolon_in_line(&self, line: &str, line_byte_offset: usize) -> Option<BreakKind> {
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escape_next = false;
        let mut best_semi: Option<usize> = None; // char index of best semicolon
        let mut best_semi_byte: Option<usize> = None;

        let mut byte_idx = 0usize;
        for (char_idx, ch) in line.chars().enumerate() {
            let ch_len = ch.len_utf8();
            if escape_next {
                escape_next = false;
                byte_idx += ch_len;
                continue;
            }

            if ch == '\\' && (in_single_quote || in_double_quote) {
                escape_next = true;
                byte_idx += ch_len;
                continue;
            }

            if ch == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
            } else if ch == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
            } else if ch == ';' && !in_single_quote && !in_double_quote {
                best_semi = Some(char_idx);
                best_semi_byte = Some(byte_idx);
            }

            byte_idx += ch_len;
        }

        let semi_char_idx = best_semi?;
        let semi_byte_idx = best_semi_byte?;

        // The semicolon must have non-whitespace content after it (and optional whitespace)
        let after_semi = &line[semi_byte_idx + 1..];

        // Find where trailing semicolons end
        let mut all_semis_end = semi_byte_idx + 1;
        for ch in after_semi.chars() {
            if ch == ';' {
                all_semis_end += 1;
            } else {
                break;
            }
        }

        let after_all_semis = &line[all_semis_end..];
        let trimmed_after = after_all_semis.trim_start();

        // If nothing meaningful after the semicolons, no break possible
        if trimmed_after.is_empty() {
            return None;
        }

        // Find the whitespace after the last semicolon group
        let ws_after = after_all_semis.len() - after_all_semis.trim_start().len();

        // Break: keep everything through the semicolons, replace whitespace with \n + remaining whitespace
        let break_start = line_byte_offset + all_semis_end;
        let break_end = line_byte_offset + all_semis_end + ws_after;

        let _ = semi_char_idx; // Used for validation above
        Some(BreakKind::SemicolonBreak {
            start: break_start,
            end: break_end,
        })
    }

    /// Find string split points for long strings on a given line.
    fn find_string_splits(&mut self) {
        if !self.split_strings {
            return;
        }

        let result = ruby_prism::parse(self.source.as_bytes());
        let root = result.node();
        let mut string_finder = StringSplitFinder {
            source: self.source,
            max: self.max,
            line_starts: &self.line_starts,
            splits: Vec::new(),
        };
        string_finder.visit(&root);

        for (line_idx, kind) in string_finder.splits {
            // String splits have lowest priority — don't overwrite existing breaks
            self.ranges.entry(line_idx).or_insert(kind);
        }
    }
}

impl Visit<'_> for BreakableRangeFinder<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if self.depth == 0 && self.is_single_line(start, end) && !self.ranges.contains_key(&line_idx) {
            self.depth += 1;
            self.process_call_args(
                start,
                end,
                node.arguments().as_ref(),
                node.opening_loc(),
                line_idx,
            );
            ruby_prism::visit_call_node(self, node);
            self.depth -= 1;
        } else {
            self.depth += 1;
            ruby_prism::visit_call_node(self, node);
            self.depth -= 1;
        }
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let start = node.location().start_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) {
            if let Some(params) = node.parameters() {
                let params_end = params.location().end_offset();

                // Check if parameters are on the same line as def (allows multi-line def...end)
                if self.is_single_line(start, params_end) {
                    let param_list: Vec<(usize, usize)> = params
                        .requireds().iter()
                        .map(|p| (p.location().start_offset(), p.location().end_offset()))
                        .chain(params.optionals().iter().map(|p| (p.location().start_offset(), p.location().end_offset())))
                        .chain(params.keywords().iter().map(|p| (p.location().start_offset(), p.location().end_offset())))
                        .chain(params.posts().iter().map(|p| (p.location().start_offset(), p.location().end_offset())))
                        .collect();

                    if !param_list.is_empty() {
                        let open_paren = node.lparen_loc().map(|l| l.start_offset());
                        if let Some(offset) = self.find_break_in_elements(&param_list, open_paren) {
                            self.register(line_idx, BreakKind::Newline { offset });
                        }
                    }
                }
            }
        }

        ruby_prism::visit_def_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if self.depth == 0 && self.is_single_line(start, end) && !self.ranges.contains_key(&line_idx) {
            let elements: Vec<(usize, usize)> = node.elements().iter()
                .map(|e| (e.location().start_offset(), e.location().end_offset()))
                .collect();

            // Don't autocorrect if already multiline or if first element is a comment
            if elements.len() >= 2 {
                let open_offset = Some(node.opening_loc().start_offset());
                self.depth += 1;
                self.process_collection_elements(start, end, &elements, open_offset, line_idx);
                ruby_prism::visit_hash_node(self, node);
                self.depth -= 1;
            } else {
                self.depth += 1;
                ruby_prism::visit_hash_node(self, node);
                self.depth -= 1;
            }
        } else {
            self.depth += 1;
            ruby_prism::visit_hash_node(self, node);
            self.depth -= 1;
        }
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if self.depth == 0 && self.is_single_line(start, end) && !self.ranges.contains_key(&line_idx) {
            let elements: Vec<(usize, usize)> = node.elements().iter()
                .map(|e| (e.location().start_offset(), e.location().end_offset()))
                .collect();

            // Check for heredoc elements
            let has_heredoc = node.elements().iter().any(|e| self.is_heredoc_node(&e));
            if has_heredoc {
                // Don't break arrays containing heredocs
                self.depth += 1;
                ruby_prism::visit_array_node(self, node);
                self.depth -= 1;
                return;
            }

            if elements.len() >= 2 {
                let open_offset = node.opening_loc().map(|l| l.start_offset());
                self.depth += 1;
                self.process_collection_elements(start, end, &elements, open_offset, line_idx);
                ruby_prism::visit_array_node(self, node);
                self.depth -= 1;
            } else {
                self.depth += 1;
                ruby_prism::visit_array_node(self, node);
                self.depth -= 1;
            }
        } else {
            self.depth += 1;
            ruby_prism::visit_array_node(self, node);
            self.depth -= 1;
        }
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) && self.is_single_line(start, end) {
            // Get params end, but skip implicit params (e.g., Ruby 3.4 `it`) whose
            // ItParametersNode spans the entire block (same start as block node).
            let params_end = node.parameters().and_then(|p| {
                if p.location().start_offset() == start {
                    None // Implicit parameter — treat as no explicit params
                } else {
                    Some(p.location().end_offset())
                }
            });
            self.process_block(
                start,
                end,
                params_end,
                node.opening_loc().end_offset(),
                node.body().is_some(),
                None,
                line_idx,
            );
        }

        ruby_prism::visit_block_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) && self.is_single_line(start, end) {
            // For lambdas, params like (x) come before the opening brace {,
            // so we always break after the opening brace, not after params.
            self.process_block(
                start,
                end,
                None,
                node.opening_loc().end_offset(),
                node.body().is_some(),
                None,
                line_idx,
            );
        }

        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) && self.is_single_line(start, end) {
            // Check if the value is a multi-value assignment (comma-separated)
            let value = node.value();
            let value_start = value.location().start_offset();
            let value_end = value.location().end_offset();
            let value_text = &self.source[value_start..value_end];
            if value_text.contains(',') {
                // First break: before the value (after "= ")
                let mut offsets = vec![value_start];

                // Find comma break points within the value for iterative-like correction.
                // The second line starts at col 0, so find the last comma+space that fits within max.
                let mut byte_pos = 0usize;
                let mut char_pos = 0usize;
                let mut last_comma_break: Option<usize> = None;
                let value_bytes = value_text.as_bytes();
                while byte_pos < value_bytes.len() {
                    let ch = value_text[byte_pos..].chars().next().unwrap();
                    let ch_len = ch.len_utf8();

                    if ch == ',' && byte_pos + 1 < value_bytes.len() {
                        // Find end of whitespace after comma
                        let after_comma = &value_text[byte_pos + 1..];
                        let ws = after_comma.bytes().take_while(|&b| b == b' ').count();
                        let break_offset = value_start + byte_pos + 1 + ws;
                        // Check if the text up to and including comma + space fits on the line
                        if char_pos + 1 + ws <= self.max {
                            last_comma_break = Some(break_offset);
                        }
                    }

                    char_pos += 1;
                    byte_pos += ch_len;
                }

                if let Some(comma_break) = last_comma_break {
                    offsets.push(comma_break);
                }

                if offsets.len() == 1 {
                    self.register(line_idx, BreakKind::Newline { offset: value_start });
                } else {
                    self.register(line_idx, BreakKind::MultiNewline { offsets });
                }
            }
        }

        ruby_prism::visit_local_variable_write_node(self, node);
    }
}

/// Separate visitor for finding string split points.
struct StringSplitFinder<'a> {
    source: &'a str,
    max: usize,
    line_starts: &'a [usize],
    splits: Vec<(usize, BreakKind)>,
}

impl<'a> StringSplitFinder<'a> {
    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    fn col_of(&self, offset: usize) -> usize {
        let line_idx = self.line_of(offset);
        let line_start = self.line_starts[line_idx];
        self.source[line_start..offset].chars().count()
    }

    fn is_single_line(&self, start: usize, end: usize) -> bool {
        self.line_of(start) == self.line_of(end.saturating_sub(1).max(start))
    }

    /// Find a safe split point in a string near the max column.
    fn find_split_offset(
        &self,
        content_start: usize, // byte offset of first char of string content (after quote)
        content_end: usize,   // byte offset of last char before closing quote
        string_start_col: usize, // column of the opening quote
        delimiter: char,
    ) -> Option<usize> {
        if string_start_col >= self.max {
            return None; // String starts after max, can't split usefully
        }

        let content = &self.source[content_start..content_end];
        if content.is_empty() {
            return None;
        }

        // After splitting, the first line is: [prefix]'[content_part]' \
        // So we need room for: opening quote (1) + content + closing quote (1) + ' \' (2) = content + 4
        // content_chars <= max - string_start_col - 4
        let max_content_chars = if self.max > string_start_col + 4 {
            self.max - string_start_col - 4
        } else {
            return None;
        };

        // Scan content for the best split point
        let mut best_space: Option<usize> = None; // byte offset in source of best space split
        let mut char_count = 0usize;
        let mut byte_idx = 0usize;
        let content_bytes = content.as_bytes();

        while byte_idx < content_bytes.len() {
            let ch = content[byte_idx..].chars().next().unwrap();
            let ch_len = ch.len_utf8();

            if char_count >= max_content_chars {
                // We've reached the max. Use the best candidate we found.
                break;
            }

            // Track space positions (prefer last space before max)
            if ch == ' ' && char_count + 1 <= max_content_chars {
                best_space = Some(content_start + byte_idx + ch_len);
            }

            char_count += 1;
            byte_idx += ch_len;
        }

        // If we found a space, split after it
        if let Some(offset) = best_space {
            return Some(offset);
        }

        // Check for escape sequences near max (don't split inside them)
        // Scan backwards from max to find \n, \u, \x, #{
        let split_at_chars = max_content_chars;
        let mut char_idx = 0usize;
        let mut byte_at_split = 0usize;
        let mut scan_idx = 0usize;
        while scan_idx < content_bytes.len() {
            let ch = content[scan_idx..].chars().next().unwrap();
            let ch_len = ch.len_utf8();
            if char_idx == split_at_chars {
                byte_at_split = scan_idx;
                break;
            }
            char_idx += 1;
            scan_idx += ch_len;
        }
        if scan_idx >= content_bytes.len() {
            byte_at_split = content_bytes.len();
        }

        // Check for escape sequences: scan backwards a few chars from split point
        // to avoid splitting inside \n, \u0061, \x61, #{...}
        let lookback_start = if byte_at_split > 6 { byte_at_split - 6 } else { 0 };
        let near_split = &content[lookback_start..byte_at_split.min(content_bytes.len())];
        // Check for \n, \u, \x near the end
        if let Some(esc_pos) = near_split.rfind('\\') {
            let abs_esc = lookback_start + esc_pos;
            if abs_esc < byte_at_split && byte_at_split < content_bytes.len() {
                let after_backslash = content.as_bytes().get(abs_esc + 1);
                if matches!(after_backslash, Some(b'n') | Some(b'u') | Some(b'x')) {
                    // Split before the escape sequence
                    return Some(content_start + abs_esc);
                }
            }
        }

        // Check for interpolation #{...} near split point
        if delimiter == '"' {
            // Look for #{ near or at the split point
            if let Some(interp_pos) = content[..byte_at_split.min(content_bytes.len())].rfind("#{") {
                let interp_col = content[..interp_pos].chars().count();
                // If #{} is close to or straddles the max, split before it
                if interp_col + string_start_col + 1 >= self.max.saturating_sub(4) {
                    if interp_pos > 0 {
                        return Some(content_start + interp_pos);
                    }
                }
            }
        }

        // Default: hard split at max
        if byte_at_split > 0 && byte_at_split < content_bytes.len() {
            Some(content_start + byte_at_split)
        } else {
            None
        }
    }
}

impl Visit<'_> for StringSplitFinder<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if !self.is_single_line(start, end) {
            return;
        }

        let line_idx = self.line_of(start);
        let line_start = self.line_starts[line_idx];
        let line_end = if line_idx + 1 < self.line_starts.len() {
            self.line_starts[line_idx + 1] - 1
        } else {
            self.source.len()
        };
        let line = &self.source[line_start..line_end];
        let line_char_len = line.chars().count();

        if line_char_len <= self.max {
            return;
        }

        // Get delimiter info
        let opening_loc = match node.opening_loc() {
            Some(loc) => loc,
            None => return, // No opening delimiter (shouldn't happen for normal strings)
        };
        let opening = &self.source[opening_loc.start_offset()..opening_loc.end_offset()];

        // Skip heredocs, %q, %Q, %{, percent literals
        if opening.starts_with("<<") || opening.starts_with('%') {
            return;
        }

        let delimiter = if opening.contains('"') { '"' } else { '\'' };
        let string_start_col = self.col_of(start);

        // String must start before max (straddle check)
        if string_start_col >= self.max {
            return;
        }

        // Content is between opening and closing delimiters
        let content_start = opening_loc.end_offset();
        let closing_loc = match node.closing_loc() {
            Some(loc) => loc,
            None => return,
        };
        let content_end = closing_loc.start_offset();

        // Find split point(s). If a single split leaves the continuation line still too long,
        // find additional splits (simulating RuboCop's iterative correction).
        let mut split_offsets: Vec<usize> = Vec::new();
        let mut remaining_start = content_start;
        let mut current_start_col = string_start_col;

        loop {
            match self.find_split_offset(remaining_start, content_end, current_start_col, delimiter) {
                Some(split_offset) => {
                    split_offsets.push(split_offset);
                    remaining_start = split_offset;
                    // After split, continuation line starts at col 0: 'remaining...'
                    current_start_col = 0;

                    // Check if the remaining content still needs splitting
                    let remaining_content = &self.source[remaining_start..content_end];
                    let remaining_chars = remaining_content.chars().count();
                    // Continuation line: ' + remaining + ' = remaining_chars + 2
                    if remaining_chars + 2 <= self.max {
                        break; // Fits on one line
                    }
                }
                None => break,
            }
        }

        if split_offsets.len() == 1 {
            self.splits.push((line_idx, BreakKind::StringSplit { offset: split_offsets[0], delimiter }));
        } else if split_offsets.len() > 1 {
            self.splits.push((line_idx, BreakKind::MultiStringSplit { offsets: split_offsets, delimiter }));
        }

        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if !self.is_single_line(start, end) {
            return;
        }

        let line_idx = self.line_of(start);
        let line_start = self.line_starts[line_idx];
        let line_end = if line_idx + 1 < self.line_starts.len() {
            self.line_starts[line_idx + 1] - 1
        } else {
            self.source.len()
        };
        let line = &self.source[line_start..line_end];
        let line_char_len = line.chars().count();

        if line_char_len <= self.max {
            return;
        }

        let opening_loc = match node.opening_loc() {
            Some(loc) => loc,
            None => return,
        };
        let opening = &self.source[opening_loc.start_offset()..opening_loc.end_offset()];

        if opening.starts_with("<<") || opening.starts_with('%') {
            return;
        }

        let delimiter = '"';
        let string_start_col = self.col_of(start);

        if string_start_col >= self.max {
            return;
        }

        let closing_loc = match node.closing_loc() {
            Some(loc) => loc,
            None => return,
        };

        // For interpolated strings, we need to find safe split points that don't
        // break inside #{...} interpolations.
        let content_start = opening_loc.end_offset();
        let content_end = closing_loc.start_offset();
        let content = &self.source[content_start..content_end];

        if content.is_empty() {
            return;
        }

        // Check if the entire string is just interpolation
        if content.starts_with("#{") && content.ends_with('}') && Self::is_single_interpolation(content) {
            return; // Can't split a string that's entirely one interpolation
        }

        // Find max_content_chars (same logic as plain strings)
        // First line: "content_part" \ → need room for opening quote + closing quote + ' \'
        let max_content_chars = if self.max > string_start_col + 4 {
            self.max - string_start_col - 4
        } else {
            return;
        };

        // Scan content to find best split point, respecting interpolation boundaries
        let split_offset = self.find_interpolated_split(content, content_start, max_content_chars);
        if let Some(offset) = split_offset {
            self.splits.push((line_idx, BreakKind::StringSplit { offset, delimiter }));
        }
    }
}

impl<'a> StringSplitFinder<'a> {
    /// Check if content is a single interpolation: #{...} with no text around it
    fn is_single_interpolation(content: &str) -> bool {
        if !content.starts_with("#{") {
            return false;
        }
        let mut depth = 0;
        for (i, ch) in content.char_indices() {
            if ch == '{' && i > 0 && content.as_bytes().get(i - 1) == Some(&b'#') {
                depth += 1;
            } else if ch == '{' && (i == 0 || content.as_bytes().get(i - 1) != Some(&b'#')) {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    return i == content.len() - 1;
                }
            }
        }
        false
    }

    fn find_interpolated_split(
        &self,
        content: &str,
        content_start: usize,
        max_content_chars: usize,
    ) -> Option<usize> {
        let mut char_count = 0usize;
        let mut byte_idx = 0usize;
        let bytes = content.as_bytes();
        let mut best_space: Option<usize> = None;
        let mut last_interp_start: Option<usize> = None;
        let mut in_interp = false;
        let mut interp_depth = 0;

        while byte_idx < bytes.len() {
            let ch = content[byte_idx..].chars().next().unwrap();
            let ch_len = ch.len_utf8();

            // Track interpolation boundaries
            if !in_interp && byte_idx + 1 < bytes.len() && bytes[byte_idx] == b'#' && bytes[byte_idx + 1] == b'{' {
                in_interp = true;
                interp_depth = 1;
                last_interp_start = Some(byte_idx);
                if char_count >= max_content_chars.saturating_sub(2) {
                    // Interpolation starts near/past max — split before it
                    if byte_idx > 0 {
                        return Some(content_start + byte_idx);
                    }
                }
                char_count += 1;
                byte_idx += ch_len;
                continue;
            }

            if in_interp {
                if ch == '{' {
                    interp_depth += 1;
                } else if ch == '}' {
                    interp_depth -= 1;
                    if interp_depth == 0 {
                        in_interp = false;
                    }
                }
                char_count += 1;
                byte_idx += ch_len;
                continue;
            }

            if char_count >= max_content_chars {
                break;
            }

            if ch == ' ' && char_count + 1 <= max_content_chars {
                best_space = Some(content_start + byte_idx + ch_len);
            }

            char_count += 1;
            byte_idx += ch_len;
        }

        if let Some(offset) = best_space {
            return Some(offset);
        }

        // If we hit the max and there's a nearby interpolation start, split before it
        if let Some(interp_byte) = last_interp_start {
            let interp_chars = content[..interp_byte].chars().count();
            if interp_chars > 0 && interp_chars < max_content_chars + 5 {
                return Some(content_start + interp_byte);
            }
        }

        // Hard split at max
        let mut c = 0usize;
        let mut bi = 0usize;
        while bi < bytes.len() {
            if c == max_content_chars {
                if bi > 0 && bi < bytes.len() {
                    return Some(content_start + bi);
                }
                break;
            }
            let ch = content[bi..].chars().next().unwrap();
            bi += ch.len_utf8();
            c += 1;
        }

        None
    }
}

impl Default for LineLength {
    fn default() -> Self {
        Self::new(Self::default_max())
    }
}

impl Cop for LineLength {
    fn name(&self) -> &'static str {
        "Layout/LineLength"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Phase 1: Collect breakable ranges from AST
        let breakable_ranges = {
            let result = ruby_prism::parse(ctx.source.as_bytes());
            let root = result.node();
            let mut finder = BreakableRangeFinder::new(ctx.source, self.max, self.split_strings);
            finder.visit(&root);
            // Phase 1b: Find semicolon break points (text-based, lower priority)
            finder.find_semicolon_breaks();
            // Phase 1c: Find string split points (lowest priority)
            finder.find_string_splits();
            finder.ranges
        };

        // Phase 2: Line scanning (existing logic)
        let mut offenses = Vec::new();
        let mut past_end = false;

        // Pre-compute heredoc body lines if AllowHeredoc is enabled
        let heredoc_lines = match &self.allow_heredoc {
            AllowHeredoc::Disabled => Vec::new(),
            _ => Self::find_heredoc_body_lines(ctx.source),
        };

        for (line_index, line) in ctx.source.lines().enumerate() {
            // Skip lines after __END__
            if line == "__END__" {
                past_end = true;
                continue;
            }
            if past_end {
                continue;
            }

            let visual_len = self.line_length(line);

            // Skip if within limit
            if visual_len <= self.max {
                continue;
            }

            // Skip shebang lines
            if self.is_shebang(line, line_index) {
                continue;
            }

            // Skip lines matching allowed patterns
            if self.matches_allowed_pattern(line) {
                continue;
            }

            // Skip lines in permitted heredoc bodies
            if self.is_in_permitted_heredoc(line_index, &heredoc_lines) {
                continue;
            }

            // Skip RBS inline annotations
            if self.allow_rbs_inline_annotation && self.is_rbs_annotation(line) {
                continue;
            }

            let char_len = line.chars().count();
            let line_num = (line_index + 1) as u32;

            // Look up correction for this line
            let correction = breakable_ranges.get(&line_index).map(|br| {
                match br {
                    BreakKind::Newline { offset } => {
                        Correction::replace(*offset, *offset, "\n")
                    }
                    BreakKind::MultiNewline { offsets } => {
                        use crate::offense::Edit;
                        Correction {
                            edits: offsets.iter().map(|&off| Edit {
                                start_offset: off,
                                end_offset: off,
                                replacement: "\n".to_string(),
                            }).collect(),
                        }
                    }
                    BreakKind::BlockBreak { start, end } => {
                        if start == end {
                            // No whitespace gap (e.g., `{body}`) — just insert newline
                            Correction::replace(*start, *end, "\n")
                        } else {
                            // Replace whitespace with newline + single space indent
                            Correction::replace(*start, *end, "\n ")
                        }
                    }
                    BreakKind::SemicolonBreak { start, end } => {
                        // Find what whitespace to preserve for indentation
                        let after_semi_text = &ctx.source[*start..*end];
                        let indent = if after_semi_text.is_empty() {
                            "\n ".to_string()
                        } else {
                            format!("\n{}", after_semi_text)
                        };
                        Correction::replace(*start, *end, indent)
                    }
                    BreakKind::StringSplit { offset, delimiter } => {
                        let text = format!("{} \\\n{}", delimiter, delimiter);
                        Correction::replace(*offset, *offset, text)
                    }
                    BreakKind::MultiStringSplit { offsets, delimiter } => {
                        use crate::offense::Edit;
                        let text = format!("{} \\\n{}", delimiter, delimiter);
                        Correction {
                            edits: offsets.iter().map(|&off| Edit {
                                start_offset: off,
                                end_offset: off,
                                replacement: text.clone(),
                            }).collect(),
                        }
                    }
                }
            });

            // Handle cop directives
            if self.allow_cop_directives && Self::has_cop_directive(line) {
                let len_without = self.line_length_without_directive(line);
                if len_without <= self.max {
                    continue; // directive covers all excess
                }
                // Still too long even without directive — report adjusted length
                let col_start = self.highlight_start(line) as u32;
                // Column end = char position for len_without_directive
                let indent_diff = self.indentation_difference(line);
                let col_end = if len_without > indent_diff {
                    (len_without - indent_diff) as u32
                } else {
                    char_len as u32
                };
                let mut offense = Offense::new(
                    self.name(),
                    format!("Line is too long. [{}/{}]", len_without, self.max),
                    self.severity(),
                    Location::new(line_num, col_start, line_num, col_end),
                    ctx.filename,
                );
                if let Some(c) = correction {
                    offense = offense.with_correction(c);
                }
                offenses.push(offense);
                continue;
            }

            // Handle URI / qualified name exemptions
            if self.allow_uri || self.allow_qualified_name {
                let uri_range = if self.allow_uri {
                    self.find_excessive_uri_range(line)
                } else {
                    None
                };
                let qn_range = if self.allow_qualified_name {
                    self.find_excessive_qn_range(line)
                } else {
                    None
                };

                if uri_range.is_some() || qn_range.is_some() {
                    if self.allowed_combination(line, &uri_range, &qn_range) {
                        continue; // URI/QN covers all excess to end of line
                    }

                    // Still too long — report with adjusted column
                    let range = uri_range.or(qn_range);
                    let excessive_pos = self.excess_position(line, &range) as u32;

                    let mut offense = Offense::new(
                        self.name(),
                        format!("Line is too long. [{}/{}]", visual_len, self.max),
                        self.severity(),
                        Location::new(line_num, excessive_pos, line_num, char_len as u32),
                        ctx.filename,
                    );
                    if let Some(c) = correction {
                        offense = offense.with_correction(c);
                    }
                    offenses.push(offense);
                    continue;
                }
            }

            // Default offense
            let col_start = self.highlight_start(line) as u32;
            let mut offense = Offense::new(
                self.name(),
                format!("Line is too long. [{}/{}]", visual_len, self.max),
                self.severity(),
                Location::new(line_num, col_start, line_num, char_len as u32),
                ctx.filename,
            );
            if let Some(c) = correction {
                offense = offense.with_correction(c);
            }
            offenses.push(offense);
        }

        offenses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_max(source: &str, max: usize) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(LineLength::new(max));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_max(source, 80)
    }

    #[test]
    fn allows_short_lines() {
        let offenses = check("puts 'hello'");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_exactly_max_length() {
        let line = "x".repeat(80);
        let offenses = check(&line);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_line_over_max() {
        let line = "x".repeat(81);
        let offenses = check(&line);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].location.line, 1);
        assert_eq!(offenses[0].location.column, 80); // highlights from max
        assert_eq!(offenses[0].location.last_column, 81);
        assert!(offenses[0].message.contains("[81/80]"));
    }

    #[test]
    fn respects_custom_max() {
        let line = "x".repeat(100);

        let offenses = check_with_max(&line, 80);
        assert_eq!(offenses.len(), 1);

        let offenses = check_with_max(&line, 160);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_multiple_long_lines() {
        let source = format!("short\n{}\nokay\n{}\n", "a".repeat(100), "b".repeat(90));
        let offenses = check(&source);
        assert_eq!(offenses.len(), 2);
        assert_eq!(offenses[0].location.line, 2);
        assert_eq!(offenses[1].location.line, 4);
    }

    #[test]
    fn counts_unicode_correctly() {
        let emojis = "🎉".repeat(80);
        let offenses = check(&emojis);
        assert_eq!(offenses.len(), 0);

        let emojis = "🎉".repeat(81);
        let offenses = check(&emojis);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn ignores_shebang() {
        let source = format!("#!/usr/bin/env ruby {}\nputs 'ok'", "x".repeat(100));
        let offenses = check(&source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn ignores_lines_with_uri() {
        let source = format!(
            "# See: https://example.com/very/long/path/to/resource{}",
            "/x".repeat(50)
        );
        let offenses = check(&source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn message_format_matches_rubocop() {
        let line = "x".repeat(100);
        let offenses = check(&line);
        assert_eq!(offenses[0].message, "Line is too long. [100/80]");
    }

    #[test]
    fn skips_lines_after_end() {
        let source = format!("{}\n__END__\n{}", "x".repeat(81), "y".repeat(200));
        let offenses = check(&source);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].location.line, 1);
    }
}
