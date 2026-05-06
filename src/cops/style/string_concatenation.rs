//! Style/StringConcatenation - Prefer string interpolation over `+`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/string_concatenation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/StringConcatenation";
const MSG: &str = "Prefer string interpolation to string concatenation.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Aggressive,
    Conservative,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Aggressive
    }
}

#[derive(Default)]
pub struct StringConcatenation {
    mode: Mode,
}

impl StringConcatenation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mode(mode: Mode) -> Self {
        Self { mode }
    }
}

impl Cop for StringConcatenation {
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
        let mut v = Visitor {
            ctx,
            mode: self.mode,
            offenses: Vec::new(),
            in_plus_chain: 0,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    mode: Mode,
    offenses: Vec<Offense>,
    /// Depth counter: when >0 we are inside a `+` call chain (as receiver or
    /// argument). Visiting a nested `+` call at depth 0 means it's topmost.
    in_plus_chain: usize,
}

fn is_plus_call(node: &ruby_prism::CallNode, src: &str) -> bool {
    let name = String::from_utf8_lossy(node.name().as_slice());
    if name != "+" {
        return false;
    }
    // must have a receiver and at least one argument
    if node.receiver().is_none() {
        return false;
    }
    let args = match node.arguments() {
        Some(a) => a,
        None => return false,
    };
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 {
        return false;
    }
    // message_loc is "+" operator
    if let Some(msg) = node.message_loc() {
        let s = &src[msg.start_offset()..msg.end_offset()];
        return s == "+";
    }
    false
}

fn is_string_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::StringNode { .. } | Node::InterpolatedStringNode { .. }
    )
}

/// Check if this `+` call is a string concatenation (one side is a string literal).
fn is_string_concat<'pr>(node: &ruby_prism::CallNode<'pr>, src: &str) -> bool {
    if !is_plus_call(node, src) {
        return false;
    }
    let recv = match node.receiver() {
        Some(r) => r,
        None => return false,
    };
    let arg = node.arguments().unwrap().arguments().iter().next().unwrap();
    is_string_literal(&recv) || is_string_literal(&arg)
}

/// Returns true if any `+` call within the plus chain rooted at `node`
/// has a string literal operand. Matches RuboCop's pattern matcher check.
fn chain_has_string_concat<'pr>(node: &Node<'pr>, src: &str) -> bool {
    if let Some(call) = node.as_call_node() {
        if is_plus_call(&call, src) {
            if is_string_concat(&call, src) {
                return true;
            }
            if let Some(recv) = call.receiver() {
                if chain_has_string_concat(&recv, src) {
                    return true;
                }
            }
            if let Some(arg) = call.arguments().and_then(|a| a.arguments().iter().next()) {
                if chain_has_string_concat(&arg, src) {
                    return true;
                }
            }
        }
    }
    false
}

impl<'a> Visitor<'a> {
    fn is_multiline_string_concat(&self, node: &ruby_prism::CallNode) -> bool {
        // Ruby `line_end_concatenation?`: receiver.str_type? && first_arg.str_type?
        // && multiline? && source =~ /\+\s*\n/
        let recv = match node.receiver() {
            Some(r) => r,
            None => return false,
        };
        let arg = match node.arguments().and_then(|a| a.arguments().iter().next()) {
            Some(a) => a,
            None => return false,
        };
        // Both sides must be simple str (not dstr)
        let r_str = matches!(recv, Node::StringNode { .. });
        let a_str = matches!(arg, Node::StringNode { .. });
        if !(r_str && a_str) {
            return false;
        }
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let src = &self.ctx.source[start..end];
        if !src.contains('\n') {
            return false;
        }
        // check for `+\s*\n` pattern
        if let Some(msg) = node.message_loc() {
            let after = &self.ctx.source[msg.end_offset()..end];
            let trimmed = after.trim_start_matches(|c: char| c == ' ' || c == '\t');
            trimmed.starts_with('\n')
        } else {
            false
        }
    }

    /// Returns whether leftmost terminal part is a string literal.
    fn leftmost_is_string<'pr>(&self, node: &Node<'pr>) -> bool {
        if let Some(call) = node.as_call_node() {
            if is_plus_call(&call, self.ctx.source) {
                if let Some(recv) = call.receiver() {
                    return self.leftmost_is_string(&recv);
                }
            }
        }
        is_string_literal(node)
    }
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let src = self.ctx.source;
        let is_plus = is_plus_call(node, src);
        // Report only at topmost `+` in the chain (in_plus_chain == 0) if any
        // call within that chain has a string literal operand.
        if is_plus && self.in_plus_chain == 0 && chain_has_string_concat(&node.as_node(), src) {
            if !self.is_multiline_string_concat(node) {
                let first_is_str = self.leftmost_is_string(&node.as_node());
                let skip = self.mode == Mode::Conservative && !first_is_str;
                if !skip {
                    let start = node.location().start_offset();
                    let end = node.location().end_offset();
                    let mut offense = self.ctx.offense_with_range(
                        COP_NAME,
                        MSG,
                        Severity::Convention,
                        start,
                        end,
                    );
                    // Build correction: collect leaf parts, check uncorrectable, build replacement
                    let parts = collect_parts(&node.as_node(), src);
                    let correctable = parts.iter().all(|p| !is_uncorrectable(p, src));
                    if correctable {
                        let replacement = build_replacement(&parts, src);
                        offense = offense.with_correction(Correction::replace(start, end, replacement));
                    }
                    self.offenses.push(offense);
                }
            }
        }

        // Propagate `in_plus_chain` depth only through `+` chain (receiver/arg).
        // Non-plus sub-expressions (ternary bodies, call args of other methods,
        // block bodies) reset the depth so nested independent plus chains are
        // detected as topmost.
        if is_plus {
            // Only keep depth > 0 when descending into another `+` call directly.
            // Otherwise reset (so nested plus inside ternary, block, etc. is topmost).
            if let Some(recv) = node.receiver() {
                self.descend_plus_child(&recv);
            }
            if let Some(args) = node.arguments() {
                for a in args.arguments().iter() {
                    self.descend_plus_child(&a);
                }
            }
            if let Some(block) = node.block() {
                let saved = self.in_plus_chain;
                self.in_plus_chain = 0;
                self.visit(&block);
                self.in_plus_chain = saved;
            }
        } else {
            let saved = self.in_plus_chain;
            self.in_plus_chain = 0;
            ruby_prism::visit_call_node(self, node);
            self.in_plus_chain = saved;
        }
    }
}

impl<'pr> Visitor<'_> {
    fn descend_plus_child(&mut self, child: &Node<'pr>) {
        let is_child_plus = child
            .as_call_node()
            .map_or(false, |c| is_plus_call(&c, self.ctx.source));
        if is_child_plus {
            self.in_plus_chain += 1;
            self.visit(child);
            self.in_plus_chain -= 1;
        } else {
            let saved = self.in_plus_chain;
            self.in_plus_chain = 0;
            self.visit(child);
            self.in_plus_chain = saved;
        }
    }
}

/// Kind of leaf part in a `+` chain.
#[derive(Debug)]
enum PartKind {
    Str {
        is_single_quoted: bool,
        value: String, // unescaped value from Prism
    },
    Dstr,   // interpolated string node
    Other {
        /// Inner string concat corrections: (inner_start, inner_end, replacement_text)
        /// to be applied within this Other expression before wrapping in #{...}
        inner_corrections: Vec<(usize, usize, String)>,
        /// If true, strip outer parens when emitting #{...}
        is_parens: bool,
    },
}

/// Offset-based part info (avoids Node clone).
struct PartInfo {
    start: usize,
    end: usize,
    kind: PartKind,
    // For dstr: child range info
    dstr_children: Vec<DstrChild>,
}

enum DstrChildKind {
    Str(String),        // StringNode — store unescaped value
    Embedded,           // EmbeddedStatements/Variable — emit source as-is
    Nested(Vec<DstrChild>), // nested InterpolatedStringNode — recurse
}

struct DstrChild {
    start: usize,
    end: usize,
    kind: DstrChildKind,
}

/// Collect inner string concat corrections (start, end, replacement) within a node.
/// These are topmost + chains that would generate their own offense + correction.
fn collect_inner_str_concat_corrections(node: &Node, src: &str) -> Vec<(usize, usize, String)> {
    struct Finder<'s> {
        src: &'s str,
        corrections: Vec<(usize, usize, String)>,
        in_plus_chain: usize,
    }
    impl<'s, 'pr> Visit<'pr> for Finder<'s> {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            let is_plus = is_plus_call(node, self.src);
            if is_plus && self.in_plus_chain == 0 && chain_has_string_concat(&node.as_node(), self.src) {
                // Collect parts and build correction
                let parts = collect_parts_from_call(node, self.src);
                let correctable = parts.iter().all(|p| !is_uncorrectable_part(p, self.src));
                if correctable {
                    let replacement = build_replacement(&parts, self.src);
                    let start = node.location().start_offset();
                    let end = node.location().end_offset();
                    self.corrections.push((start, end, replacement));
                }
                // Visit children with depth tracking
                if let Some(recv) = node.receiver() {
                    self.in_plus_chain += 1;
                    self.visit(&recv);
                    self.in_plus_chain -= 1;
                }
                if let Some(args) = node.arguments() {
                    for a in args.arguments().iter() {
                        self.in_plus_chain += 1;
                        self.visit(&a);
                        self.in_plus_chain -= 1;
                    }
                }
            } else {
                ruby_prism::visit_call_node(self, node);
            }
        }
    }
    let mut finder = Finder { src, corrections: vec![], in_plus_chain: 0 };
    finder.visit(node);
    finder.corrections
}

fn collect_parts_from_call(node: &ruby_prism::CallNode, src: &str) -> Vec<PartInfo> {
    let mut parts = Vec::new();
    if let Some(recv) = node.receiver() {
        collect_parts_inner(&recv, src, &mut parts);
    }
    if let Some(args) = node.arguments() {
        if let Some(arg) = args.arguments().iter().next() {
            collect_parts_inner(&arg, src, &mut parts);
        }
    }
    parts
}

fn is_uncorrectable_part(p: &PartInfo, src: &str) -> bool {
    is_uncorrectable(p, src)
}

fn collect_dstr_children_from_parts(nodes: Vec<Node>) -> Vec<DstrChild> {
    let mut result = Vec::new();
    for child in nodes {
        let start = child.location().start_offset();
        let end = child.location().end_offset();
        match &child {
            Node::StringNode { .. } => {
                let sn = child.as_string_node().unwrap();
                let val = String::from_utf8_lossy(sn.unescaped()).to_string();
                result.push(DstrChild { start, end, kind: DstrChildKind::Str(val) });
            }
            Node::InterpolatedStringNode { .. } => {
                let dstr = child.as_interpolated_string_node().unwrap();
                let nested = collect_dstr_children_from_parts(dstr.parts().iter().collect());
                result.push(DstrChild { start, end, kind: DstrChildKind::Nested(nested) });
            }
            _ => {
                result.push(DstrChild { start, end, kind: DstrChildKind::Embedded });
            }
        }
    }
    result
}

/// Collect leaf parts of a `+` chain in order (left-to-right).
fn collect_parts(node: &Node, src: &str) -> Vec<PartInfo> {
    let mut parts = Vec::new();
    collect_parts_inner(node, src, &mut parts);
    parts
}

fn collect_parts_inner(node: &Node, src: &str, parts: &mut Vec<PartInfo>) {
    if let Some(call) = node.as_call_node() {
        if is_plus_call(&call, src) {
            if let Some(recv) = call.receiver() {
                collect_parts_inner(&recv, src, parts);
            }
            if let Some(args) = call.arguments() {
                if let Some(arg) = args.arguments().iter().next() {
                    collect_parts_inner(&arg, src, parts);
                }
            }
            return;
        }
    }
    let start = node.location().start_offset();
    let end = node.location().end_offset();
    let node_src = &src[start..end];
    match node {
        Node::StringNode { .. } => {
            let str_node = node.as_string_node().unwrap();
            let value = String::from_utf8_lossy(str_node.unescaped()).to_string();
            let is_single_quoted = node_src.starts_with('\'')
                || node_src.starts_with("%q(")
                || (node_src.starts_with("%(") && !node_src.starts_with("%(\""));
            parts.push(PartInfo {
                start, end,
                kind: PartKind::Str { is_single_quoted, value },
                dstr_children: vec![],
            });
        }
        Node::InterpolatedStringNode { .. } => {
            let dstr = node.as_interpolated_string_node().unwrap();
            let children = collect_dstr_children_from_parts(dstr.parts().iter().collect());
            parts.push(PartInfo { start, end, kind: PartKind::Dstr, dstr_children: children });
        }
        _ => {
            // Collect inner string concat corrections within this Other node
            let inner_corrections = collect_inner_str_concat_corrections(node, src);
            // For ParenthesesNode, we want to strip outer parens when emitting #{...}
            let is_parens = matches!(node, Node::ParenthesesNode { .. });
            parts.push(PartInfo { start, end, kind: PartKind::Other { inner_corrections, is_parens }, dstr_children: vec![] });
        }
    }
}

/// Part is uncorrectable if: multiline or heredoc.
fn is_uncorrectable(p: &PartInfo, src: &str) -> bool {
    let node_src = &src[p.start..p.end];
    if node_src.contains('\n') { return true; }
    if node_src.starts_with("<<") { return true; }
    false
}

/// Build the replacement interpolated string from parts.
fn build_replacement(parts: &[PartInfo], src: &str) -> String {
    let adjusted: Vec<String> = parts.iter().map(|p| adjust_part(p, src)).collect();
    let joined = handle_quotes(adjusted).join("");
    format!("\"{}\"", joined)
}

/// Apply inner corrections (absolute offsets) to the node_src string.
/// Corrections are (abs_start, abs_end, replacement); node_base = absolute offset of node_src[0].
fn apply_inner_corrections(node_src: &str, node_base: usize, corrections: &[(usize, usize, String)]) -> String {
    if corrections.is_empty() {
        return node_src.to_string();
    }
    // Sort by start offset ascending
    let mut sorted: Vec<&(usize, usize, String)> = corrections.iter().collect();
    sorted.sort_by_key(|(s, _, _)| *s);

    let mut result = String::new();
    let mut cursor = 0usize; // cursor into node_src (relative)
    for (abs_start, abs_end, replacement) in sorted {
        let rel_start = abs_start.saturating_sub(node_base);
        let rel_end = abs_end.saturating_sub(node_base);
        if rel_start < cursor || rel_start > node_src.len() {
            continue; // skip overlapping
        }
        result.push_str(&node_src[cursor..rel_start]);
        result.push_str(replacement);
        cursor = rel_end.min(node_src.len());
    }
    result.push_str(&node_src[cursor..]);
    result
}

fn emit_dstr_children(children: &[DstrChild], src: &str, out: &mut String) {
    for child in children {
        match &child.kind {
            DstrChildKind::Str(val) => {
                out.push_str(&double_quote_escape(val));
            }
            DstrChildKind::Embedded => {
                let child_src = &src[child.start..child.end];
                out.push_str(child_src);
            }
            DstrChildKind::Nested(nested) => {
                emit_dstr_children(nested, src, out);
            }
        }
    }
}

fn adjust_part(p: &PartInfo, src: &str) -> String {
    match &p.kind {
        PartKind::Str { is_single_quoted, value } => {
            if *is_single_quoted {
                // single-quoted: escape \, ", #{, #@, #$
                value.replace('\\', "\\\\")
                     .replace('"', "\\\"")
                     .replace("#{", "\\#{")
                     .replace("#@", "\\#@")
                     .replace("#$", "\\#$")
            } else {
                // double-quoted: use value with double-quote escaping
                // Ruby's inspect[1..-2]: re-escape for double-quote context
                double_quote_escape(value)
            }
        }
        PartKind::Dstr => {
            let mut result = String::new();
            emit_dstr_children(&p.dstr_children, src, &mut result);
            result
        }
        PartKind::Other { inner_corrections, is_parens } => {
            let node_src = &src[p.start..p.end];
            // Strip outer parens if this is a ParenthesesNode
            let expr = if *is_parens && node_src.starts_with('(') && node_src.ends_with(')') {
                &node_src[1..node_src.len()-1]
            } else {
                node_src
            };
            let base = if *is_parens { p.start + 1 } else { p.start };
            if inner_corrections.is_empty() {
                format!("#{{{}}}", expr)
            } else {
                // Apply inner corrections to the expression (relative to base)
                let corrected = apply_inner_corrections(expr, base, inner_corrections);
                format!("#{{{}}}", corrected)
            }
        }
    }
}

/// Escape a string value for use inside a double-quoted Ruby string.
/// Mirrors Ruby's String#inspect[1..-2]: re-escapes special chars.
fn double_quote_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// If any adjusted part is a literal `"`, escape it to `\"`.
fn handle_quotes(parts: Vec<String>) -> Vec<String> {
    parts.into_iter().map(|p| if p == "\"" { "\\\"".to_string() } else { p }).collect()
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { mode: String }

crate::register_cop!("Style/StringConcatenation", |cfg| {
    let c: Cfg = cfg.typed("Style/StringConcatenation");
    let mode = match c.mode.as_str() {
        "conservative" => Mode::Conservative,
        _ => Mode::Aggressive,
    };
    Some(Box::new(StringConcatenation::with_mode(mode)))
});
