use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct OneLineConditional {
    always_multiline: bool,
}

impl OneLineConditional {
    pub fn new() -> Self {
        Self {
            always_multiline: false,
        }
    }

    pub fn with_config(always_multiline: bool) -> Self {
        Self { always_multiline }
    }

    fn is_single_line(source: &str, start: usize, end: usize) -> bool {
        !source[start..end].contains('\n')
    }

    /// Whether the if node starts with "elsif"
    fn is_elsif(node: &ruby_prism::IfNode, source: &str) -> bool {
        node.if_keyword_loc()
            .map_or(false, |loc| source[loc.start_offset()..].starts_with("elsif"))
    }

    /// Whether the if node is a ternary (doesn't start with "if")
    fn is_ternary(node: &ruby_prism::IfNode, source: &str) -> bool {
        !source[node.location().start_offset()..].starts_with("if")
    }

    /// Check if statements have multiple expressions (begin_type equivalent)
    fn has_multiple_stmts(stmts: &Option<ruby_prism::StatementsNode>) -> bool {
        if let Some(s) = stmts {
            let body: Vec<_> = s.body().iter().collect();
            body.len() >= 2
        } else {
            false
        }
    }

    /// Determine if the subsequent node means we must use multiline
    /// Returns (has_else, has_elsif_or_multi_else)
    fn analyze_subsequent(subsequent: &Option<ruby_prism::Node>) -> (bool, bool) {
        match subsequent {
            None => (false, false),
            Some(node) => {
                match node {
                    Node::ElseNode { .. } => {
                        let else_node = node.as_else_node().unwrap();
                        // Empty else = no offense
                        if else_node.statements().is_none() {
                            return (false, false); // treat as no else
                        }
                        // Check if else body has multiple expressions
                        let multi = if let Some(stmts) = else_node.statements() {
                            let body: Vec<_> = stmts.body().iter().collect();
                            body.len() >= 2
                        } else {
                            false
                        };
                        (true, multi)
                    }
                    Node::IfNode { .. } => {
                        // elsif - always multiline
                        (true, true)
                    }
                    _ => (false, false),
                }
            }
        }
    }

    fn message(keyword: &str, multiline: bool) -> String {
        if multiline {
            format!(
                "Favor multi-line `{}` over single-line `{}/then/else/end` constructs.",
                keyword, keyword
            )
        } else {
            format!(
                "Favor the ternary operator (`?:`) over single-line `{}/then/else/end` constructs.",
                keyword
            )
        }
    }
}

impl Cop for OneLineConditional {
    fn name(&self) -> &'static str {
        "Style/OneLineConditional"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip elsif nodes, ternaries, and modifier-if (no end keyword)
        if Self::is_elsif(node, ctx.source) {
            return vec![];
        }
        if Self::is_ternary(node, ctx.source) {
            return vec![];
        }
        if node.end_keyword_loc().is_none() {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if !Self::is_single_line(ctx.source, start, end) {
            return vec![];
        }

        let subsequent = node.subsequent();

        let (has_else, cannot_ternary) = Self::analyze_subsequent(&subsequent);
        if !has_else {
            return vec![];
        }

        // Check if the if-branch has multiple statements
        if Self::has_multiple_stmts(&node.statements()) {
            return vec![];
        }

        let multiline = self.always_multiline || cannot_ternary;
        let msg = Self::message("if", multiline);

        let mut offense = ctx.offense_with_range(self.name(), &msg, self.severity(), start, end);
        if let Some(correction) = build_correction(
            multiline,
            ctx.source,
            start, end,
            Some(node.predicate()),
            node.statements(),
            else_statements_from_subsequent(&subsequent),
            /*swap_branches=*/false,
            /*keyword=*/"if",
        ) {
            offense = offense.with_correction(correction);
        }
        vec![offense]
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip modifier-unless (no end keyword)
        if node.end_keyword_loc().is_none() {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if !Self::is_single_line(ctx.source, start, end) {
            return vec![];
        }

        // UnlessNode uses else_clause() which returns Option<ElseNode>
        let else_clause = node.else_clause();
        match else_clause {
            None => return vec![],
            Some(else_node) => {
                // Empty else = no offense
                if else_node.statements().is_none() {
                    return vec![];
                }
                // Check if else body has multiple expressions
                let else_multi = if let Some(stmts) = else_node.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    body.len() >= 2
                } else {
                    false
                };

                // Check if the if-branch has multiple statements
                if Self::has_multiple_stmts(&node.statements()) {
                    return vec![];
                }

                let multiline = self.always_multiline || else_multi;
                let msg = Self::message("unless", multiline);

                let mut offense = ctx.offense_with_range(self.name(), &msg, self.severity(), start, end);
                if let Some(correction) = build_correction(
                    multiline,
                    ctx.source,
                    start, end,
                    Some(node.predicate()),
                    node.statements(),
                    else_node.statements(),
                    /*swap_branches=*/true,
                    /*keyword=*/"unless",
                ) {
                    offense = offense.with_correction(correction);
                }
                vec![offense]
            }
        }
    }
}

fn else_statements_from_subsequent<'a>(subsequent: &Option<ruby_prism::Node<'a>>) -> Option<ruby_prism::StatementsNode<'a>> {
    match subsequent {
        Some(n) => n.as_else_node().and_then(|e| e.statements()),
        None => None,
    }
}

fn statements_first<'a>(stmts: &Option<ruby_prism::StatementsNode<'a>>) -> Option<ruby_prism::Node<'a>> {
    stmts.as_ref().and_then(|s| s.body().iter().next())
}

fn requires_parens(node: &Node) -> bool {
    match node {
        Node::AndNode { .. } | Node::OrNode { .. } | Node::IfNode { .. } | Node::UnlessNode { .. } => true,
        Node::LocalVariableWriteNode { .. } | Node::InstanceVariableWriteNode { .. }
        | Node::ClassVariableWriteNode { .. } | Node::GlobalVariableWriteNode { .. }
        | Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. }
        | Node::CallOperatorWriteNode { .. } | Node::CallAndWriteNode { .. } | Node::CallOrWriteNode { .. }
        | Node::LocalVariableOperatorWriteNode { .. } | Node::LocalVariableAndWriteNode { .. } | Node::LocalVariableOrWriteNode { .. }
        | Node::InstanceVariableOperatorWriteNode { .. } | Node::InstanceVariableAndWriteNode { .. } | Node::InstanceVariableOrWriteNode { .. }
        | Node::ClassVariableOperatorWriteNode { .. } | Node::ClassVariableAndWriteNode { .. } | Node::ClassVariableOrWriteNode { .. }
        | Node::GlobalVariableOperatorWriteNode { .. } | Node::GlobalVariableAndWriteNode { .. } | Node::GlobalVariableOrWriteNode { .. }
        | Node::ConstantOperatorWriteNode { .. } | Node::ConstantAndWriteNode { .. } | Node::ConstantOrWriteNode { .. }
        | Node::ConstantPathOperatorWriteNode { .. } | Node::ConstantPathAndWriteNode { .. } | Node::ConstantPathOrWriteNode { .. }
        | Node::IndexOperatorWriteNode { .. } | Node::IndexAndWriteNode { .. } | Node::IndexOrWriteNode { .. }
        | Node::MultiWriteNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            // method call with non-parenthesized args (and not an operator method)
            if call.arguments().is_some() {
                if call.opening_loc().is_some() { return false; } // parenthesized
                // operator method names: these don't require paren-wrap
                let name = call.name();
                let name_bytes = name.as_slice();
                let is_op = matches!(name_bytes, b"+" | b"-" | b"*" | b"/" | b"%" | b"**" | b"==" | b"!="
                    | b"<" | b">" | b"<=" | b">=" | b"<=>" | b"===" | b"=~" | b"!~"
                    | b"<<" | b">>" | b"&" | b"|" | b"^" | b"!" | b"~");
                return !is_op;
            }
            false
        }
        _ => false,
    }
}

fn expr_replacement(stmts: &Option<ruby_prism::StatementsNode>, source: &str) -> String {
    match statements_first(stmts) {
        None => "nil".to_string(),
        Some(n) => {
            let loc = n.location();
            let src = &source[loc.start_offset()..loc.end_offset()];
            if requires_parens(&n) {
                format!("({})", src)
            } else {
                src.to_string()
            }
        }
    }
}

fn predicate_replacement(pred: &Option<ruby_prism::Node>, source: &str) -> String {
    match pred {
        None => "nil".to_string(),
        Some(n) => {
            let loc = n.location();
            let src = &source[loc.start_offset()..loc.end_offset()];
            if requires_parens(n) {
                format!("({})", src)
            } else {
                src.to_string()
            }
        }
    }
}

/// Heuristic: check if the if/unless node is the operand of a binary operator,
/// requiring paren-wrap of the ternary/multiline replacement.
/// Walk back from node_start through whitespace; if the last char belongs to an operator
/// (or operator keyword `and`/`or`), wrap.
fn parent_is_operator(source: &str, node_start: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = node_start;
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') { i -= 1; }
    if i == 0 { return false; }
    let c = bytes[i - 1];
    if matches!(c, b'|' | b'&' | b'^' | b'~' | b'+' | b'-' | b'*' | b'/' | b'%'
        | b'<' | b'>' | b'=' | b'!') {
        return true;
    }
    // operator keyword: `and`/`or` (rare in practice)
    if i >= 3 && &bytes[i - 3..i] == b"and" {
        // ensure not part of an identifier
        if i == 3 || !bytes[i - 4].is_ascii_alphanumeric() && bytes[i - 4] != b'_' {
            return true;
        }
    }
    if i >= 2 && &bytes[i - 2..i] == b"or" {
        if i == 2 || !bytes[i - 3].is_ascii_alphanumeric() && bytes[i - 3] != b'_' {
            return true;
        }
    }
    false
}

fn build_correction(
    multiline: bool,
    source: &str,
    node_start: usize,
    node_end: usize,
    predicate: Option<ruby_prism::Node>,
    if_stmts: Option<ruby_prism::StatementsNode>,
    else_stmts: Option<ruby_prism::StatementsNode>,
    swap_branches: bool,
    keyword: &str,
) -> Option<Correction> {
    if multiline {
        // Build multi-line keyword form. Preserve original keyword (if/unless) — no branch swap.
        let cond = predicate_replacement(&predicate, source);
        let then_src = expr_replacement(&if_stmts, source);
        let else_src = expr_replacement(&else_stmts, source);
        let line_start = source[..node_start].rfind('\n').map_or(0, |p| p + 1);
        let indent: String = source[line_start..node_start].chars().take_while(|c| c.is_whitespace()).collect();
        let body_indent = format!("{}  ", indent);
        let mut result = format!(
            "{} {}\n{}{}\n{}else\n{}{}\n{}end",
            keyword, cond, body_indent, then_src, indent, body_indent, else_src, indent,
        );
        if parent_is_operator(source, node_start) {
            result = format!("({})", result);
        }
        Some(Correction::replace(node_start, node_end, &result))
    } else {
        let cond = predicate_replacement(&predicate, source);
        let (a, b) = if swap_branches {
            (expr_replacement(&else_stmts, source), expr_replacement(&if_stmts, source))
        } else {
            (expr_replacement(&if_stmts, source), expr_replacement(&else_stmts, source))
        };
        let mut result = format!("{} ? {} : {}", cond, a, b);
        if parent_is_operator(source, node_start) {
            result = format!("({})", result);
        }
        Some(Correction::replace(node_start, node_end, &result))
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { always_correct_to_multiline: bool }

crate::register_cop!("Style/OneLineConditional", |cfg| {
    let c: Cfg = cfg.typed("Style/OneLineConditional");
    Some(Box::new(OneLineConditional::with_config(c.always_correct_to_multiline)))
});
