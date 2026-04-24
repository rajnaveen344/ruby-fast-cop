//! Style/RedundantDoubleSplatHashBraces
//!
//! Flags `foo(**{a: 1})` — the `**{}` is redundant; use keyword args directly.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{HashNode, Node, ProgramNode, Visit};

const MSG: &str = "Remove the redundant double splat and braces, use keyword arguments directly.";
const MERGE_METHODS: &[&str] = &["merge", "merge!"];

#[derive(Default)]
pub struct RedundantDoubleSplatHashBraces;

impl RedundantDoubleSplatHashBraces {
    pub fn new() -> Self { Self }
}

impl Cop for RedundantDoubleSplatHashBraces {
    fn name(&self) -> &'static str { "Style/RedundantDoubleSplatHashBraces" }

    fn check_program(&self, node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Finder { ctx, cop_name: self.name(), offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Finder<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    cop_name: &'static str,
    offenses: Vec<Offense>,
}

impl<'pr> Visit<'pr> for Finder<'_, '_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if let Some(args) = node.arguments() {
            for a in args.arguments().iter() {
                if let Some(kw) = a.as_keyword_hash_node() {
                    for elem in kw.elements().iter() {
                        if let Some(splat) = elem.as_assoc_splat_node() {
                            let s = splat.location().start_offset();
                            let e = splat.location().end_offset();
                            self.process_splat(s, e, splat.value().as_ref());
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &HashNode<'pr>) {
        for elem in node.elements().iter() {
            if let Some(splat) = elem.as_assoc_splat_node() {
                let s = splat.location().start_offset();
                let e = splat.location().end_offset();
                self.process_splat(s, e, splat.value().as_ref());
            }
        }
        ruby_prism::visit_hash_node(self, node);
    }
}

impl<'a, 'b> Finder<'a, 'b> {
    fn process_splat(&mut self, splat_start: usize, splat_end: usize, value: Option<&Node>) {
        let Some(value) = value else { return };
        // Detect braced-hash root somewhere in the receiver chain via merge.
        if !self.is_flaggable_splat_target(value) {
            return;
        }
        let Some(replacement) = self.build_replacement(splat_start, splat_end) else { return };
        let correction = Correction::replace(splat_start, splat_end, replacement);
        self.offenses.push(
            self.ctx
                .offense_with_range(self.cop_name, MSG, Severity::Convention, splat_start, splat_end)
                .with_correction(correction),
        );
    }

    /// Root must be a non-empty, colon-style, braced HashNode; every intermediate
    /// call must be `.merge(...)` or `.merge!(...)` (possibly `&.`). A final
    /// call must be mergeable too (handled by walking up from the first call).
    fn is_flaggable_splat_target(&self, expr: &Node) -> bool {
        match expr {
            Node::HashNode { .. } => {
                let h = expr.as_hash_node().unwrap();
                let op = h.opening_loc();
                if op.as_slice() != b"{" { return false; }
                let pairs: Vec<_> = h.elements().iter().collect();
                if pairs.is_empty() { return false; }
                for p in &pairs {
                    if let Some(assoc) = p.as_assoc_node() {
                        if assoc.operator_loc().is_some() { return false; }
                    }
                }
                true
            }
            Node::CallNode { .. } => {
                let c = expr.as_call_node().unwrap();
                let name = node_name!(c);
                if !MERGE_METHODS.contains(&name.as_ref()) { return false; }
                let Some(recv) = c.receiver() else { return false };
                self.is_flaggable_splat_target(&recv)
            }
            _ => false,
        }
    }

    fn build_replacement(&self, start: usize, end: usize) -> Option<String> {
        let s = std::str::from_utf8(&self.ctx.source.as_bytes()[start..end]).ok()?;
        let rest = s.strip_prefix("**")?;
        let open_idx = rest.find('{')?;
        let close_idx = find_matching_close(rest.as_bytes(), open_idx)?;
        let hash_inner_raw = rest[open_idx + 1..close_idx].trim().to_string();
        let hash_inner = flatten_nested_splat_braces(&hash_inner_raw);
        let after_hash = &rest[close_idx + 1..];

        let mut merge_args: Vec<String> = Vec::new();
        let mut cursor = after_hash;
        loop {
            let t = cursor.trim_start();
            let after = if let Some(r) = t.strip_prefix("&.") { r }
                else if let Some(r) = t.strip_prefix('.') { r }
                else { break };
            let (mname, after_name) = match take_ident(after) { Some(x) => x, None => break };
            if !MERGE_METHODS.contains(&mname.as_str()) { break; }
            let after_name = after_name.trim_start().strip_prefix('(')?;
            let close = find_matching_close_paren(after_name.as_bytes())?;
            let args_src = &after_name[..close];
            for arg in split_top_level(args_src) {
                let arg = arg.trim();
                if arg.starts_with('{') && arg.ends_with('}') {
                    let inner = arg[1..arg.len() - 1].trim();
                    merge_args.push(inner.to_string());
                } else if is_bare_keyword_hash(arg) {
                    merge_args.push(arg.to_string());
                } else {
                    merge_args.push(format!("**{}", arg));
                }
            }
            cursor = &after_name[close + 1..];
        }

        let mut parts: Vec<String> = Vec::new();
        if !hash_inner.is_empty() { parts.push(hash_inner); }
        parts.extend(merge_args);
        Some(parts.join(", "))
    }
}

fn find_matching_close(bytes: &[u8], open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open_idx;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => { depth -= 1; if depth == 0 { return Some(i); } }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_matching_close_paren(bytes: &[u8]) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_str {
            if c == b'\\' { i += 2; continue; }
            if c == q { in_str = None; }
        } else {
            match c {
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => { depth -= 1; if depth == 0 && c == b')' { return Some(i); } }
                b'"' | b'\'' => in_str = Some(c),
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn take_ident(s: &str) -> Option<(String, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
    if i < bytes.len() && bytes[i] == b'!' { i += 1; }
    if i == 0 { return None; }
    Some((s[..i].to_string(), &s[i..]))
}

fn split_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut start = 0;
    let mut in_str: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_str {
            if c == b'\\' { i += 2; continue; }
            if c == q { in_str = None; }
        } else {
            match c {
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth -= 1,
                b'"' | b'\'' => in_str = Some(c),
                b',' if depth == 0 => { out.push(&s[start..i]); start = i + 1; }
                _ => {}
            }
        }
        i += 1;
    }
    if start < bytes.len() { out.push(&s[start..]); }
    out
}

fn flatten_nested_splat_braces(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let Some(pos) = find_splat_brace(&out) else { break };
        let bytes = out.as_bytes();
        let open = pos + 2;
        let Some(close) = find_matching_close(bytes, open) else { break };
        let inner = out[open + 1..close].trim().to_string();
        out.replace_range(pos..=close, &inner);
    }
    out
}

fn find_splat_brace(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' && bytes[i + 2] == b'{' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_bare_keyword_hash(s: &str) -> bool {
    if s.starts_with('{') || s.starts_with('[') || s.starts_with('(') { return false; }
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
    i < bytes.len() && bytes[i] == b':'
}

crate::register_cop!("Style/RedundantDoubleSplatHashBraces", |_cfg| Some(Box::new(RedundantDoubleSplatHashBraces::new())));
