//! Style/ConcatArrayLiterals cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct ConcatArrayLiterals;

impl ConcatArrayLiterals {
    pub fn new() -> Self { Self }
}

fn is_array(n: &Node) -> bool { n.as_array_node().is_some() }

fn is_percent_literal(arr: &ruby_prism::ArrayNode, source: &str) -> bool {
    if let Some(op) = arr.opening_loc() {
        let s = &source[op.start_offset()..op.end_offset()];
        return s.starts_with('%');
    }
    false
}

fn percent_only_basic(arr: &ruby_prism::ArrayNode) -> bool {
    arr.elements().iter().all(|el| {
        matches!(el, Node::StringNode { .. } | Node::SymbolNode { .. })
    })
}

fn element_source(el: &Node, source: &str) -> String {
    let loc = el.location();
    source[loc.start_offset()..loc.end_offset()].to_string()
}

fn element_as_quoted(el: &Node, source: &str) -> Option<String> {
    if let Some(s) = el.as_string_node() {
        let raw = String::from_utf8_lossy(s.unescaped()).to_string();
        // .inspect on a Ruby string => double-quoted
        Some(format!("{:?}", raw))
    } else if let Some(sym) = el.as_symbol_node() {
        let raw = String::from_utf8_lossy(sym.unescaped()).to_string();
        Some(format!(":{}", raw))
    } else {
        None
    }
}

impl Cop for ConcatArrayLiterals {
    fn name(&self) -> &'static str { "Style/ConcatArrayLiterals" }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "concat" { return vec![]; }
        let args_node = match node.arguments() { Some(a) => a, None => return vec![] };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.is_empty() { return vec![]; }
        if !args.iter().all(is_array) { return vec![]; }

        let msg_loc = match node.message_loc() { Some(l) => l, None => return vec![] };
        let start = msg_loc.start_offset();
        let end = node.location().end_offset();
        let current = &ctx.source[start..end];

        // Determine if any percent literal arg, and whether basic
        let arr_nodes: Vec<ruby_prism::ArrayNode> = args.iter().map(|a| a.as_array_node().unwrap()).collect();
        let any_percent = arr_nodes.iter().any(|a| is_percent_literal(a, ctx.source));
        let percent_basic = arr_nodes.iter().filter(|a| is_percent_literal(a, ctx.source))
            .all(|a| percent_only_basic(a));

        // Build preferred if possible
        let preferred: Option<String> = if any_percent && !percent_basic {
            None
        } else {
            let mut parts: Vec<String> = Vec::new();
            let mut ok = true;
            for a in &arr_nodes {
                let is_pct = is_percent_literal(a, ctx.source);
                for el in a.elements().iter() {
                    if is_pct {
                        match element_as_quoted(&el, ctx.source) {
                            Some(q) => parts.push(q),
                            None => { ok = false; break; }
                        }
                    } else {
                        parts.push(element_source(&el, ctx.source));
                    }
                }
                if !ok { break; }
            }
            if ok { Some(format!("push({})", parts.join(", "))) } else { None }
        };

        let msg = if any_percent && !percent_basic {
            format!("Use `push` with elements as arguments without array brackets instead of `{}`.", current)
        } else {
            let p = preferred.clone().unwrap_or_else(|| "push(...)".to_string());
            format!("Use `{}` instead of `{}`.", p, current)
        };

        let mut off = ctx.offense_with_range(self.name(), &msg, Severity::Convention, start, end);

        // Correction
        if any_percent {
            if let Some(p) = preferred.as_ref() {
                off = off.with_correction(Correction::replace(start, end, p.clone()));
            }
        } else {
            // Replace selector "concat" with "push", strip brackets from each array arg.
            let mut edits: Vec<Edit> = Vec::new();
            edits.push(Edit { start_offset: msg_loc.start_offset(), end_offset: msg_loc.end_offset(), replacement: "push".to_string() });
            for a in &arr_nodes {
                if let Some(open) = a.opening_loc() {
                    edits.push(Edit { start_offset: open.start_offset(), end_offset: open.end_offset(), replacement: String::new() });
                }
                if let Some(close) = a.closing_loc() {
                    edits.push(Edit { start_offset: close.start_offset(), end_offset: close.end_offset(), replacement: String::new() });
                }
            }
            off = off.with_correction(Correction { edits });
        }
        vec![off]
    }
}

crate::register_cop!("Style/ConcatArrayLiterals", |_cfg| Some(Box::new(ConcatArrayLiterals::new())));
