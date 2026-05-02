//! Style/Next - Use `next` to skip iteration instead of a condition at the end.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/next.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/Next";
const MSG: &str = "Use `next` to skip iteration.";

/// RuboCop's enumerator_method? check
const ENUMERATOR_METHODS: &[&str] = &[
    "collect", "collect!", "detect", "downto", "each", "each_cons", "each_key",
    "each_object", "each_pair", "each_slice", "each_value", "each_with_index",
    "each_with_object", "find", "find_all", "find_index", "flat_map", "grep",
    "grep_v", "inject", "loop", "map", "map!", "max_by", "min_by", "minmax_by",
    "reduce", "reject", "reject!", "reverse_each", "select", "select!", "sort_by",
    "sum", "times", "upto",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    SkipModifierIfs,
    Always,
}

pub struct Next {
    style: EnforcedStyle,
    min_body_length: i64,
    allow_consecutive_conditionals: bool,
}

impl Next {
    pub fn new() -> Self {
        Self {
            style: EnforcedStyle::SkipModifierIfs,
            min_body_length: 1,
            allow_consecutive_conditionals: false,
        }
    }

    pub fn with_config(
        style: EnforcedStyle,
        min_body_length: i64,
        allow_consecutive_conditionals: bool,
    ) -> Self {
        Self {
            style,
            min_body_length,
            allow_consecutive_conditionals,
        }
    }
}

impl Default for Next {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for Next {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = NextVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct NextVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a Next,
    offenses: Vec<Offense>,
}

impl<'a> NextVisitor<'a> {
    fn check_body(&mut self, body: Option<Node>) {
        let body = match body {
            Some(b) => b,
            None => return,
        };

        if !self.ends_with_condition(&body) {
            return;
        }

        // Find the offending node (the last if/unless without else)
        let (off_start, off_cond_end, off_node_start, off_node_end) =
            match self.find_offense_location(&body) {
                Some(loc) => loc,
                None => return,
            };

        // AllowConsecutiveConditionals
        if self.cop.allow_consecutive_conditionals {
            if self.is_consecutive_conditional(&body, off_node_start, off_node_end) {
                return;
            }
        }

        let correction = self.build_correction_for_body(&body);
        let mut offense = self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention, off_start, off_cond_end,
        );
        if let Some(corr) = correction {
            offense = offense.with_correction(corr);
        }
        self.offenses.push(offense);
    }

    fn ends_with_condition(&self, body: &Node) -> bool {
        if self.simple_if_without_break(body) {
            return true;
        }

        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            if let Some(last) = children.last() {
                return self.simple_if_without_break(last);
            }
        }

        false
    }

    fn simple_if_without_break(&self, node: &Node) -> bool {
        if !self.if_without_else(node) {
            return false;
        }
        if self.if_else_children(node) {
            return false;
        }
        if self.allowed_modifier_if(node) {
            return false;
        }
        !self.exit_body_type(node)
    }

    fn if_without_else(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                // Not ternary
                if let Some(kw_loc) = n.if_keyword_loc() {
                    let kw = self.ctx.src(kw_loc.start_offset(), kw_loc.end_offset());
                    if kw == "?" {
                        return false;
                    }
                } else {
                    return false;
                }
                n.subsequent().is_none()
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                n.else_clause().is_none()
            }
            _ => false,
        }
    }

    fn if_else_children(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                if let Some(stmts) = n.statements() {
                    for child in stmts.body().iter() {
                        if self.has_else(&child) {
                            return true;
                        }
                    }
                }
                false
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                if let Some(stmts) = n.statements() {
                    for child in stmts.body().iter() {
                        if self.has_else(&child) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn has_else(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().subsequent().is_some(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().else_clause().is_some(),
            _ => false,
        }
    }

    fn allowed_modifier_if(&self, node: &Node) -> bool {
        let is_modifier = self.is_modifier_form(node);
        if is_modifier {
            self.cop.style == EnforcedStyle::SkipModifierIfs
        } else {
            !self.min_body_length_met(node)
        }
    }

    fn is_modifier_form(&self, node: &Node) -> bool {
        match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().end_keyword_loc().is_none(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().end_keyword_loc().is_none(),
            _ => false,
        }
    }

    fn min_body_length_met(&self, node: &Node) -> bool {
        if self.cop.min_body_length < 0 {
            return false;
        }
        let body_length = self.body_line_count(node);
        body_length >= self.cop.min_body_length as usize
    }

    fn body_line_count(&self, node: &Node) -> usize {
        let stmts = match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().statements(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().statements(),
            _ => return 0,
        };
        match stmts {
            Some(s) => {
                let first = s.body().iter().next();
                let last = s.body().iter().last();
                match (first, last) {
                    (Some(f), Some(l)) => {
                        let start_line = self.ctx.line_of(f.location().start_offset());
                        let end_line = self.ctx.line_of(l.location().end_offset());
                        end_line - start_line + 1
                    }
                    _ => 0,
                }
            }
            None => 0,
        }
    }

    fn exit_body_type(&self, node: &Node) -> bool {
        let stmts = match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().statements(),
            Node::UnlessNode { .. } => node.as_unless_node().unwrap().statements(),
            _ => return false,
        };
        match stmts {
            Some(s) => {
                // Check first child (the if_branch)
                if let Some(first) = s.body().iter().next() {
                    matches!(first, Node::BreakNode { .. } | Node::ReturnNode { .. })
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// Find the offense location (start, cond_end, node_start, node_end) for the offending if/unless.
    fn find_offense_location(&self, body: &Node) -> Option<(usize, usize, usize, usize)> {
        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            if let Some(last) = children.last() {
                if self.simple_if_without_break(last) {
                    return self.offense_loc_for_node(last);
                }
            }
        }

        // Body itself is if/unless
        if self.simple_if_without_break(body) {
            return self.offense_loc_for_node(body);
        }

        None
    }

    fn offense_loc_for_node(&self, node: &Node) -> Option<(usize, usize, usize, usize)> {
        let start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let cond_end = match node {
            Node::IfNode { .. } => {
                node.as_if_node().unwrap().predicate().location().end_offset()
            }
            Node::UnlessNode { .. } => {
                node.as_unless_node().unwrap().predicate().location().end_offset()
            }
            _ => return None,
        };
        Some((start, cond_end, start, node_end))
    }

    fn is_consecutive_conditional(&self, body: &Node, node_start: usize, node_end: usize) -> bool {
        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            for i in 1..children.len() {
                let child_start = children[i].location().start_offset();
                let child_end = children[i].location().end_offset();
                if child_start == node_start && child_end == node_end {
                    if matches!(&children[i - 1], Node::IfNode { .. } | Node::UnlessNode { .. }) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn is_enumerator_method(name: &str) -> bool {
        ENUMERATOR_METHODS.contains(&name)
    }

    /// Find the offense node within body and build correction.
    fn build_correction_for_body(&self, body: &Node) -> Option<Correction> {
        if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            if let Some(last) = children.last() {
                if self.simple_if_without_break(last) {
                    return self.build_correction(last);
                }
            }
        }
        if self.simple_if_without_break(body) {
            return self.build_correction(body);
        }
        None
    }

    /// Build correction for an if/unless node.
    fn build_correction(&self, node: &Node) -> Option<Correction> {
        let src = self.ctx.source;
        let src_bytes = src.as_bytes();

        match node {
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                let is_modifier = n.end_keyword_loc().is_none();
                let inv_kw = "unless";
                let cond = n.predicate();
                let cond_src = &src[cond.location().start_offset()..cond.location().end_offset()];

                if is_modifier {
                    // Modifier form: `body if cond` → `next unless cond\n<indent>body`
                    let body_node = n.statements()?;
                    let body_children: Vec<_> = body_node.body().iter().collect();
                    let body_src = &src[body_children.first()?.location().start_offset()
                        ..body_children.last()?.location().end_offset()];
                    let node_start = node.location().start_offset();
                    let node_end = node.location().end_offset();
                    let indent = self.ctx.col_of(node_start);
                    let replacement =
                        format!("next {} {}\n{}{}", inv_kw, cond_src, " ".repeat(indent), body_src);
                    Some(Correction {
                        edits: vec![Edit {
                            start_offset: node_start,
                            end_offset: node_end,
                            replacement,
                        }],
                    })
                } else {
                    // Block form: `if cond [then]\n  body\nend`
                    let node_start = node.location().start_offset();
                    // cond_end: after `then` keyword if present, else after predicate
                    let cond_end = if let Some(then_loc) = n.then_keyword_loc() {
                        then_loc.end_offset()
                    } else {
                        cond.location().end_offset()
                    };
                    let next_stmt = format!("next {} {}", inv_kw, cond_src);
                    // Replace node_start..cond_end with "next unless COND"
                    // (source already has \n after predicate; no trailing \n needed)
                    let mut edits = vec![Edit {
                        start_offset: node_start,
                        end_offset: cond_end,
                        replacement: next_stmt,
                    }];
                    // Remove end keyword + its line indent (and possibly preceding newline)
                    if let Some(end_loc) = n.end_keyword_loc() {
                        let end_end = end_loc.end_offset();
                        let end_start = end_loc.start_offset();
                        // line start = offset of first char on end's line
                        let end_line_start = self.ctx.line_start(end_start);
                        // Check if end is followed by whitespace-only (to include preceding \n)
                        let after_end = &src[end_end..];
                        let followed_by_ws_only = after_end.chars().next().map_or(true, |c| c == '\n' || c == '\r')
                            || after_end.starts_with('\n')
                            || after_end.trim_start_matches(|c: char| c == ' ' || c == '\t').starts_with('\n')
                            || after_end.trim_start_matches(|c: char| c == ' ' || c == '\t').is_empty();
                        let remove_start = if followed_by_ws_only && end_line_start > 0 {
                            end_line_start - 1 // include preceding \n
                        } else {
                            end_line_start
                        };
                        edits.push(Edit {
                            start_offset: remove_start,
                            end_offset: end_end,
                            replacement: String::new(),
                        });
                    }
                    // Re-indent body lines
                    let reindent_edits = self.build_reindent_edits(node, src, src_bytes, cond_end);
                    edits.extend(reindent_edits);
                    Some(Correction { edits })
                }
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                let is_modifier = n.end_keyword_loc().is_none();
                let inv_kw = "if";
                let cond = n.predicate();
                let cond_src = &src[cond.location().start_offset()..cond.location().end_offset()];

                if is_modifier {
                    let body_node = n.statements()?;
                    let body_children: Vec<_> = body_node.body().iter().collect();
                    let body_src = &src[body_children.first()?.location().start_offset()
                        ..body_children.last()?.location().end_offset()];
                    let node_start = node.location().start_offset();
                    let node_end = node.location().end_offset();
                    let indent = self.ctx.col_of(node_start);
                    let replacement =
                        format!("next {} {}\n{}{}", inv_kw, cond_src, " ".repeat(indent), body_src);
                    Some(Correction {
                        edits: vec![Edit {
                            start_offset: node_start,
                            end_offset: node_end,
                            replacement,
                        }],
                    })
                } else {
                    let node_start = node.location().start_offset();
                    let then_end = n.then_keyword_loc().map(|l| l.end_offset());
                    let cond_end = then_end.unwrap_or_else(|| cond.location().end_offset());
                    let next_stmt = format!("next {} {}", inv_kw, cond_src);
                    let mut edits = vec![Edit {
                        start_offset: node_start,
                        end_offset: cond_end,
                        replacement: next_stmt,
                    }];
                    if let Some(end_loc) = n.end_keyword_loc() {
                        let end_end = end_loc.end_offset();
                        let end_start = end_loc.start_offset();
                        let end_line_start = self.ctx.line_start(end_start);
                        let after_end = &src[end_end..];
                        let followed_by_ws_only = after_end.starts_with('\n')
                            || after_end.trim_start_matches(|c: char| c == ' ' || c == '\t').starts_with('\n')
                            || after_end.trim_start_matches(|c: char| c == ' ' || c == '\t').is_empty();
                        let remove_start = if followed_by_ws_only && end_line_start > 0 {
                            end_line_start - 1
                        } else {
                            end_line_start
                        };
                        edits.push(Edit {
                            start_offset: remove_start,
                            end_offset: end_end,
                            replacement: String::new(),
                        });
                    }
                    let reindent_edits = self.build_reindent_edits_unless(node, src, src_bytes, cond_end);
                    edits.extend(reindent_edits);
                    Some(Correction { edits })
                }
            }
            _ => None,
        }
    }

    /// Build re-indent edits for IfNode body lines.
    fn build_reindent_edits(&self, node: &Node, src: &str, _src_bytes: &[u8], _cond_end: usize) -> Vec<Edit> {
        let n = match node.as_if_node() {
            Some(n) => n,
            None => return vec![],
        };
        let end_loc = match n.end_keyword_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let cond = n.predicate();
        let target_indent = self.ctx.indentation_of(cond.location().start_offset());
        // Body lines = lines from after the `if COND` header line up to before the `end` line
        let header_end = cond.location().end_offset();
        // include any `then` keyword
        let header_end = if let Some(then_loc) = n.then_keyword_loc() {
            then_loc.end_offset()
        } else {
            header_end
        };
        let body_first_line_start = self.next_line_start(src, header_end);
        let end_line_start = self.ctx.line_start(end_loc.start_offset());
        let heredoc_ranges = self.collect_heredoc_ranges_in_source(src, body_first_line_start, end_line_start);
        self.reindent_lines_with_heredoc(src, body_first_line_start, end_line_start, target_indent, &heredoc_ranges)
    }

    /// Build re-indent edits for UnlessNode body lines.
    fn build_reindent_edits_unless(&self, node: &Node, src: &str, _src_bytes: &[u8], _cond_end: usize) -> Vec<Edit> {
        let n = match node.as_unless_node() {
            Some(n) => n,
            None => return vec![],
        };
        let end_loc = match n.end_keyword_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let cond = n.predicate();
        let target_indent = self.ctx.indentation_of(cond.location().start_offset());
        let header_end = cond.location().end_offset();
        let header_end = if let Some(then_loc) = n.then_keyword_loc() {
            then_loc.end_offset()
        } else {
            header_end
        };
        let body_first_line_start = self.next_line_start(src, header_end);
        let end_line_start = self.ctx.line_start(end_loc.start_offset());
        let heredoc_ranges = self.collect_heredoc_ranges_in_source(src, body_first_line_start, end_line_start);
        self.reindent_lines_with_heredoc(src, body_first_line_start, end_line_start, target_indent, &heredoc_ranges)
    }

    /// Byte offset of the start of the line after the line containing `offset`.
    fn next_line_start(&self, src: &str, offset: usize) -> usize {
        src[offset..].find('\n').map_or(src.len(), |p| offset + p + 1)
    }

    /// Collect byte ranges of heredoc body+closing lines in source between from_offset and to_offset.
    /// Scans source text for heredoc markers `<<[-~]?IDENT` and tracks their body line ranges.
    fn collect_heredoc_ranges_in_source(&self, src: &str, from_offset: usize, to_offset: usize) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let region = &src[from_offset..to_offset.min(src.len())];
        let src_bytes = src.as_bytes();
        let mut pos = from_offset;
        // Scan each line in the region for heredoc markers
        while pos < to_offset {
            let line_end = src[pos..].find('\n').map_or(src.len(), |p| pos + p);
            let line = &src[pos..line_end];
            // Look for `<<` heredoc markers on this line
            let mut scan = pos;
            while scan + 2 <= line_end {
                if src_bytes[scan] == b'<' && src_bytes[scan + 1] == b'<' {
                    // Check for heredoc: <<[-~]?IDENT or <<[-~]?"IDENT" etc.
                    let mut marker_pos = scan + 2;
                    if marker_pos < line_end && (src_bytes[marker_pos] == b'-' || src_bytes[marker_pos] == b'~') {
                        marker_pos += 1;
                    }
                    // Skip optional quote
                    let quote_char = if marker_pos < line_end && (src_bytes[marker_pos] == b'\'' || src_bytes[marker_pos] == b'"' || src_bytes[marker_pos] == b'`') {
                        let q = src_bytes[marker_pos];
                        marker_pos += 1;
                        Some(q)
                    } else {
                        None
                    };
                    // Read identifier
                    let ident_start = marker_pos;
                    while marker_pos < line_end && (src_bytes[marker_pos].is_ascii_alphanumeric() || src_bytes[marker_pos] == b'_') {
                        marker_pos += 1;
                    }
                    if marker_pos > ident_start {
                        let identifier = &src[ident_start..marker_pos];
                        // Closing marker: look for a line that is exactly `identifier` (possibly with indent)
                        let body_start = line_end + 1; // line after the heredoc marker
                        let mut search_pos = body_start;
                        let close_pat = identifier;
                        while search_pos < src.len() {
                            let close_line_end = src[search_pos..].find('\n').map_or(src.len(), |p| search_pos + p);
                            let close_line = &src[search_pos..close_line_end];
                            let trimmed = close_line.trim();
                            if trimmed == close_pat || (quote_char.is_some() && trimmed == close_pat) {
                                // Found closing marker. Exclude BODY lines from reindent (body_start..close_line_start).
                                // The closing marker line itself IS reindented (it's normal Ruby code position).
                                ranges.push((body_start, search_pos));
                                break;
                            }
                            search_pos = close_line_end + 1;
                        }
                    }
                    scan = marker_pos;
                } else {
                    scan += 1;
                }
            }
            pos = line_end + 1;
        }
        let _ = region;
        ranges
    }

    /// Compute re-indent edits for lines in range [from_offset, to_offset).
    /// Non-blank lines get `delta` leading spaces removed.
    /// `target_indent` = desired final indentation.
    fn reindent_lines(&self, src: &str, from_offset: usize, to_offset: usize, target_indent: usize) -> Vec<Edit> {
        self.reindent_lines_with_heredoc(src, from_offset, to_offset, target_indent, &[])
    }

    fn reindent_lines_with_heredoc(&self, src: &str, from_offset: usize, to_offset: usize, target_indent: usize, heredoc_ranges: &[(usize, usize)]) -> Vec<Edit> {
        if from_offset >= to_offset {
            return vec![];
        }

        let is_in_heredoc = |offset: usize| -> bool {
            heredoc_ranges.iter().any(|(s, e)| offset >= *s && offset < *e)
        };

        // Collect line starts, skipping heredoc body lines
        let mut line_starts: Vec<usize> = Vec::new();
        let mut pos = from_offset;
        while pos < to_offset {
            if !is_in_heredoc(pos) {
                line_starts.push(pos);
            }
            let line_end = src[pos..].find('\n').map_or(src.len(), |p| pos + p);
            pos = line_end + 1;
        }

        // Compute minimum indent of non-blank lines (excludes heredoc lines)
        let min_indent = line_starts
            .iter()
            .filter_map(|&ls| {
                let end = src[ls..].find('\n').map_or(src.len(), |p| ls + p);
                let line = &src[ls..end];
                if line.trim().is_empty() {
                    None
                } else {
                    Some(line.chars().take_while(|c| *c == ' ').count())
                }
            })
            .min()
            .unwrap_or(target_indent + 2);

        let delta = if min_indent > target_indent {
            min_indent - target_indent
        } else {
            return vec![];
        };

        // For each non-blank, non-heredoc line, remove `delta` leading spaces
        let mut edits = Vec::new();
        for &ls in &line_starts {
            let end = src[ls..].find('\n').map_or(src.len(), |p| ls + p);
            let line = &src[ls..end];
            if line.trim().is_empty() {
                continue;
            }
            let actual_spaces = line.chars().take_while(|c| *c == ' ').count();
            let remove = delta.min(actual_spaces);
            if remove > 0 {
                edits.push(Edit {
                    start_offset: ls,
                    end_offset: ls + remove,
                    replacement: String::new(),
                });
            }
        }
        edits
    }
}

impl Visit<'_> for NextVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        if Self::is_enumerator_method(&method_name) {
            if let Some(block) = node.block() {
                if let Some(block_node) = block.as_block_node() {
                    self.check_body(block_node.body());
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if let Some(stmts) = node.statements() {
            self.check_body(Some(stmts.as_node()));
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if let Some(stmts) = node.statements() {
            self.check_body(Some(stmts.as_node()));
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        if let Some(stmts) = node.statements() {
            self.check_body(Some(stmts.as_node()));
        }
        ruby_prism::visit_for_node(self, node);
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String, min_body_length: i64, allow_consecutive_conditionals: bool }
impl Default for Cfg {
    fn default() -> Self {
        Self { enforced_style: String::new(), min_body_length: 1, allow_consecutive_conditionals: false }
    }
}

crate::register_cop!("Style/Next", |cfg| {
    let c: Cfg = cfg.typed("Style/Next");
    let style = match c.enforced_style.as_str() {
        "always" => EnforcedStyle::Always,
        _ => EnforcedStyle::SkipModifierIfs,
    };
    Some(Box::new(Next::with_config(style, c.min_body_length, c.allow_consecutive_conditionals)))
});
