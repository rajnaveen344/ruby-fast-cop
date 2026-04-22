//! Style/AccessorGrouping cop
//!
//! Checks for grouping of accessors in class and module bodies.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
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

    fn check_grouped(&mut self, stmts: &[Node]) {
        for (i, node) in stmts.iter().enumerate() {
            let name = match Self::accessor_call_method_name(node) {
                Some(x) => x,
                None => continue,
            };

            // Skip if has comment before
            if self.has_comment_before(node) {
                continue;
            }

            // Skip if has Sorbet sig before
            if self.has_prev_sorbet_sig(stmts, i) {
                continue;
            }

            // Skip if has non-accessor, non-modifier send before (annotation method)
            if self.has_prev_non_accessor_send(stmts, i) {
                continue;
            }

            // Skip if this accessor itself has an RBS inline annotation (can't be grouped)
            if self.has_rbs_inline_comment_after(node) {
                continue;
            }

            // Find all groupable siblings with same name, same visibility
            let groupable_siblings = self.find_groupable_siblings(stmts, i, &name);

            if groupable_siblings.len() <= 1 {
                continue;
            }

            // Constants between accessors don't prevent grouping (RuboCop still reports offense)

            // Report offense on this node
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            let name_str = String::from_utf8_lossy(&name);
            let msg = GROUPED_MSG.replace("%accessor%", &name_str);
            self.offenses.push(self.ctx.offense_with_range(
                "Style/AccessorGrouping",
                &msg,
                Severity::Convention,
                start,
                end,
            ));
        }
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
                let name_str = String::from_utf8_lossy(&name);
                let msg = SEPARATED_MSG.replace("%accessor%", &name_str);
                self.offenses.push(self.ctx.offense_with_range(
                    "Style/AccessorGrouping",
                    &msg,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }
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
