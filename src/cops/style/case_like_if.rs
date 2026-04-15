//! Style/CaseLikeIf - Identifies where `if-elsif` constructions can be replaced with `case-when`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/case_like_if.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Style/CaseLikeIf";
const MSG: &str = "Convert `if-elsif` to `case-when`.";

pub struct CaseLikeIf {
    min_branches_count: usize,
}

impl Default for CaseLikeIf {
    fn default() -> Self { Self { min_branches_count: 3 } }
}

impl CaseLikeIf {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(min_branches_count: usize) -> Self { Self { min_branches_count } }
}

impl Cop for CaseLikeIf {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip elsif (handled when visiting the parent if)
        let if_kw = match node.if_keyword_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let kw_text = &ctx.source[if_kw.start_offset()..if_kw.end_offset()];
        if kw_text == "elsif" { return vec![]; }

        // Skip modifier / ternary (no end keyword)
        if node.end_keyword_loc().is_none() { return vec![]; }

        // Gather if/elsif branch conditions
        let predicate = node.predicate();
        let mut branch_preds: Vec<Node> = vec![predicate];
        let mut next = node.subsequent();
        while let Some(n) = next {
            if let Some(ifn) = n.as_if_node() {
                if ifn.end_keyword_loc().is_none() { break; }
                branch_preds.push(ifn.predicate());
                next = ifn.subsequent();
            } else {
                break;
            }
        }

        // Must have elsif_conditional (>= 2 branches) AND >= min_branches_count
        if branch_preds.len() < 2 { return vec![]; }
        if branch_preds.len() < self.min_branches_count { return vec![]; }

        // Determine target from first branch
        let target_src = match find_target(&branch_preds[0], ctx.source) {
            Some(t) => t,
            None => return vec![],
        };

        // Every branch must be convertible
        for bc in &branch_preds {
            if regexp_with_working_captures(bc, ctx.source) {
                return vec![];
            }
            let mut conditions: Vec<String> = Vec::new();
            if !collect_conditions(bc, &target_src, &mut conditions, ctx.source) {
                return vec![];
            }
            if conditions.is_empty() { return vec![]; }
        }

        // Emit offense on whole if node
        let loc = node.location();
        vec![ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention,
            loc.start_offset(), loc.end_offset(),
        )]
    }
}

// ──────────────── helpers ────────────────

/// Deparenthesize: for `(x)` → `x`; for nested parens, fully unwrap. Return the
/// location (start,end) of the innermost expression.
fn deparen_loc<'a>(node: &Node<'a>) -> (usize, usize) {
    if let Some(paren) = node.as_parentheses_node() {
        if let Some(body) = paren.body() {
            if let Some(stmts) = body.as_statements_node() {
                let items: Vec<_> = stmts.body().iter().collect();
                if items.len() == 1 {
                    return deparen_loc(&items[0]);
                }
                let loc = node.location();
                return (loc.start_offset(), loc.end_offset());
            }
            return deparen_loc(&body);
        }
    }
    let loc = node.location();
    (loc.start_offset(), loc.end_offset())
}

fn node_src<'a>(node: &Node<'a>, source: &str) -> String {
    let loc = node.location();
    source[loc.start_offset()..loc.end_offset()].to_string()
}

fn deparen_src<'a>(node: &Node<'a>, source: &str) -> String {
    let (s, e) = deparen_loc(node);
    source[s..e].to_string()
}

fn sources_equal<'a>(node: &Node<'a>, target_src: &str, source: &str) -> bool {
    deparen_src(node, source) == target_src
}

fn is_literal<'a>(node: &Node<'a>) -> bool {
    matches!(
        node,
        Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. } | Node::StringNode { .. } | Node::SymbolNode { .. }
        | Node::RegularExpressionNode { .. } | Node::TrueNode { .. } | Node::FalseNode { .. }
        | Node::NilNode { .. } | Node::RangeNode { .. } | Node::ArrayNode { .. }
        | Node::HashNode { .. } | Node::SourceFileNode { .. } | Node::SourceLineNode { .. }
        | Node::SourceEncodingNode { .. } | Node::InterpolatedStringNode { .. }
        | Node::InterpolatedSymbolNode { .. }
    )
}

fn is_regexp<'a>(node: &Node<'a>) -> bool {
    matches!(node, Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. })
}

fn const_reference<'a>(node: &Node<'a>) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        let name = node_name!(c).to_string();
        return name.len() > 1 && name == name.to_uppercase();
    }
    false
}

fn class_reference<'a>(node: &Node<'a>) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        let name = node_name!(c).to_string();
        return name.chars().any(|ch| ch.is_lowercase());
    }
    false
}

fn deparen_node_is_range<'a>(node: &Node<'a>) -> bool {
    if node.as_range_node().is_some() { return true; }
    if let Some(paren) = node.as_parentheses_node() {
        if let Some(body) = paren.body() {
            if let Some(stmts) = body.as_statements_node() {
                let items: Vec<_> = stmts.body().iter().collect();
                if items.len() == 1 {
                    return deparen_node_is_range(&items[0]);
                }
            } else {
                return deparen_node_is_range(&body);
            }
        }
    }
    false
}

// ──────────────── find_target ────────────────

fn find_target<'a>(node: &Node<'a>, source: &str) -> Option<String> {
    // Deparenthesize
    if let Some(paren) = node.as_parentheses_node() {
        if let Some(body) = paren.body() {
            if let Some(stmts) = body.as_statements_node() {
                let items: Vec<_> = stmts.body().iter().collect();
                if items.len() == 1 {
                    return find_target(&items[0], source);
                }
            } else {
                return find_target(&body, source);
            }
        }
        return None;
    }

    if let Some(orn) = node.as_or_node() {
        return find_target(&orn.left(), source);
    }

    if let Some(call) = node.as_call_node() {
        return find_target_in_send(&call, source);
    }

    None
}

fn find_target_in_send<'a>(call: &ruby_prism::CallNode<'a>, source: &str) -> Option<String> {
    let method = node_name!(call);
    let method = method.as_ref();
    match method {
        "is_a?" => {
            call.receiver().map(|r| deparen_src(&r, source))
        }
        "==" | "eql?" | "equal?" => find_target_in_equality(call, source),
        "===" => call.arguments().and_then(|a| a.arguments().iter().next())
                    .map(|n| deparen_src(&n, source)),
        "include?" | "cover?" => find_target_in_include_or_cover(call, source),
        "match" | "match?" | "=~" => find_target_in_match(call, source),
        _ => None,
    }
}

fn find_target_in_equality<'a>(call: &ruby_prism::CallNode<'a>, source: &str) -> Option<String> {
    let method = node_name!(call);
    if method == "equal?" {
        let count = call.arguments().map_or(0, |a| a.arguments().iter().count());
        if count != 1 { return None; }
    }
    let arg = call.arguments().and_then(|a| a.arguments().iter().next())?;
    let recv = call.receiver()?;
    if is_literal(&arg) || const_reference(&arg) {
        Some(deparen_src(&recv, source))
    } else if is_literal(&recv) || const_reference(&recv) {
        Some(deparen_src(&arg, source))
    } else {
        None
    }
}

fn find_target_in_include_or_cover<'a>(call: &ruby_prism::CallNode<'a>, source: &str) -> Option<String> {
    let recv = call.receiver()?;
    if deparen_node_is_range(&recv) {
        let arg = call.arguments().and_then(|a| a.arguments().iter().next())?;
        Some(deparen_src(&arg, source))
    } else { None }
}

fn find_target_in_match<'a>(call: &ruby_prism::CallNode<'a>, source: &str) -> Option<String> {
    let recv = call.receiver()?;
    let arg = call.arguments().and_then(|a| a.arguments().iter().next());
    if is_regexp(&recv) {
        arg.map(|a| deparen_src(&a, source))
    } else if let Some(a) = arg {
        if is_regexp(&a) { Some(deparen_src(&recv, source)) } else { None }
    } else { None }
}

// ──────────────── collect_conditions ────────────────

fn collect_conditions<'a>(
    node: &Node<'a>, target_src: &str, out: &mut Vec<String>, source: &str,
) -> bool {
    if let Some(paren) = node.as_parentheses_node() {
        if let Some(body) = paren.body() {
            if let Some(stmts) = body.as_statements_node() {
                let items: Vec<_> = stmts.body().iter().collect();
                if items.len() == 1 {
                    return collect_conditions(&items[0], target_src, out, source);
                }
                return false;
            }
            return collect_conditions(&body, target_src, out, source);
        }
        return false;
    }

    if let Some(orn) = node.as_or_node() {
        return collect_conditions(&orn.left(), target_src, out, source)
            && collect_conditions(&orn.right(), target_src, out, source);
    }

    if let Some(call) = node.as_call_node() {
        return collect_send_cond(&call, target_src, out, source);
    }

    false
}

fn collect_send_cond<'a>(
    call: &ruby_prism::CallNode<'a>, target_src: &str,
    out: &mut Vec<String>, source: &str,
) -> bool {
    let method = node_name!(call);
    let method = method.as_ref();
    let cond: Option<String> = match method {
        "is_a?" => {
            let recv = match call.receiver() { Some(r) => r, None => return false };
            if sources_equal(&recv, target_src, source) {
                call.arguments().and_then(|a| a.arguments().iter().next())
                    .map(|n| node_src(&n, source))
            } else { None }
        }
        "==" | "eql?" | "equal?" => {
            if method == "equal?" {
                let count = call.arguments().map_or(0, |a| a.arguments().iter().count());
                if count != 1 { return false; }
            }
            let recv = match call.receiver() { Some(r) => r, None => return false };
            let arg = match call.arguments().and_then(|a| a.arguments().iter().next()) {
                Some(a) => a, None => return false,
            };
            // condition_from_binary, then exclude class_reference
            let chosen = if sources_equal(&recv, target_src, source) {
                Some(&arg)
            } else if sources_equal(&arg, target_src, source) {
                Some(&recv)
            } else { None };
            chosen.and_then(|n| {
                if class_reference(n) { None } else { Some(deparen_src(n, source)) }
            })
        }
        "=~" | "match" | "match?" => {
            let recv = match call.receiver() { Some(r) => r, None => return false };
            let arg = match call.arguments().and_then(|a| a.arguments().iter().next()) {
                Some(a) => a, None => return false,
            };
            if sources_equal(&recv, target_src, source) {
                Some(deparen_src(&arg, source))
            } else if sources_equal(&arg, target_src, source) {
                Some(deparen_src(&recv, source))
            } else { None }
        }
        "===" => {
            let arg = match call.arguments().and_then(|a| a.arguments().iter().next()) {
                Some(a) => a, None => return false,
            };
            if sources_equal(&arg, target_src, source) {
                call.receiver().map(|r| node_src(&r, source))
            } else { None }
        }
        "include?" | "cover?" => {
            let recv = match call.receiver() { Some(r) => r, None => return false };
            if !deparen_node_is_range(&recv) { return false; }
            let arg = match call.arguments().and_then(|a| a.arguments().iter().next()) {
                Some(a) => a, None => return false,
            };
            if sources_equal(&arg, target_src, source) {
                Some(deparen_src(&recv, source))
            } else { None }
        }
        _ => None,
    };

    match cond {
        Some(c) => { out.push(c); true }
        None => false,
    }
}

// ──────────────── regexp with named captures ────────────────

fn regexp_with_working_captures<'a>(node: &Node<'a>, source: &str) -> bool {
    if let Some(call) = node.as_call_node() {
        let method = node_name!(call);
        let method = method.as_ref();
        if method == "=~" {
            if let Some(recv) = call.receiver() {
                if regexp_has_named_captures(&recv, source) { return true; }
            }
        } else if method == "match" {
            if let Some(recv) = call.receiver() {
                if regexp_has_named_captures(&recv, source) { return true; }
            }
            if let Some(arg) = call.arguments().and_then(|a| a.arguments().iter().next()) {
                if regexp_has_named_captures(&arg, source) { return true; }
            }
        }
    }
    false
}

fn regexp_has_named_captures<'a>(node: &Node<'a>, source: &str) -> bool {
    if let Some(re) = node.as_regular_expression_node() {
        let loc = re.content_loc();
        let txt = &source[loc.start_offset()..loc.end_offset()];
        // `(?<name>` or `(?'name'`
        return has_named_capture_pattern(txt);
    }
    false
}

fn has_named_capture_pattern(s: &str) -> bool {
    // Scan for unescaped `(?<` or `(?'`
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        // skip backslash-escaped char
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'(' && bytes[i+1] == b'?' && (bytes[i+2] == b'<' || bytes[i+2] == b'\'') {
            // Distinguish `(?<=` (lookbehind) from `(?<name>`
            if bytes[i+2] == b'<' && i + 3 < bytes.len() && (bytes[i+3] == b'=' || bytes[i+3] == b'!') {
                i += 1; continue;
            }
            return true;
        }
        i += 1;
    }
    false
}
