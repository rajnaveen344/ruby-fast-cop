use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

pub struct OneLineConditional {
    always_multiline: bool,
    indentation_width: usize,
}

impl Default for OneLineConditional {
    fn default() -> Self {
        Self { always_multiline: false, indentation_width: 2 }
    }
}

impl OneLineConditional {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(always_multiline: bool, indentation_width: usize) -> Self {
        Self { always_multiline, indentation_width }
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

    /// Check if statements have multiple expressions
    fn has_multiple_stmts(stmts: &Option<ruby_prism::StatementsNode>) -> bool {
        if let Some(s) = stmts {
            s.body().iter().count() >= 2
        } else {
            false
        }
    }

    /// Check if subsequent has elsif (IfNode as subsequent, not ElseNode)
    fn has_elsif(subsequent: &Option<Node>) -> bool {
        matches!(subsequent, Some(Node::IfNode { .. }))
    }

    /// Analyze subsequent to determine multiline-forcing and else presence
    fn analyze_subsequent(subsequent: &Option<Node>) -> (bool, bool) {
        match subsequent {
            None => (false, false),
            Some(node) => match node {
                Node::ElseNode { .. } => {
                    let else_node = node.as_else_node().unwrap();
                    if else_node.statements().is_none() {
                        return (false, false);
                    }
                    let multi = if let Some(stmts) = else_node.statements() {
                        stmts.body().iter().count() >= 2
                    } else {
                        false
                    };
                    (true, multi)
                }
                Node::IfNode { .. } => {
                    // elsif — always multiline
                    (true, true)
                }
                _ => (false, false),
            },
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

        if Self::has_multiple_stmts(&node.statements()) {
            return vec![];
        }

        let multiline = self.always_multiline || cannot_ternary;
        let msg = Self::message("if", multiline);

        let mut offense = ctx.offense_with_range(self.name(), &msg, self.severity(), start, end);
        if let Some(correction) = build_correction(
            multiline,
            ctx.source,
            start,
            end,
            Some(node.predicate()),
            node.statements(),
            &subsequent,
            /*swap_branches=*/ false,
            /*keyword=*/ "if",
            self.indentation_width,
        ) {
            offense = offense.with_correction(correction);
        }
        vec![offense]
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        if node.end_keyword_loc().is_none() {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if !Self::is_single_line(ctx.source, start, end) {
            return vec![];
        }

        let else_clause = node.else_clause();
        match else_clause {
            None => return vec![],
            Some(else_node) => {
                if else_node.statements().is_none() {
                    return vec![];
                }
                let else_multi = if let Some(stmts) = else_node.statements() {
                    stmts.body().iter().count() >= 2
                } else {
                    false
                };

                if Self::has_multiple_stmts(&node.statements()) {
                    return vec![];
                }

                let multiline = self.always_multiline || else_multi;
                let msg = Self::message("unless", multiline);

                // Wrap subsequent as a pseudo-ElseNode reference for build_correction
                let subsequent_none: Option<Node> = None;
                let mut offense =
                    ctx.offense_with_range(self.name(), &msg, self.severity(), start, end);
                if let Some(correction) = build_correction_unless(
                    multiline,
                    ctx.source,
                    start,
                    end,
                    Some(node.predicate()),
                    node.statements(),
                    else_node.statements(),
                    /*swap_branches=*/ true,
                    self.indentation_width,
                ) {
                    offense = offense.with_correction(correction);
                }
                drop(subsequent_none);
                vec![offense]
            }
        }
    }
}

/// Compute the column (0-based) of byte offset in source
fn col_of(source: &str, offset: usize) -> usize {
    let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    offset - line_start
}

/// Get raw source of a statements range (all stmts, first to last)
fn stmts_source<'a>(stmts: &Option<ruby_prism::StatementsNode<'a>>, source: &str) -> String {
    match stmts {
        None => "nil".to_string(),
        Some(s) => {
            let mut iter = s.body().iter();
            let first = match iter.next() {
                None => return "nil".to_string(),
                Some(n) => n,
            };
            let start = first.location().start_offset();
            // Find last node
            let mut last_end = first.location().end_offset();
            for n in iter {
                last_end = n.location().end_offset();
            }
            source[start..last_end].to_string()
        }
    }
}

fn statements_first<'a>(stmts: &Option<ruby_prism::StatementsNode<'a>>) -> Option<Node<'a>> {
    stmts.as_ref().and_then(|s| s.body().iter().next())
}

fn requires_parens(node: &Node, source: &str) -> bool {
    match node {
        Node::AndNode { .. } | Node::OrNode { .. } | Node::IfNode { .. } | Node::UnlessNode { .. } => true,
        Node::LocalVariableWriteNode { .. }
        | Node::InstanceVariableWriteNode { .. }
        | Node::ClassVariableWriteNode { .. }
        | Node::GlobalVariableWriteNode { .. }
        | Node::ConstantWriteNode { .. }
        | Node::ConstantPathWriteNode { .. }
        | Node::CallOperatorWriteNode { .. }
        | Node::CallAndWriteNode { .. }
        | Node::CallOrWriteNode { .. }
        | Node::LocalVariableOperatorWriteNode { .. }
        | Node::LocalVariableAndWriteNode { .. }
        | Node::LocalVariableOrWriteNode { .. }
        | Node::InstanceVariableOperatorWriteNode { .. }
        | Node::InstanceVariableAndWriteNode { .. }
        | Node::InstanceVariableOrWriteNode { .. }
        | Node::ClassVariableOperatorWriteNode { .. }
        | Node::ClassVariableAndWriteNode { .. }
        | Node::ClassVariableOrWriteNode { .. }
        | Node::GlobalVariableOperatorWriteNode { .. }
        | Node::GlobalVariableAndWriteNode { .. }
        | Node::GlobalVariableOrWriteNode { .. }
        | Node::ConstantOperatorWriteNode { .. }
        | Node::ConstantAndWriteNode { .. }
        | Node::ConstantOrWriteNode { .. }
        | Node::ConstantPathOperatorWriteNode { .. }
        | Node::ConstantPathAndWriteNode { .. }
        | Node::ConstantPathOrWriteNode { .. }
        | Node::IndexOperatorWriteNode { .. }
        | Node::IndexAndWriteNode { .. }
        | Node::IndexOrWriteNode { .. }
        | Node::MultiWriteNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let name_bytes = call.name().as_slice();
            // prefix `not` — keyword changing precedence.
            // Prism: `not x` → CallNode with name `!` and message_loc text "not"
            if name_bytes == b"!" {
                if let Some(msg_loc) = call.message_loc() {
                    let msg_text = &source[msg_loc.start_offset()..msg_loc.end_offset()];
                    if msg_text == "not" {
                        return true;
                    }
                }
            }
            if call.arguments().is_some() {
                if call.opening_loc().is_some() {
                    return false; // parenthesized
                }
                // operator methods don't require wrapping
                let is_op = is_operator_method(name_bytes);
                return !is_op;
            }
            false
        }
        // yield with arguments (unparenthesized)
        Node::YieldNode { .. } => {
            let y = node.as_yield_node().unwrap();
            if y.lparen_loc().is_some() {
                return false; // yield(args) — parenthesized
            }
            y.arguments().is_some()
        }
        // super with arguments (not bare super)
        Node::SuperNode { .. } => {
            let s = node.as_super_node().unwrap();
            // super(args) has lparen_loc; super args does not
            if s.lparen_loc().is_some() {
                return false; // parenthesized
            }
            s.arguments().is_some()
        }
        // defined? with argument (unparenthesized)
        Node::DefinedNode { .. } => {
            let d = node.as_defined_node().unwrap();
            // defined? always has an argument; needs parens to avoid precedence issues
            // defined?(x) has lparen_loc; defined? x does not
            d.lparen_loc().is_none()
        }
        _ => false,
    }
}

fn is_operator_method(name_bytes: &[u8]) -> bool {
    matches!(
        name_bytes,
        b"+" | b"-"
            | b"*"
            | b"/"
            | b"%"
            | b"**"
            | b"=="
            | b"!="
            | b"<"
            | b">"
            | b"<="
            | b">="
            | b"<=>"
            | b"==="
            | b"=~"
            | b"!~"
            | b"<<"
            | b">>"
            | b"&"
            | b"|"
            | b"^"
            | b"!"
            | b"~"
            | b"[]"
            | b"[]="
    )
}

/// For ternary branches: wrap in parens if required
fn expr_replacement(stmts: &Option<ruby_prism::StatementsNode>, source: &str) -> String {
    match statements_first(stmts) {
        None => "nil".to_string(),
        Some(n) => {
            let loc = n.location();
            let src = &source[loc.start_offset()..loc.end_offset()];
            if requires_parens(&n, source) {
                format!("({})", src)
            } else {
                src.to_string()
            }
        }
    }
}

/// For ternary predicate: wrap in parens if required
fn predicate_replacement(pred: &Option<Node>, source: &str) -> String {
    match pred {
        None => "nil".to_string(),
        Some(n) => {
            let loc = n.location();
            let src = &source[loc.start_offset()..loc.end_offset()];
            if requires_parens(n, source) {
                format!("({})", src)
            } else {
                src.to_string()
            }
        }
    }
}

/// For multiline predicate/branches: raw source, no paren wrapping
fn node_source(node: &Option<Node>, source: &str) -> String {
    match node {
        None => "nil".to_string(),
        Some(n) => {
            let loc = n.location();
            source[loc.start_offset()..loc.end_offset()].to_string()
        }
    }
}

/// Check if the node at node_start is RHS of an operator expression.
/// Only used for ternary wrapping (not multiline).
/// Returns true if the byte before the node (skipping whitespace) is an operator char
/// that is genuinely a binary operator, not e.g. closing block-param `|`.
fn parent_is_operator(source: &str, node_start: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = node_start;
    // skip whitespace
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    if i == 0 {
        return false;
    }
    let c = bytes[i - 1];
    match c {
        b'|' => {
            // `||` is operator; single `|` followed by identifier (block param) is not
            if i >= 2 && bytes[i - 2] == b'|' {
                return true; // `||`
            }
            // single `|` — check if it's closing a block param: preceded by identifier/underscore
            if i >= 2 && (bytes[i - 2].is_ascii_alphanumeric() || bytes[i - 2] == b'_') {
                return false; // closing `|` of block params like `|line|`
            }
            true // genuinely operator
        }
        b'&' => {
            if i >= 2 && bytes[i - 2] == b'&' {
                return true; // `&&`
            }
            true
        }
        b'^' | b'~' | b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' => true,
        _ => false,
    }
}

/// Build multiline correction for an if/unless chain (may have elsif/else)
fn build_multiline_correction(
    source: &str,
    node_start: usize,
    node_end: usize,
    predicate: Option<Node>,
    if_stmts: Option<ruby_prism::StatementsNode>,
    subsequent: &Option<Node>,
    keyword: &str,
    indentation_width: usize,
) -> Option<Correction> {
    let col = col_of(source, node_start);
    let indent = " ".repeat(col);
    let body_indent = " ".repeat(col + indentation_width);

    // For multiline, use raw source for predicate and branches (no paren wrapping)
    let cond_src = node_source(&predicate, source);
    let then_src = stmts_source(&if_stmts, source);

    let mut result = format!("{} {}\n{}{}", keyword, cond_src, body_indent, then_src);

    // Recursively expand subsequent
    build_subsequent_multiline(source, subsequent, &indent, &body_indent, indentation_width, &mut result);

    Some(Correction::replace(node_start, node_end, &result))
}

/// Append multiline elsif/else chain to result string
fn build_subsequent_multiline(
    source: &str,
    subsequent: &Option<Node>,
    indent: &str,
    body_indent: &str,
    indentation_width: usize,
    result: &mut String,
) {
    match subsequent {
        None => {
            result.push_str(&format!("\n{}end", indent));
        }
        Some(node) => match node {
            Node::ElseNode { .. } => {
                let else_node = node.as_else_node().unwrap();
                let else_src = stmts_source(&else_node.statements(), source);
                result.push_str(&format!(
                    "\n{}else\n{}{}\n{}end",
                    indent, body_indent, else_src, indent
                ));
            }
            Node::IfNode { .. } => {
                // elsif branch
                let elsif = node.as_if_node().unwrap();
                // Raw source for elsif predicate too
                let cond_src = node_source(&Some(elsif.predicate()), source);
                let body_src = stmts_source(&elsif.statements(), source);
                result.push_str(&format!(
                    "\n{}elsif {}\n{}{}",
                    indent, cond_src, body_indent, body_src
                ));
                let nested_subsequent = elsif.subsequent();
                build_subsequent_multiline(source, &nested_subsequent, indent, body_indent, indentation_width, result);
            }
            _ => {
                result.push_str(&format!("\n{}end", indent));
            }
        },
    }
}

fn build_correction(
    multiline: bool,
    source: &str,
    node_start: usize,
    node_end: usize,
    predicate: Option<Node>,
    if_stmts: Option<ruby_prism::StatementsNode>,
    subsequent: &Option<Node>,
    swap_branches: bool,
    keyword: &str,
    indentation_width: usize,
) -> Option<Correction> {
    if multiline {
        build_multiline_correction(
            source,
            node_start,
            node_end,
            predicate,
            if_stmts,
            subsequent,
            keyword,
            indentation_width,
        )
    } else {
        // ternary
        let cond = predicate_replacement(&predicate, source);
        // For else_stmts, extract from subsequent ElseNode
        let else_stmts = match subsequent {
            Some(n) => n.as_else_node().and_then(|e| e.statements()),
            None => None,
        };
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

fn build_correction_unless(
    multiline: bool,
    source: &str,
    node_start: usize,
    node_end: usize,
    predicate: Option<Node>,
    if_stmts: Option<ruby_prism::StatementsNode>,
    else_stmts: Option<ruby_prism::StatementsNode>,
    swap_branches: bool,
    indentation_width: usize,
) -> Option<Correction> {
    if multiline {
        // unless multiline — no elsif possible for unless
        let col = col_of(source, node_start);
        let indent = " ".repeat(col);
        let body_indent = " ".repeat(col + indentation_width);
        let cond_src = node_source(&predicate, source);
        let then_src = stmts_source(&if_stmts, source);
        let else_src = stmts_source(&else_stmts, source);
        let result = format!(
            "unless {}\n{}{}\n{}else\n{}{}\n{}end",
            cond_src, body_indent, then_src, indent, body_indent, else_src, indent
        );
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
struct Cfg {
    always_correct_to_multiline: bool,
}

crate::register_cop!("Style/OneLineConditional", |cfg| {
    let c: Cfg = cfg.typed("Style/OneLineConditional");
    let indent_width = if cfg.is_cop_enabled("Layout/IndentationWidth") {
        cfg.get_cop_config("Layout/IndentationWidth")
            .and_then(|c| c.raw.get("Width"))
            .and_then(|v| v.as_i64())
            .map(|v| v as usize)
            .unwrap_or(2)
    } else {
        2
    };
    Some(Box::new(OneLineConditional::with_config(
        c.always_correct_to_multiline,
        indent_width,
    )))
});
