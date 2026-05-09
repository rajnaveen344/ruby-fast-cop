//! Style/AccessorGrouping cop
//!
//! Checks for grouping of accessors in class and module bodies.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const GROUPED_MSG: &str = "Group together all `%accessor%` attributes.";
const SEPARATED_MSG: &str = "Use one attribute per `%accessor%`.";

const ACCESSOR_METHODS: &[&[u8]] = &[b"attr_reader", b"attr_writer", b"attr_accessor"];

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Grouped,
    Separated,
}

pub struct AccessorGrouping {
    style: EnforcedStyle,
}

impl AccessorGrouping {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Default for AccessorGrouping {
    fn default() -> Self {
        Self::new(EnforcedStyle::Grouped)
    }
}

impl Cop for AccessorGrouping {
    fn name(&self) -> &'static str {
        "Style/AccessorGrouping"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = AccessorGroupingVisitor { ctx, cop: self, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct AccessorGroupingVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a AccessorGrouping,
    offenses: Vec<Offense>,
}

impl<'a> AccessorGroupingVisitor<'a> {
    fn node_src(&self, node: &Node) -> &str {
        let s = node.location().start_offset();
        let e = node.location().end_offset();
        &self.ctx.source[s..e]
    }

    fn is_accessor_method(name: &[u8]) -> bool {
        ACCESSOR_METHODS.contains(&name)
    }

    fn get_call_method_name<'b>(node: &'b Node) -> Option<&'b [u8]> {
        node.as_call_node().map(|c| c.name().as_slice())
    }

    fn is_comment_line(&self, line_idx: usize) -> bool {
        // line_idx is 0-based line index
        let src = self.ctx.source;
        let lines: Vec<&str> = src.lines().collect();
        if line_idx >= lines.len() {
            return false;
        }
        lines[line_idx].trim().starts_with('#')
    }

    fn line_of_offset(&self, offset: usize) -> usize {
        let bytes = self.ctx.source.as_bytes();
        let mut line = 0usize; // 0-based
        for &b in &bytes[..offset.min(bytes.len())] {
            if b == b'\n' { line += 1; }
        }
        line
    }

    fn has_comment_before(&self, node: &Node) -> bool {
        let start_line = self.line_of_offset(node.location().start_offset());
        if start_line == 0 { return false; }
        self.is_comment_line(start_line - 1)
    }

    fn has_rbs_inline_comment_after(&self, node: &Node) -> bool {
        // Check for `#: ...` RBS inline annotation on same line as node.
        // RBS inline annotations start with `#:` directly (not inside a regular comment).
        let end_line = self.line_of_offset(node.location().end_offset());
        let src = self.ctx.source;
        let lines: Vec<&str> = src.lines().collect();
        if end_line >= lines.len() { return false; }
        let line = lines[end_line];
        // Find the first `#` on the line (after code)
        // If the first `#` is followed by `:`, it's an RBS annotation.
        // If the first `#` is followed by something else, it's a regular comment (not RBS).
        let node_end_col = {
            let line_start_off = {
                let bytes = src.as_bytes();
                let mut off = 0usize;
                for i in 0..end_line {
                    while off < bytes.len() && bytes[off] != b'\n' { off += 1; }
                    off += 1; // skip \n
                }
                off
            };
            node.location().end_offset().saturating_sub(line_start_off)
        };
        let after_node = &line[node_end_col.min(line.len())..];
        // Find the first `#` in after_node
        if let Some(hash_pos) = after_node.find('#') {
            let after_hash = &after_node[hash_pos + 1..];
            // RBS annotation: `#:` (colon immediately after hash)
            return after_hash.starts_with(':');
        }
        false
    }

    fn has_prev_sorbet_sig(&self, siblings: &[Node], idx: usize) -> bool {
        if idx == 0 { return false; }
        let prev = &siblings[idx - 1];
        // Check if previous sibling is a block node (Sorbet sig { ... })
        match prev {
            Node::BlockNode { .. } => {
                // BlockNode is child of CallNode — we can't get parent here
                // Approximate: if previous is a BlockNode, assume it might be a sig block
                // Check the source text for `sig` before the block
                let block_start = prev.location().start_offset();
                if block_start >= 3 {
                    let before = &self.ctx.source[..block_start].trim_end();
                    before.ends_with("sig")
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn has_prev_non_accessor_send(&self, siblings: &[Node], idx: usize) -> bool {
        if idx == 0 { return false; }
        let prev = &siblings[idx - 1];
        match prev {
            Node::CallNode { .. } => {
                let call = prev.as_call_node().unwrap();
                let name = call.name().as_slice();
                // If prev is an accessor, no problem
                if Self::is_accessor_method(name) {
                    return false;
                }
                // If prev is an access modifier (private/protected/public), no problem
                if matches!(name, b"private" | b"protected" | b"public" | b"private_class_method" | b"public_class_method") {
                    return false;
                }
                // Otherwise it's a "method call before accessor" — allow
                true
            }
            _ => false,
        }
    }

    fn is_blank_line_between(&self, node_a: &Node, node_b: &Node) -> bool {
        let end_line = self.line_of_offset(node_a.location().end_offset());
        let start_line = self.line_of_offset(node_b.location().start_offset());
        start_line > end_line + 1
    }

    fn is_constant_between(&self, siblings: &[Node], from_idx: usize, to_idx: usize) -> bool {
        for i in (from_idx + 1)..to_idx {
            if matches!(siblings[i], Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. }) {
                return true;
            }
        }
        false
    }

    fn is_accessor_call(node: &Node) -> bool {
        if let Some(call) = node.as_call_node() {
            Self::is_accessor_method(call.name().as_slice())
        } else {
            false
        }
    }

    fn accessor_call_method_name(node: &Node) -> Option<Vec<u8>> {
        let call = node.as_call_node()?;
        let name = call.name().as_slice();
        if Self::is_accessor_method(name) {
            Some(name.to_vec())
        } else {
            None
        }
    }

    fn accessor_arg_count(node: &Node) -> usize {
        if let Some(call) = node.as_call_node() {
            if let Some(args) = call.arguments() {
                return args.arguments().iter().count();
            }
        }
        0
    }

    fn check_body_stmts(&mut self, stmts: &[Node]) {
        match self.cop.style {
            EnforcedStyle::Grouped => self.check_grouped(stmts),
            EnforcedStyle::Separated => self.check_separated(stmts),
        }
    }

    fn get_indent_of(source: &str, node_start: usize) -> String {
        let bytes = source.as_bytes();
        let mut line_start = node_start;
        while line_start > 0 && bytes[line_start - 1] != b'\n' {
            line_start -= 1;
        }
        let mut indent = String::new();
        for &b in &bytes[line_start..node_start] {
            if b == b' ' || b == b'\t' { indent.push(b as char); } else { break; }
        }
        indent
    }

    /// Returns the range covering the whole line including trailing newline
    fn whole_line_range(source: &str, node_start: usize, node_end: usize) -> (usize, usize) {
        let bytes = source.as_bytes();
        let mut line_start = node_start;
        while line_start > 0 && bytes[line_start - 1] != b'\n' { line_start -= 1; }
        let mut line_end = node_end;
        while line_end < bytes.len() && bytes[line_end] != b'\n' { line_end += 1; }
        if line_end < bytes.len() && bytes[line_end] == b'\n' { line_end += 1; }
        (line_start, line_end)
    }

    /// Returns range covering the whole line plus any preceding blank lines
    fn whole_line_with_preceding_blank(source: &str, node_start: usize, node_end: usize) -> (usize, usize) {
        let bytes = source.as_bytes();
        // Find start of node's line
        let mut line_start = node_start;
        while line_start > 0 && bytes[line_start - 1] != b'\n' { line_start -= 1; }
        // Expand left to also consume preceding blank lines
        let mut expanded_start = line_start;
        // Walk backwards: if the line before is blank, consume it too
        while expanded_start > 0 {
            // Go to end of previous line
            let prev_line_end = expanded_start - 1; // points to the \n before our line_start
            // Find start of previous line
            let mut prev_line_start = prev_line_end;
            while prev_line_start > 0 && bytes[prev_line_start - 1] != b'\n' { prev_line_start -= 1; }
            let prev_line_content = &source[prev_line_start..prev_line_end];
            if prev_line_content.trim().is_empty() {
                expanded_start = prev_line_start;
            } else {
                break;
            }
        }
        // Line end (past newline)
        let mut line_end = node_end;
        while line_end < bytes.len() && bytes[line_end] != b'\n' { line_end += 1; }
        if line_end < bytes.len() && bytes[line_end] == b'\n' { line_end += 1; }
        (expanded_start, line_end)
    }

    fn node_args_srcs(node: &Node, source: &str) -> Vec<String> {
        if let Some(call) = node.as_call_node() {
            if let Some(args) = call.arguments() {
                return args.arguments().iter().map(|a| {
                    source[a.location().start_offset()..a.location().end_offset()].to_string()
                }).collect();
            }
        }
        vec![]
    }

    /// Check if between stmts[idx] and stmts[j] (j > idx) there's a constant that separates them
    fn has_constant_between(stmts: &[Node], from_idx: usize, to_idx: usize) -> bool {
        for k in (from_idx + 1)..to_idx {
            if matches!(stmts[k], Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. }) {
                return true;
            }
        }
        false
    }

    /// RuboCop's skip_for_grouping?: node has a constant to the right before another groupable sibling
    fn skip_for_grouping(stmts: &[Node], idx: usize, groupable_siblings: &[usize]) -> bool {
        for &j in groupable_siblings {
            if j <= idx { continue; }
            if Self::has_constant_between(stmts, idx, j) {
                return true;
            }
        }
        false
    }

    fn check_grouped(&mut self, stmts: &[Node]) {
        for (i, node) in stmts.iter().enumerate() {
            let name = match Self::accessor_call_method_name(node) {
                Some(x) => x,
                None => continue,
            };

            if self.has_comment_before(node) { continue; }
            if self.has_prev_sorbet_sig(stmts, i) { continue; }
            if self.has_prev_non_accessor_send(stmts, i) { continue; }
            if self.has_rbs_inline_comment_after(node) { continue; }

            let groupable_siblings = self.find_groupable_siblings(stmts, i, &name);
            if groupable_siblings.len() <= 1 { continue; }

            let start = node.location().start_offset();
            let end = node.location().end_offset();
            let name_str = String::from_utf8_lossy(&name).to_string();
            let msg = GROUPED_MSG.replace("%accessor%", &name_str);

            // Determine correction for this offense node
            let correction = self.build_grouped_correction(stmts, i, &groupable_siblings, &name_str);

            let offense = self.ctx.offense_with_range(
                "Style/AccessorGrouping",
                &msg,
                Severity::Convention,
                start,
                end,
            );
            if let Some(corr) = correction {
                self.offenses.push(offense.with_correction(corr));
            } else {
                self.offenses.push(offense);
            }
        }
    }

    fn build_grouped_correction(
        &self,
        stmts: &[Node],
        idx: usize,
        groupable_siblings: &[usize],
        name_str: &str,
    ) -> Option<Correction> {
        let source = self.ctx.source;
        let node = &stmts[idx];

        // Find the effective group leader (first sibling not skip_for_grouping)
        let group_leader = {
            let mut leader = None;
            for &j in groupable_siblings {
                if !Self::skip_for_grouping(stmts, j, groupable_siblings) {
                    leader = Some(j);
                    break;
                }
            }
            leader
        };

        if let Some(leader_idx) = group_leader {
            if idx != leader_idx {
                // Not the leader: remove (with preceding blank lines)
                let node_start = node.location().start_offset();
                let node_end = node.location().end_offset();
                let (line_start, line_end) = Self::whole_line_with_preceding_blank(source, node_start, node_end);
                return Some(Correction::delete(line_start, line_end));
            }

            // This is the group leader: replace with grouped form
            let mut all_args: Vec<String> = Vec::new();
            for &j in groupable_siblings {
                let sib = &stmts[j];
                let args = Self::node_args_srcs(sib, source);
                for arg in args {
                    if !all_args.contains(&arg) {
                        all_args.push(arg);
                    }
                }
            }

            // Get inline comment from node (non-RBS)
            let inline_comment = self.get_inline_comment(node);

            let replacement = if let Some(comment) = &inline_comment {
                format!("{} {} {}", name_str, all_args.join(", "), comment)
            } else {
                format!("{} {}", name_str, all_args.join(", "))
            };

            let node_start = node.location().start_offset();
            let node_end = node.location().end_offset();
            // Extend range to end of line (before newline) to cover any existing inline comment
            let replace_end = if inline_comment.is_some() {
                // Find end of the line
                let bytes = source.as_bytes();
                let mut e = node_end;
                while e < bytes.len() && bytes[e] != b'\n' { e += 1; }
                e
            } else {
                node_end
            };
            return Some(Correction::replace(node_start, replace_end, replacement));
        }

        None
    }

    fn get_inline_comment(&self, node: &Node) -> Option<String> {
        // Get inline comment on same line as node (non-RBS)
        let source = self.ctx.source;
        let end_line = self.line_of_offset(node.location().end_offset());
        let lines: Vec<&str> = source.lines().collect();
        if end_line >= lines.len() { return None; }
        let line = lines[end_line];
        let node_end_col = {
            let bytes = source.as_bytes();
            let mut off = 0usize;
            for _ in 0..end_line {
                while off < bytes.len() && bytes[off] != b'\n' { off += 1; }
                off += 1;
            }
            node.location().end_offset().saturating_sub(off)
        };
        let after_node = &line[node_end_col.min(line.len())..];
        if let Some(hash_pos) = after_node.find('#') {
            let after_hash = &after_node[hash_pos + 1..];
            if !after_hash.starts_with(':') {
                // Non-RBS comment
                let comment_start = hash_pos;
                return Some(after_node[comment_start..].trim_end().to_string());
            }
        }
        None
    }

    fn find_groupable_siblings<'b>(&self, stmts: &'b [Node], idx: usize, name: &[u8]) -> Vec<usize> {
        let node = &stmts[idx];
        let node_visibility = self.get_visibility(stmts, idx);

        let mut result = vec![idx];

        for (j, sib) in stmts.iter().enumerate() {
            if j == idx { continue; }

            let sib_name = match Self::accessor_call_method_name(sib) {
                Some(x) => x,
                None => continue,
            };
            if sib_name.as_slice() != name { continue; }

            // Same visibility
            if self.get_visibility(stmts, j) != node_visibility { continue; }

            // Not groupable if has comment before
            if self.has_comment_before(sib) { continue; }

            // Not groupable if has Sorbet sig before
            if self.has_prev_sorbet_sig(stmts, j) { continue; }

            // Not groupable if has non-accessor send before
            if self.has_prev_non_accessor_send(stmts, j) { continue; }

            // No adjacency restriction — accessors of the same kind/visibility are always groupable
            // (only constants and visibility changes between them matter, handled elsewhere)

            // Check RBS inline annotation
            if self.has_rbs_inline_comment_after(sib) {
                continue;
            }

            result.push(j);
        }

        result.sort();
        result
    }

    fn get_visibility(&self, stmts: &[Node], idx: usize) -> u8 {
        // Walk backwards to find the most recent access modifier
        // 0 = public (default), 1 = protected, 2 = private
        let mut visibility = 0u8;
        for j in 0..idx {
            if let Some(call) = stmts[j].as_call_node() {
                let name = call.name().as_slice();
                match name {
                    b"private" if call.arguments().is_none() => visibility = 2,
                    b"protected" if call.arguments().is_none() => visibility = 1,
                    b"public" if call.arguments().is_none() => visibility = 0,
                    _ => {}
                }
            }
        }
        visibility
    }

    fn check_separated(&mut self, stmts: &[Node]) {
        for node in stmts.iter() {
            let name = match Self::accessor_call_method_name(node) {
                Some(x) => x,
                None => continue,
            };
            let arg_count = Self::accessor_arg_count(node);

            // Skip if has comment before
            if self.has_comment_before(node) {
                continue;
            }

            if arg_count > 1 {
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                let name_str = String::from_utf8_lossy(&name).to_string();
                let msg = SEPARATED_MSG.replace("%accessor%", &name_str);
                let correction = self.build_separated_correction(node, &name_str);
                let offense = self.ctx.offense_with_range(
                    "Style/AccessorGrouping",
                    &msg,
                    Severity::Convention,
                    start,
                    end,
                );
                if let Some(corr) = correction {
                    self.offenses.push(offense.with_correction(corr));
                } else {
                    self.offenses.push(offense);
                }
            }
        }
    }

    fn build_separated_correction(&self, node: &Node, name_str: &str) -> Option<Correction> {
        let source = self.ctx.source;
        let call = node.as_call_node()?;
        let args_node = call.arguments()?;
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() <= 1 { return None; }

        let node_start = node.location().start_offset();
        let indent = Self::get_indent_of(source, node_start);

        // RuboCop uses value equality (`arg == node.first_argument`); duplicate
        // args (e.g. `:one, :two, :one`) match first by value → first-arg treatment.
        let first_arg_src = {
            let f = &args[0];
            &source[f.location().start_offset()..f.location().end_offset()]
        };

        // Build the replacement. RuboCop's separate_accessors:
        // - For each arg (reversed), build "name arg_src" lines
        // - First arg: no extra indent (uses existing)
        // - Subsequent: indent + "name arg_src"
        // But it also includes comments from ast_with_comments[arg]
        // We need to handle trailing inline comments (e.g., `:one, # comment`)

        // Check for trailing comments on arg lines
        let mut lines: Vec<String> = Vec::new();

        // Detect if node uses parentheses form: attr_reader(\n  # comment\n  :one,\n  :two\n)
        let node_src = &source[node_start..node.location().end_offset()];
        let has_parens = {
            if let Some(opening) = call.opening_loc() {
                opening.as_slice() == b"("
            } else {
                false
            }
        };

        if has_parens {
            // Complex case with parens: extract comments from each arg's preceding line
            // For each arg, check the line above it for comments
            for (ai, arg) in args.iter().enumerate() {
                let arg_start = arg.location().start_offset();
                let arg_end = arg.location().end_offset();
                let arg_src = &source[arg_start..arg_end];
                let is_first_by_value = ai == 0 || arg_src == first_arg_src;

                // Check for preceding comment (line before arg)
                let arg_line = self.line_of_offset(arg_start);
                let mut preceding_comments: Vec<String> = Vec::new();
                // Scan from current line upward
                if arg_line > 0 {
                    let all_lines: Vec<&str> = source.lines().collect();
                    // Check line immediately above
                    let prev_line = all_lines[arg_line - 1].trim();
                    if prev_line.starts_with('#') {
                        preceding_comments.push(prev_line.to_string());
                    }
                }

                // Check for trailing comment on same line as arg
                let arg_end_line = self.line_of_offset(arg_end);
                let all_lines: Vec<&str> = source.lines().collect();
                let trailing_comment = if arg_end_line < all_lines.len() {
                    let line = all_lines[arg_end_line];
                    // Find comment after arg_end col
                    let line_start_off = {
                        let bytes = source.as_bytes();
                        let mut off = 0usize;
                        for _ in 0..arg_end_line {
                            while off < bytes.len() && bytes[off] != b'\n' { off += 1; }
                            off += 1;
                        }
                        off
                    };
                    let col = arg_end.saturating_sub(line_start_off);
                    let after = &line[col.min(line.len())..];
                    if let Some(hp) = after.find('#') {
                        let comment_text = after[hp..].trim_end();
                        if !comment_text.starts_with("#:") {
                            Some(comment_text.to_string())
                        } else { None }
                    } else { None }
                } else { None };

                // Build this arg's line(s)
                for comment in &preceding_comments {
                    if is_first_by_value {
                        lines.push(comment.clone());
                    } else {
                        lines.push(format!("{}{}", indent, comment));
                    }
                }
                let accessor_line = if let Some(_tc) = &trailing_comment {
                    // trailing comment goes on next line for the attr_reader line
                    // Actually RuboCop puts the trailing comment as a preceding comment for next
                    if is_first_by_value {
                        format!("{} {}", name_str, arg_src)
                    } else {
                        format!("{}{} {}", indent, name_str, arg_src)
                    }
                } else if is_first_by_value {
                    format!("{} {}", name_str, arg_src)
                } else {
                    format!("{}{} {}", indent, name_str, arg_src)
                };
                lines.push(accessor_line);
                // If there was a trailing comment, add it as preceding comment for next iteration
                // Actually RuboCop groups it before the next arg
                // But let's handle it for the previous arg instead (append comment after current line? No)
                // Looking at test: `:two, # comment two B` → `# comment two B\n  attr_reader :two`
                if let Some(tc) = &trailing_comment {
                    // The trailing comment of arg goes before the attr_reader line for that arg
                    // We already pushed the attr_reader without the comment, now insert comment before
                    // Actually looking at expected output:
                    // `# comment two B\n  attr_reader :two`
                    // So comment comes BEFORE the attr_reader for that arg
                    // We pushed attr_reader then need to insert comment before it
                    let last = lines.pop().unwrap();
                    let comment_line = if is_first_by_value {
                        tc.to_string()
                    } else {
                        format!("{}{}", indent, tc)
                    };
                    lines.push(comment_line);
                    lines.push(last);
                }
            }
        } else {
            // Non-paren form: check for trailing comments after each arg (inline on same line)
            // e.g., `attr_reader :a, # comment a\n    :b, # comment b\n    :c # comment c`
            // Expected:
            //   `# comment a\nattr_reader :a\n  # comment b\n  attr_reader :b\n  # comment c\n  attr_reader :c`
            for (ai, arg) in args.iter().enumerate() {
                let arg_start = arg.location().start_offset();
                let arg_end = arg.location().end_offset();
                let arg_src = &source[arg_start..arg_end];
                let is_first_by_value = ai == 0 || arg_src == first_arg_src;

                // Get trailing comment on arg's line
                let arg_end_line = self.line_of_offset(arg_end);
                let all_lines: Vec<&str> = source.lines().collect();
                let trailing_comment = if arg_end_line < all_lines.len() {
                    let line = all_lines[arg_end_line];
                    let line_start_off = {
                        let bytes = source.as_bytes();
                        let mut off = 0usize;
                        for _ in 0..arg_end_line {
                            while off < bytes.len() && bytes[off] != b'\n' { off += 1; }
                            off += 1;
                        }
                        off
                    };
                    let col = arg_end.saturating_sub(line_start_off);
                    let after = &line[col.min(line.len())..].trim_start();
                    if let Some(hp) = after.find('#') {
                        let comment_text = after[hp..].trim_end();
                        Some(comment_text.to_string())
                    } else { None }
                } else { None };

                // Add comment before attr_reader line if present
                if let Some(ref tc) = trailing_comment {
                    if is_first_by_value {
                        lines.push(tc.clone());
                    } else {
                        lines.push(format!("{}{}", indent, tc));
                    }
                }

                // Add attr_reader line
                if is_first_by_value {
                    lines.push(format!("{} {}", name_str, arg_src));
                } else {
                    lines.push(format!("{}{} {}", indent, name_str, arg_src));
                }
            }
        }

        let replacement = lines.join("\n");
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        // For non-paren form: extend range to include trailing comment on last arg's line
        let actual_end = if !has_parens {
            let last_arg = args.last()?;
            let last_arg_end_line = self.line_of_offset(last_arg.location().end_offset());
            let all_lines: Vec<&str> = source.lines().collect();
            if last_arg_end_line < all_lines.len() {
                let line = all_lines[last_arg_end_line];
                let line_start_off = {
                    let bytes = source.as_bytes();
                    let mut off = 0usize;
                    for _ in 0..last_arg_end_line {
                        while off < bytes.len() && bytes[off] != b'\n' { off += 1; }
                        off += 1;
                    }
                    off
                };
                let col = last_arg.location().end_offset().saturating_sub(line_start_off);
                let after = &line[col.min(line.len())..];
                if after.trim_start().starts_with('#') {
                    line_start_off + line.len()
                } else {
                    node_end
                }
            } else {
                node_end
            }
        } else {
            // For paren form: node_end already includes the closing ')'
            node_end
        };

        Some(Correction::replace(node_start, actual_end, replacement))
    }
}

impl<'a> Visit<'_> for AccessorGroupingVisitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(body) = node.body() {
            self.check_class_or_module_body(&body);
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if let Some(body) = node.body() {
            self.check_class_or_module_body(&body);
        }
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        if let Some(body) = node.body() {
            self.check_class_or_module_body(&body);
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

impl<'a> AccessorGroupingVisitor<'a> {
    fn check_class_or_module_body(&mut self, body: &Node) {
        let stmts = if let Some(s) = body.as_statements_node() {
            s.body().iter().collect::<Vec<_>>()
        } else {
            return;
        };
        self.check_body_stmts(&stmts);
    }
}

crate::register_cop!("Style/AccessorGrouping", |cfg| {
    let style_str = cfg.get_cop_config("Style/AccessorGrouping")
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str())
        .unwrap_or("grouped");
    let style = match style_str {
        "separated" => EnforcedStyle::Separated,
        _ => EnforcedStyle::Grouped,
    };
    Some(Box::new(AccessorGrouping::new(style)))
});
