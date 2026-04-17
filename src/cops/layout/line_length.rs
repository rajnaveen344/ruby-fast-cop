use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
pub enum AllowHeredoc {
    Disabled,
    All,
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
    tab_width: usize,
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
        max: usize, allow_uri: bool, allow_heredoc: AllowHeredoc,
        allow_qualified_name: bool, allow_cop_directives: bool,
        allow_rbs_inline_annotation: bool, uri_schemes: Vec<String>,
        allowed_patterns: Vec<String>, tab_width: usize, split_strings: bool,
    ) -> Self {
        Self {
            max, allow_uri, allow_heredoc, allow_qualified_name, allow_cop_directives,
            allow_rbs_inline_annotation, uri_schemes, allowed_patterns, tab_width, split_strings,
        }
    }

    pub fn default_max() -> usize { 120 }

    fn line_length(&self, line: &str) -> usize {
        line.chars().count() + self.indentation_difference(line)
    }

    fn indentation_difference(&self, line: &str) -> usize {
        if self.tab_width <= 1 || !line.starts_with('\t') { return 0; }
        line.chars().take_while(|&c| c == '\t').count() * (self.tab_width - 1)
    }

    fn highlight_start(&self, line: &str) -> usize {
        let diff = self.indentation_difference(line);
        self.max.saturating_sub(diff)
    }

    fn matches_allowed_pattern(&self, line: &str) -> bool {
        self.allowed_patterns.iter().any(|pattern| {
            Regex::new(pattern.trim_matches('/')).map_or(false, |re| re.is_match(line))
        })
    }

    fn find_heredoc_body_lines(source: &str) -> Vec<(usize, Vec<String>)> {
        let lines: Vec<&str> = source.lines().collect();
        let heredoc_re = Regex::new(r#"<<[-~]?['"]?(\w+)['"]?"#).unwrap();
        let mut result = Vec::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        let mut nesting: Vec<String> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if let Some(current_delim) = queue.front().cloned() {
                if nesting.last().map_or(true, |d| *d != current_delim) {
                    nesting.push(current_delim.clone());
                }
                if line.trim() == current_delim {
                    queue.pop_front();
                    nesting.pop();
                } else {
                    result.push((i, nesting.clone()));
                    let openings: Vec<_> = heredoc_re.captures_iter(line).map(|c| c[1].to_string()).collect();
                    for delim in openings.into_iter().rev() {
                        queue.push_front(delim);
                    }
                }
            } else {
                nesting.clear();
                for cap in heredoc_re.captures_iter(line) {
                    queue.push_back(cap[1].to_string());
                }
            }
        }
        result
    }

    fn is_in_permitted_heredoc(&self, line_index: usize, heredoc_lines: &[(usize, Vec<String>)]) -> bool {
        match &self.allow_heredoc {
            AllowHeredoc::Disabled => false,
            AllowHeredoc::All => heredoc_lines.iter().any(|(idx, _)| *idx == line_index),
            AllowHeredoc::Specific(delimiters) => heredoc_lines.iter().any(|(idx, enclosing)| {
                *idx == line_index && enclosing.iter().any(|d| delimiters.contains(d))
            }),
        }
    }

    fn is_rbs_annotation(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.starts_with("#:") || trimmed.starts_with("#|") { return true; }
        for marker in &[" #:", "\t#:", " #|", "\t#|"] {
            if let Some(pos) = line.rfind(marker) {
                if pos > 0 { return true; }
            }
        }
        false
    }

    fn cop_directive_regex() -> Regex {
        Regex::new(r"#\s*rubocop\s*:\s*(?:disable|enable|todo)\b").unwrap()
    }

    fn has_cop_directive(line: &str) -> bool {
        Self::cop_directive_regex().is_match(line)
    }

    fn line_length_without_directive(&self, line: &str) -> usize {
        if let Some(m) = Self::cop_directive_regex().find(line) {
            let trimmed = line[..m.start()].trim_end();
            trimmed.len() + self.indentation_difference(trimmed)
        } else {
            self.line_length(line)
        }
    }

    fn find_last_uri_match(&self, line: &str) -> Option<(usize, usize)> {
        if self.uri_schemes.is_empty() { return None; }
        let mut last_match: Option<(usize, usize)> = None;

        for scheme in &self.uri_schemes {
            let needle = format!("{}://", scheme);
            let mut search_from = 0;
            while let Some(byte_pos) = line[search_from..].find(&needle) {
                let abs_start = search_from + byte_pos;
                let uri_end = line[abs_start..].find(char::is_whitespace).unwrap_or(line.len() - abs_start);
                let abs_end = abs_start + uri_end;
                let char_start = line[..abs_start].chars().count();
                let char_end = line[..abs_end].chars().count();
                if last_match.map_or(true, |(prev, _)| char_start > prev) {
                    last_match = Some((char_start, char_end));
                }
                search_from = abs_end;
            }
        }
        last_match
    }

    fn find_last_qn_match(line: &str) -> Option<(usize, usize)> {
        let re = Regex::new(r"\b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b").unwrap();
        re.find_iter(line).last().map(|m| {
            (line[..m.start()].chars().count(), line[..m.end()].chars().count())
        })
    }

    fn extend_end_position(line: &str, char_end: usize) -> usize {
        let chars: Vec<char> = line.chars().collect();
        let mut end = char_end;
        if Self::has_yard_braces(line) {
            if let Some(brace_pos) = chars[end..].iter().rposition(|&c| c == '}') {
                end += brace_pos + 1;
            }
        }
        while end < chars.len() && !chars[end].is_whitespace() { end += 1; }
        end
    }

    fn has_yard_braces(line: &str) -> bool {
        let trimmed = line.trim_end();
        trimmed.ends_with('}') && trimmed.contains('{')
    }

    /// Find excessive range for URI or qualified name match.
    /// Returns adjusted (begin, end) with indentation_difference applied, or None if before max.
    fn find_excessive_range(&self, line: &str, raw_match: Option<(usize, usize)>) -> Option<(usize, usize)> {
        let (begin, end) = raw_match?;
        let end = Self::extend_end_position(line, end);
        let diff = self.indentation_difference(line);
        let (adj_begin, adj_end) = (begin + diff, end + diff);
        if adj_begin < self.max && adj_end < self.max { return None; }
        Some((adj_begin, adj_end))
    }

    fn allowed_position(&self, range: (usize, usize), line: &str) -> bool {
        range.0 < self.max && range.1 == self.line_length(line)
    }

    fn allowed_combination(&self, line: &str, uri: &Option<(usize, usize)>, qn: &Option<(usize, usize)>) -> bool {
        match (uri, qn) {
            (Some(ur), Some(qr)) => self.allowed_position(*ur, line) && self.allowed_position(*qr, line),
            (Some(ur), None) => self.allowed_position(*ur, line),
            (None, Some(qr)) => self.allowed_position(*qr, line),
            (None, None) => false,
        }
    }

    fn excess_position(&self, line: &str, range: &Option<(usize, usize)>) -> usize {
        if let Some((begin, end)) = range {
            if *begin < self.max {
                return end.saturating_sub(self.indentation_difference(line));
            }
        }
        self.highlight_start(line)
    }
}

// Autocorrect types and AST visitor

enum BreakKind {
    Newline { offset: usize },
    MultiNewline { offsets: Vec<usize> },
    BlockBreak { start: usize, end: usize },
    SemicolonBreak { start: usize, end: usize },
    StringSplit { offset: usize, delimiter: char },
    MultiStringSplit { offsets: Vec<usize>, delimiter: char },
}

struct BreakableRangeFinder<'a> {
    source: &'a str,
    max: usize,
    split_strings: bool,
    ranges: HashMap<usize, BreakKind>,
    depth: usize,
    line_starts: Vec<usize>,
}

impl<'a> BreakableRangeFinder<'a> {
    fn new(source: &'a str, max: usize, split_strings: bool) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' { line_starts.push(i + 1); }
        }
        Self { source, max, split_strings, ranges: HashMap::new(), depth: 0, line_starts }
    }

    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    fn col_of(&self, offset: usize) -> usize {
        let line_idx = self.line_of(offset);
        self.source[self.line_starts[line_idx]..offset].chars().count()
    }

    fn is_single_line(&self, start: usize, end: usize) -> bool {
        self.line_of(start) == self.line_of(end.saturating_sub(1).max(start))
    }

    fn register(&mut self, line_idx: usize, kind: BreakKind) {
        self.ranges.entry(line_idx).or_insert(kind);
    }

    fn find_break_in_elements(&self, elements: &[(usize, usize)], open_paren_offset: Option<usize>) -> Option<usize> {
        if elements.is_empty() { return None; }

        let mut last_fitting_idx: Option<usize> = None;
        for (i, &(_start, end)) in elements.iter().enumerate() {
            let fits = if i + 1 < elements.len() {
                self.col_of(elements[i + 1].0) <= self.max
            } else {
                self.col_of(end) <= self.max
            };
            if fits { last_fitting_idx = Some(i); } else { break; }
        }

        match last_fitting_idx {
            Some(idx) if idx + 1 < elements.len() => Some(elements[idx + 1].0),
            Some(_) => None,
            None => {
                if open_paren_offset.is_some() && !elements.is_empty() {
                    Some(elements[0].0)
                } else if elements.len() >= 2 {
                    Some(elements[1].0)
                } else {
                    None
                }
            }
        }
    }

    /// Check if a node's opening location starts with `<<` (heredoc).
    fn is_heredoc_opening(&self, open_start: usize, open_end: usize) -> bool {
        self.source[open_start..open_end].starts_with("<<")
    }

    fn has_heredoc_arg(&self, args: &ruby_prism::NodeList) -> bool {
        args.iter().any(|arg| self.is_heredoc_node(&arg))
    }

    fn is_heredoc_node(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::StringNode { .. } => {
                node.as_string_node().unwrap().opening_loc()
                    .map_or(false, |loc| self.is_heredoc_opening(loc.start_offset(), loc.end_offset()))
            }
            ruby_prism::Node::InterpolatedStringNode { .. } => {
                node.as_interpolated_string_node().unwrap().opening_loc()
                    .map_or(false, |loc| self.is_heredoc_opening(loc.start_offset(), loc.end_offset()))
            }
            ruby_prism::Node::XStringNode { .. } => {
                let n = node.as_x_string_node().unwrap();
                self.is_heredoc_opening(n.opening_loc().start_offset(), n.opening_loc().end_offset())
            }
            _ => false,
        }
    }

    fn process_call_args(
        &mut self, node_start: usize, node_end: usize,
        arguments: Option<&ruby_prism::ArgumentsNode>,
        opening_loc: Option<ruby_prism::Location>, line_idx: usize,
    ) {
        let args = match arguments { Some(a) => a, None => return };
        let arg_list = args.arguments();
        if arg_list.is_empty() || !self.is_single_line(node_start, node_end) { return; }

        let is_parenthesized = opening_loc.is_some();

        if self.has_heredoc_arg(&arg_list) {
            let non_heredoc: Vec<_> = arg_list.iter()
                .take_while(|arg| !self.is_heredoc_node(arg))
                .map(|arg| (arg.location().start_offset(), arg.location().end_offset()))
                .collect();
            if non_heredoc.is_empty() { return; }
            if let Some(offset) = self.find_break_in_elements(&non_heredoc, opening_loc.as_ref().map(|l| l.start_offset())) {
                self.register(line_idx, BreakKind::Newline { offset });
            }
            return;
        }

        // Single HashNode argument: break inside hash rather than after call paren
        if arg_list.len() == 1 {
            if let Some(hash) = arg_list.iter().next().and_then(|a| {
                if matches!(a, ruby_prism::Node::HashNode{..}) { a.as_hash_node() } else { None }
            }) {
                let elems: Vec<_> = hash.elements().iter()
                    .map(|e| (e.location().start_offset(), e.location().end_offset())).collect();
                if elems.len() >= 2 {
                    if let Some(offset) = self.find_break_in_elements(&elems, Some(hash.opening_loc().start_offset())) {
                        self.register(line_idx, BreakKind::Newline { offset });
                    }
                    return;
                }
            }
        }

        // Flatten keyword hash arguments
        let mut elements: Vec<(usize, usize)> = Vec::new();
        for arg in arg_list.iter() {
            if let ruby_prism::Node::KeywordHashNode { .. } = arg {
                for elem in arg.as_keyword_hash_node().unwrap().elements().iter() {
                    elements.push((elem.location().start_offset(), elem.location().end_offset()));
                }
            } else {
                elements.push((arg.location().start_offset(), arg.location().end_offset()));
            }
        }

        if !is_parenthesized && elements.len() == 1 { return; }

        if let Some(offset) = self.find_break_in_elements(&elements, opening_loc.as_ref().map(|l| l.start_offset())) {
            self.register(line_idx, BreakKind::Newline { offset });
        }
    }

    fn process_collection_elements(
        &mut self, node_start: usize, node_end: usize,
        elements: &[(usize, usize)], open_offset: Option<usize>, line_idx: usize,
    ) {
        if elements.is_empty() || !self.is_single_line(node_start, node_end) { return; }
        if let Some(offset) = self.find_break_in_elements(elements, open_offset) {
            self.register(line_idx, BreakKind::Newline { offset });
        }
    }

    fn receiver_has_heredoc(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if call.arguments().map_or(false, |a| self.has_heredoc_arg(&a.arguments())) {
                    return true;
                }
                call.receiver().map_or(false, |r| self.receiver_has_heredoc(&r))
            }
            ruby_prism::Node::StringNode { .. } | ruby_prism::Node::InterpolatedStringNode { .. }
            | ruby_prism::Node::XStringNode { .. } => self.is_heredoc_node(node),
            ruby_prism::Node::ArrayNode { .. } => {
                node.as_array_node().unwrap().elements().iter().any(|e| self.receiver_has_heredoc(&e))
            }
            _ => false,
        }
    }

    fn process_block(
        &mut self, node_start: usize, node_end: usize,
        params_end_offset: Option<usize>, opening_end_offset: usize,
        has_body: bool, call_node: Option<&ruby_prism::Node>, line_idx: usize,
    ) {
        if !self.is_single_line(node_start, node_end) { return; }
        if let Some(call) = call_node {
            if self.receiver_has_heredoc(call) { return; }
        }

        let break_after = params_end_offset.unwrap_or(opening_end_offset);
        let skip_ws = self.source[break_after..].bytes().take_while(|&b| b == b' ').count();
        let body_offset = break_after + skip_ws;

        if has_body || body_offset < node_end {
            self.register(line_idx, BreakKind::BlockBreak { start: break_after, end: body_offset });
        }
    }

    fn find_semicolon_breaks(&mut self) {
        let heredoc_re = Regex::new(r#"<<[-~]?['"`]?(\w+)['"`]?"#).unwrap();
        let lines: Vec<&str> = self.source.lines().collect();
        let mut in_heredoc = false;
        let mut heredoc_delim: Option<String> = None;
        let mut byte_offset = 0usize;

        for (line_idx, line) in lines.iter().enumerate() {
            if in_heredoc {
                if line.trim() == heredoc_delim.as_deref().unwrap_or("") {
                    in_heredoc = false;
                    heredoc_delim = None;
                }
                byte_offset += line.len() + 1;
                continue;
            }
            if let Some(cap) = heredoc_re.captures(line) {
                in_heredoc = true;
                heredoc_delim = Some(cap[1].to_string());
            }
            if line.chars().count() <= self.max || line.trim_start().starts_with('#') {
                byte_offset += line.len() + 1;
                continue;
            }
            if let Some(semi_break) = self.find_semicolon_in_line(line, byte_offset) {
                self.ranges.entry(line_idx).or_insert(semi_break);
            }
            byte_offset += line.len() + 1;
        }
    }

    fn find_semicolon_in_line(&self, line: &str, line_byte_offset: usize) -> Option<BreakKind> {
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escape_next = false;
        let mut best_semi_byte: Option<usize> = None;
        let mut byte_idx = 0usize;

        for (_char_idx, ch) in line.chars().enumerate() {
            let ch_len = ch.len_utf8();
            if escape_next { escape_next = false; byte_idx += ch_len; continue; }
            if ch == '\\' && (in_single_quote || in_double_quote) { escape_next = true; byte_idx += ch_len; continue; }

            if ch == '\'' && !in_double_quote { in_single_quote = !in_single_quote; }
            else if ch == '"' && !in_single_quote { in_double_quote = !in_double_quote; }
            else if ch == ';' && !in_single_quote && !in_double_quote { best_semi_byte = Some(byte_idx); }

            byte_idx += ch_len;
        }

        let semi_byte_idx = best_semi_byte?;
        let after_semi = &line[semi_byte_idx + 1..];

        // Find where trailing semicolons end
        let mut all_semis_end = semi_byte_idx + 1;
        for ch in after_semi.chars() {
            if ch == ';' { all_semis_end += 1; } else { break; }
        }

        let after_all_semis = &line[all_semis_end..];
        if after_all_semis.trim_start().is_empty() { return None; }

        let ws_after = after_all_semis.len() - after_all_semis.trim_start().len();
        Some(BreakKind::SemicolonBreak {
            start: line_byte_offset + all_semis_end,
            end: line_byte_offset + all_semis_end + ws_after,
        })
    }

    fn find_string_splits(&mut self) {
        if !self.split_strings { return; }
        let result = ruby_prism::parse(self.source.as_bytes());
        let root = result.node();
        let mut finder = StringSplitFinder {
            source: self.source, max: self.max, line_starts: &self.line_starts, splits: Vec::new(),
        };
        finder.visit(&root);
        for (line_idx, kind) in finder.splits {
            self.ranges.entry(line_idx).or_insert(kind);
        }
    }
}

impl Visit<'_> for BreakableRangeFinder<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        self.depth += 1;
        if self.depth == 1 && self.is_single_line(start, end) && !self.ranges.contains_key(&line_idx) {
            self.process_call_args(start, end, node.arguments().as_ref(), node.opening_loc(), line_idx);
        }
        ruby_prism::visit_call_node(self, node);
        self.depth -= 1;
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let start = node.location().start_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) {
            if let Some(params) = node.parameters() {
                let params_end = params.location().end_offset();
                if self.is_single_line(start, params_end) {
                    let param_list: Vec<_> = params.requireds().iter()
                        .chain(params.optionals().iter())
                        .chain(params.keywords().iter())
                        .chain(params.posts().iter())
                        .map(|p| (p.location().start_offset(), p.location().end_offset()))
                        .collect();
                    if !param_list.is_empty() {
                        if let Some(offset) = self.find_break_in_elements(&param_list, node.lparen_loc().map(|l| l.start_offset())) {
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

        self.depth += 1;
        if self.depth == 1 && self.is_single_line(start, end) && !self.ranges.contains_key(&line_idx) {
            let elements: Vec<_> = node.elements().iter()
                .map(|e| (e.location().start_offset(), e.location().end_offset())).collect();
            if elements.len() >= 2 {
                self.process_collection_elements(start, end, &elements, Some(node.opening_loc().start_offset()), line_idx);
            }
        }
        ruby_prism::visit_hash_node(self, node);
        self.depth -= 1;
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        self.depth += 1;
        if self.depth == 1 && self.is_single_line(start, end) && !self.ranges.contains_key(&line_idx) {
            if !node.elements().iter().any(|e| self.is_heredoc_node(&e)) {
                let elements: Vec<_> = node.elements().iter()
                    .map(|e| (e.location().start_offset(), e.location().end_offset())).collect();
                if elements.len() >= 2 {
                    self.process_collection_elements(start, end, &elements, node.opening_loc().map(|l| l.start_offset()), line_idx);
                }
            }
        }
        ruby_prism::visit_array_node(self, node);
        self.depth -= 1;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) && self.is_single_line(start, end) {
            let params_end = node.parameters().and_then(|p| {
                if p.location().start_offset() == start { None } else { Some(p.location().end_offset()) }
            });
            self.process_block(start, end, params_end, node.opening_loc().end_offset(), node.body().is_some(), None, line_idx);
        }
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) && self.is_single_line(start, end) {
            self.process_block(start, end, None, node.opening_loc().end_offset(), node.body().is_some(), None, line_idx);
        }
        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let line_idx = self.line_of(start);

        if !self.ranges.contains_key(&line_idx) && self.is_single_line(start, end) {
            let value = node.value();
            let value_start = value.location().start_offset();
            let value_end = value.location().end_offset();
            let value_text = &self.source[value_start..value_end];
            if value_text.contains(',') {
                let mut offsets = vec![value_start];

                let mut byte_pos = 0usize;
                let mut char_pos = 0usize;
                let mut last_comma_break: Option<usize> = None;
                let value_bytes = value_text.as_bytes();
                while byte_pos < value_bytes.len() {
                    let ch = value_text[byte_pos..].chars().next().unwrap();
                    let ch_len = ch.len_utf8();
                    if ch == ',' && byte_pos + 1 < value_bytes.len() {
                        let ws = value_text[byte_pos + 1..].bytes().take_while(|&b| b == b' ').count();
                        let break_offset = value_start + byte_pos + 1 + ws;
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

// String split visitor
struct StringSplitFinder<'a> {
    source: &'a str,
    max: usize,
    line_starts: &'a [usize],
    splits: Vec<(usize, BreakKind)>,
}

impl<'a> StringSplitFinder<'a> {
    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i, Err(i) => i.saturating_sub(1),
        }
    }

    fn col_of(&self, offset: usize) -> usize {
        let li = self.line_of(offset);
        self.source[self.line_starts[li]..offset].chars().count()
    }

    fn is_single_line(&self, start: usize, end: usize) -> bool {
        self.line_of(start) == self.line_of(end.saturating_sub(1).max(start))
    }

    fn line_too_long(&self, line_idx: usize) -> bool {
        let start = self.line_starts[line_idx];
        let end = if line_idx + 1 < self.line_starts.len() {
            self.line_starts[line_idx + 1] - 1
        } else {
            self.source.len()
        };
        self.source[start..end].chars().count() > self.max
    }

    fn check_string_node(&mut self, start: usize, end: usize, opening_loc: Option<ruby_prism::Location>, closing_loc: Option<ruby_prism::Location>, is_interpolated: bool) {
        if !self.is_single_line(start, end) { return; }
        let line_idx = self.line_of(start);
        if !self.line_too_long(line_idx) { return; }

        let opening_loc = match opening_loc { Some(l) => l, None => return };
        let opening = &self.source[opening_loc.start_offset()..opening_loc.end_offset()];
        if opening.starts_with("<<") || opening.starts_with('%') { return; }

        let delimiter = if is_interpolated || opening.contains('"') { '"' } else { '\'' };
        let string_start_col = self.col_of(start);
        if string_start_col >= self.max { return; }

        let closing_loc = match closing_loc { Some(l) => l, None => return };
        let content_start = opening_loc.end_offset();
        let content_end = closing_loc.start_offset();
        let content = &self.source[content_start..content_end];
        if content.is_empty() { return; }

        if is_interpolated && content.starts_with("#{") && content.ends_with('}') && Self::is_single_interpolation(content) {
            return;
        }

        let max_content_chars = if self.max > string_start_col + 4 {
            self.max - string_start_col - 4
        } else { return };

        if is_interpolated {
            if let Some(offset) = self.find_interpolated_split(content, content_start, max_content_chars) {
                self.splits.push((line_idx, BreakKind::StringSplit { offset, delimiter }));
            }
        } else {
            let mut split_offsets = Vec::new();
            let mut remaining_start = content_start;
            let mut current_start_col = string_start_col;

            loop {
                match self.find_split_offset(remaining_start, content_end, current_start_col, delimiter) {
                    Some(split_offset) => {
                        split_offsets.push(split_offset);
                        remaining_start = split_offset;
                        current_start_col = 0;
                        let remaining = &self.source[remaining_start..content_end];
                        if remaining.chars().count() + 2 <= self.max { break; }
                    }
                    None => break,
                }
            }

            if split_offsets.len() == 1 {
                self.splits.push((line_idx, BreakKind::StringSplit { offset: split_offsets[0], delimiter }));
            } else if split_offsets.len() > 1 {
                self.splits.push((line_idx, BreakKind::MultiStringSplit { offsets: split_offsets, delimiter }));
            }
        }
    }

    fn find_split_offset(&self, content_start: usize, content_end: usize, string_start_col: usize, delimiter: char) -> Option<usize> {
        if string_start_col >= self.max { return None; }
        let content = &self.source[content_start..content_end];
        if content.is_empty() { return None; }

        let max_content_chars = if self.max > string_start_col + 4 {
            self.max - string_start_col - 4
        } else { return None };

        let mut best_space: Option<usize> = None;
        let mut char_count = 0usize;
        let mut byte_idx = 0usize;
        let content_bytes = content.as_bytes();

        while byte_idx < content_bytes.len() {
            let ch = content[byte_idx..].chars().next().unwrap();
            let ch_len = ch.len_utf8();
            if char_count >= max_content_chars { break; }
            if ch == ' ' && char_count + 1 <= max_content_chars {
                best_space = Some(content_start + byte_idx + ch_len);
            }
            char_count += 1;
            byte_idx += ch_len;
        }

        if let Some(offset) = best_space { return Some(offset); }

        // Find byte position at split point
        let mut byte_at_split = 0usize;
        let mut ci = 0usize;
        let mut si = 0usize;
        while si < content_bytes.len() {
            if ci == max_content_chars { byte_at_split = si; break; }
            ci += 1;
            si += content[si..].chars().next().unwrap().len_utf8();
        }
        if si >= content_bytes.len() { byte_at_split = content_bytes.len(); }

        // Check for escape sequences near split point
        let lookback_start = byte_at_split.saturating_sub(6);
        let near_split = &content[lookback_start..byte_at_split.min(content_bytes.len())];
        if let Some(esc_pos) = near_split.rfind('\\') {
            let abs_esc = lookback_start + esc_pos;
            if abs_esc < byte_at_split && byte_at_split < content_bytes.len() {
                if matches!(content.as_bytes().get(abs_esc + 1), Some(b'n') | Some(b'u') | Some(b'x')) {
                    return Some(content_start + abs_esc);
                }
            }
        }

        // Check for interpolation near split point
        if delimiter == '"' {
            if let Some(interp_pos) = content[..byte_at_split.min(content_bytes.len())].rfind("#{") {
                let interp_col = content[..interp_pos].chars().count();
                if interp_col + string_start_col + 1 >= self.max.saturating_sub(4) && interp_pos > 0 {
                    return Some(content_start + interp_pos);
                }
            }
        }

        if byte_at_split > 0 && byte_at_split < content_bytes.len() {
            Some(content_start + byte_at_split)
        } else {
            None
        }
    }

    fn is_single_interpolation(content: &str) -> bool {
        if !content.starts_with("#{") { return false; }
        let mut depth = 0;
        for (i, ch) in content.char_indices() {
            if ch == '{' { depth += 1; }
            else if ch == '}' {
                depth -= 1;
                if depth == 0 { return i == content.len() - 1; }
            }
        }
        false
    }

    fn find_interpolated_split(&self, content: &str, content_start: usize, max_content_chars: usize) -> Option<usize> {
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

            if !in_interp && byte_idx + 1 < bytes.len() && bytes[byte_idx] == b'#' && bytes[byte_idx + 1] == b'{' {
                in_interp = true;
                interp_depth = 1;
                last_interp_start = Some(byte_idx);
                if char_count >= max_content_chars.saturating_sub(2) && byte_idx > 0 {
                    return Some(content_start + byte_idx);
                }
                char_count += 1;
                byte_idx += ch_len;
                continue;
            }

            if in_interp {
                match ch {
                    '{' => interp_depth += 1,
                    '}' => { interp_depth -= 1; if interp_depth == 0 { in_interp = false; } }
                    _ => {}
                }
                char_count += 1;
                byte_idx += ch_len;
                continue;
            }

            if char_count >= max_content_chars { break; }
            if ch == ' ' && char_count + 1 <= max_content_chars {
                best_space = Some(content_start + byte_idx + ch_len);
            }
            char_count += 1;
            byte_idx += ch_len;
        }

        if let Some(offset) = best_space { return Some(offset); }

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
            if c == max_content_chars && bi > 0 && bi < bytes.len() {
                return Some(content_start + bi);
            }
            bi += content[bi..].chars().next().unwrap().len_utf8();
            c += 1;
        }
        None
    }
}

impl Visit<'_> for StringSplitFinder<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_string_node(start, end, node.opening_loc(), node.closing_loc(), false);
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.check_string_node(start, end, node.opening_loc(), node.closing_loc(), true);
    }
}

impl Default for LineLength {
    fn default() -> Self { Self::new(Self::default_max()) }
}

impl Cop for LineLength {
    fn name(&self) -> &'static str { "Layout/LineLength" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let breakable_ranges = {
            let result = ruby_prism::parse(ctx.source.as_bytes());
            let root = result.node();
            let mut finder = BreakableRangeFinder::new(ctx.source, self.max, self.split_strings);
            finder.visit(&root);
            finder.find_semicolon_breaks();
            finder.find_string_splits();
            finder.ranges
        };

        let mut offenses = Vec::new();
        let mut past_end = false;

        let heredoc_lines = match &self.allow_heredoc {
            AllowHeredoc::Disabled => Vec::new(),
            _ => Self::find_heredoc_body_lines(ctx.source),
        };

        for (line_index, line) in ctx.source.lines().enumerate() {
            if line == "__END__" { past_end = true; continue; }
            if past_end { continue; }

            let visual_len = self.line_length(line);
            if visual_len <= self.max { continue; }
            if line_index == 0 && line.starts_with("#!") { continue; }
            if self.matches_allowed_pattern(line) { continue; }
            if self.is_in_permitted_heredoc(line_index, &heredoc_lines) { continue; }
            if self.allow_rbs_inline_annotation && self.is_rbs_annotation(line) { continue; }

            let char_len = line.chars().count();
            let line_num = (line_index + 1) as u32;

            let correction = breakable_ranges.get(&line_index).map(|br| Self::build_correction(br, ctx.source));

            if self.allow_cop_directives && Self::has_cop_directive(line) {
                let len_without = self.line_length_without_directive(line);
                if len_without <= self.max { continue; }
                let col_start = self.highlight_start(line) as u32;
                let indent_diff = self.indentation_difference(line);
                let col_end = if len_without > indent_diff { (len_without - indent_diff) as u32 } else { char_len as u32 };
                let mut off = Offense::new(
                    self.name(), format!("Line is too long. [{}/{}]", len_without, self.max),
                    self.severity(), Location::new(line_num, col_start, line_num, col_end), ctx.filename,
                );
                if let Some(c) = correction { off = off.with_correction(c); }
                offenses.push(off);
                continue;
            }

            if self.allow_uri || self.allow_qualified_name {
                let uri_range = if self.allow_uri { self.find_excessive_range(line, self.find_last_uri_match(line)) } else { None };
                let qn_range = if self.allow_qualified_name { self.find_excessive_range(line, Self::find_last_qn_match(line)) } else { None };

                if uri_range.is_some() || qn_range.is_some() {
                    if self.allowed_combination(line, &uri_range, &qn_range) { continue; }
                    let range = uri_range.or(qn_range);
                    let excessive_pos = self.excess_position(line, &range) as u32;
                    let mut off = Offense::new(
                        self.name(), format!("Line is too long. [{}/{}]", visual_len, self.max),
                        self.severity(), Location::new(line_num, excessive_pos, line_num, char_len as u32), ctx.filename,
                    );
                    if let Some(c) = correction { off = off.with_correction(c); }
                    offenses.push(off);
                    continue;
                }
            }

            let col_start = self.highlight_start(line) as u32;
            let mut off = Offense::new(
                self.name(), format!("Line is too long. [{}/{}]", visual_len, self.max),
                self.severity(), Location::new(line_num, col_start, line_num, char_len as u32), ctx.filename,
            );
            if let Some(c) = correction { off = off.with_correction(c); }
            offenses.push(off);
        }
        offenses
    }
}

impl LineLength {
    fn build_correction(br: &BreakKind, source: &str) -> Correction {
        use crate::offense::Edit;
        match br {
            BreakKind::Newline { offset } => Correction::replace(*offset, *offset, "\n"),
            BreakKind::MultiNewline { offsets } => Correction {
                edits: offsets.iter().map(|&off| Edit { start_offset: off, end_offset: off, replacement: "\n".to_string() }).collect(),
            },
            BreakKind::BlockBreak { start, end } => {
                if start == end { Correction::replace(*start, *end, "\n") }
                else { Correction::replace(*start, *end, "\n ") }
            }
            BreakKind::SemicolonBreak { start, end } => {
                let after = &source[*start..*end];
                let indent = if after.is_empty() { "\n ".to_string() } else { format!("\n{}", after) };
                Correction::replace(*start, *end, indent)
            }
            BreakKind::StringSplit { offset, delimiter } => {
                Correction::replace(*offset, *offset, format!("{} \\\n{}", delimiter, delimiter))
            }
            BreakKind::MultiStringSplit { offsets, delimiter } => {
                let text = format!("{} \\\n{}", delimiter, delimiter);
                Correction {
                    edits: offsets.iter().map(|&off| Edit { start_offset: off, end_offset: off, replacement: text.clone() }).collect(),
                }
            }
        }
    }
}

crate::register_cop!("Layout/LineLength", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/LineLength");
    let max = cop_config.and_then(|c| c.max).unwrap_or(120);
    let allow_uri = cop_config
        .and_then(|c| c.raw.get("AllowURI"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let allow_heredoc = cop_config
        .and_then(|c| c.raw.get("AllowHeredoc"))
        .map(|v| {
            if let Some(b) = v.as_bool() {
                if b { AllowHeredoc::All } else { AllowHeredoc::Disabled }
            } else if let Some(seq) = v.as_sequence() {
                let delimiters: Vec<String> = seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                AllowHeredoc::Specific(delimiters)
            } else {
                AllowHeredoc::Disabled
            }
        })
        .unwrap_or(AllowHeredoc::Disabled);
    let allow_qualified_name = cop_config
        .and_then(|c| c.raw.get("AllowQualifiedName"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let allow_cop_directives = cop_config
        .and_then(|c| c.raw.get("AllowCopDirectives"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let allow_rbs_inline_annotation = cop_config
        .and_then(|c| c.raw.get("AllowRBSInlineAnnotation"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let uri_schemes = cop_config
        .and_then(|c| c.raw.get("URISchemes"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_else(|| vec!["http".to_string(), "https".to_string()]);
    let allowed_patterns = cop_config
        .and_then(|c| c.raw.get("AllowedPatterns"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let tab_width = cop_config
        .and_then(|c| c.raw.get("TabWidth"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(2);
    let split_strings = cop_config
        .and_then(|c| c.raw.get("SplitStrings"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(Box::new(LineLength::with_config(
        max, allow_uri, allow_heredoc, allow_qualified_name, allow_cop_directives,
        allow_rbs_inline_annotation, uri_schemes, allowed_patterns, tab_width, split_strings,
    )))
});
