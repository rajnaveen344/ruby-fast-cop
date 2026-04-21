//! Style/ParenthesesAroundCondition - no parentheses around if/unless/while/until conditions.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/parentheses_around_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

pub struct ParenthesesAroundCondition {
    allow_in_multiline_conditions: bool,
    allow_safe_assignment: bool,
}

impl Default for ParenthesesAroundCondition {
    fn default() -> Self {
        Self {
            allow_in_multiline_conditions: false,
            allow_safe_assignment: true,
        }
    }
}

impl ParenthesesAroundCondition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        allow_in_multiline_conditions: bool,
        allow_safe_assignment: bool,
    ) -> Self {
        Self {
            allow_in_multiline_conditions,
            allow_safe_assignment,
        }
    }
}

/// Info captured from a parenthesized condition — stored as location fields so
/// we don't need to hold Prism nodes (which aren't Clone/Copy).
struct ParensInfo {
    paren_start: usize,
    paren_end: usize,
    stmt_count: usize,
    // first_start unused; kept for symmetry if needed later
    first_end: usize,
    second_start: usize,
    is_multiline: bool,
}

fn as_parens_info(cond: &Node, source: &str) -> Option<ParensInfo> {
    let pn = cond.as_parentheses_node()?;
    let body = pn.body()?;
    let stmts = body.as_statements_node()?;
    let mut count = 0usize;
    let mut first_end = 0usize;
    let mut second_start = 0usize;
    for (idx, s) in stmts.body().iter().enumerate() {
        let l = s.location();
        if idx == 0 {
            first_end = l.end_offset();
        } else if idx == 1 {
            second_start = l.start_offset();
        }
        count += 1;
    }
    let loc = pn.location();
    Some(ParensInfo {
        paren_start: loc.start_offset(),
        paren_end: loc.end_offset(),
        stmt_count: count,
        first_end,
        second_start,
        is_multiline: source[loc.start_offset()..loc.end_offset()].contains('\n'),
    })
}

fn is_modifier_or_rescue(node: &Node) -> bool {
    // basic_conditional (if, unless, while, until) in modifier form OR rescue modifier
    match node {
        Node::RescueModifierNode { .. } => true,
        Node::IfNode { .. } => {
            let ifn = node.as_if_node().unwrap();
            // Ternaries are if-nodes with a `?` operator — but a ternary is not
            // a "modifier if", so return false for ternaries.
            // Modifier form: `x if y` — the `if` keyword isn't at start of line.
            // Prism: a modifier if has `end_keyword_loc` as None.
            ifn.end_keyword_loc().is_none() && !is_ternary_if(&ifn)
        }
        Node::UnlessNode { .. } => {
            let un = node.as_unless_node().unwrap();
            un.end_keyword_loc().is_none()
        }
        Node::WhileNode { .. } => {
            let wn = node.as_while_node().unwrap();
            wn.closing_loc().is_none()
        }
        Node::UntilNode { .. } => {
            let un = node.as_until_node().unwrap();
            un.closing_loc().is_none()
        }
        _ => false,
    }
}

fn is_ternary_if(n: &ruby_prism::IfNode) -> bool {
    // Ternary: kw_loc is None, or kw_loc source == "?".
    match n.if_keyword_loc() {
        None => true,
        Some(kw_loc) => kw_loc.as_slice() == b"?",
    }
}

/// Is `n` a call that is a block-call with a do/end or brace block?
/// For multiline do/end with keyword, parens required (otherwise parses differently).
fn requires_parens_block(cond_body: &Node) -> bool {
    match cond_body {
        Node::CallNode { .. } => {
            let c = cond_body.as_call_node().unwrap();
            // The block attached to a CallNode isn't always a child of it in Prism —
            // Prism puts blocks on CallNode.block() as an Option<BlockNode|BlockArgumentNode>.
            if let Some(b) = c.block() {
                return matches!(b, Node::BlockNode { .. });
            }
            false
        }
        _ => false,
    }
}

/// Is the call's block a do/end block (uses the `do` keyword)?
fn block_is_do_end(cond_body: &Node) -> bool {
    if let Node::CallNode { .. } = cond_body {
        let c = cond_body.as_call_node().unwrap();
        if let Some(b) = c.block() {
            if let Node::BlockNode { .. } = b {
                let bn = b.as_block_node().unwrap();
                let open = bn.opening_loc();
                return open.as_slice() == b"do";
            }
        }
    }
    false
}

/// Is this a safe assignment inside parens: `(foo = x)`, `(a[0] = x)`, `(self.foo = x)`?
fn is_safe_assignment(first: &Node) -> bool {
    match first {
        Node::LocalVariableWriteNode { .. }
        | Node::InstanceVariableWriteNode { .. }
        | Node::ClassVariableWriteNode { .. }
        | Node::GlobalVariableWriteNode { .. }
        | Node::ConstantWriteNode { .. }
        | Node::IndexAndWriteNode { .. }
        | Node::IndexOrWriteNode { .. }
        | Node::IndexOperatorWriteNode { .. } => true,
        Node::CallNode { .. } => {
            let c = first.as_call_node().unwrap();
            let name = node_name!(c);
            let n: &str = name.as_ref();
            n.ends_with('=') && n != "==" && n != "!=" && n != "<=" && n != ">=" && n != "===" && n != "=~"
        }
        Node::CallOperatorWriteNode { .. }
        | Node::CallAndWriteNode { .. }
        | Node::CallOrWriteNode { .. } => true,
        _ => false,
    }
}

impl ParenthesesAroundCondition {
    fn process<'a>(
        &self,
        keyword: &str,
        article: &str,
        cond: Option<ruby_prism::Node<'a>>,
        _node_loc_before_cond_char: u8,
        ctx: &CheckContext,
    ) -> Option<Offense> {
        let cond = cond?;
        let pn_info = as_parens_info(&cond, ctx.source)?;
        if pn_info.stmt_count == 0 {
            return None;
        }
        // Re-iterate to inspect first statement (Prism Nodes aren't Clone; we don't store them).
        let pn = cond.as_parentheses_node().unwrap();
        let body = pn.body().unwrap();
        let stmts = body.as_statements_node().unwrap();
        let mut iter = stmts.body().iter();
        let first = iter.next().unwrap();

        if (keyword == "while" || keyword == "until")
            && requires_parens_block(&first)
            && block_is_do_end(&first)
        {
            return None;
        }

        if pn_info.stmt_count >= 2 && ctx.source[pn_info.first_end..pn_info.second_start].contains(';') {
            return None;
        }

        if is_modifier_or_rescue(&first) {
            return None;
        }

        let start = pn_info.paren_start;
        let end = pn_info.paren_end;
        let before = if start > 0 {
            ctx.source.as_bytes()[start - 1]
        } else {
            0
        };
        let after = if end < ctx.source.len() {
            ctx.source.as_bytes()[end]
        } else {
            0
        };
        let letter_before = before.is_ascii_lowercase();
        let letter_after = after.is_ascii_lowercase();
        if letter_before || letter_after {
            return None;
        }

        // safe assignment
        if self.allow_safe_assignment && pn_info.stmt_count == 1 && is_safe_assignment(&first) {
            return None;
        }

        // Multiline — allow if config
        let is_multiline = pn_info.is_multiline;
        if self.allow_in_multiline_conditions && is_multiline {
            return None;
        }

        // Build correction.
        let correction = self.build_correction(&pn_info, ctx);

        // Range + message: for multiline when AllowInMultilineConditions=false,
        // the range is only the opening `(`. Otherwise it's the full parens.
        let (range_start, range_end) = if is_multiline && !self.allow_in_multiline_conditions {
            // End range at first newline after the `(`.
            let nl = ctx.source[start..]
                .find('\n')
                .map(|p| start + p)
                .unwrap_or(end);
            (start, nl)
        } else {
            (start, end)
        };
        let message = format!(
            "Don't use parentheses around the condition of {} `{}`.",
            article, keyword
        );
        let mut offense = ctx.offense_with_range(
            "Style/ParenthesesAroundCondition",
            &message,
            Severity::Convention,
            range_start,
            range_end,
        );
        if let Some(c) = correction {
            offense = offense.with_correction(c);
        }
        Some(offense)
    }

    fn build_correction(&self, pn_info: &ParensInfo, ctx: &CheckContext) -> Option<Correction> {
        let start = pn_info.paren_start;
        let end = pn_info.paren_end;

        let open_pos = start;
        let close_pos = end.saturating_sub(1);

        // Remove `(` + right-adjacent whitespace (incl. newline) — mirrors
        // RuboCop's ParenthesesCorrector: `range_with_surrounding_space(side: :right, whitespace: true)`
        let mut after_open = open_pos + 1;
        while after_open < ctx.source.len() {
            let b = ctx.source.as_bytes()[after_open];
            if b == b' ' || b == b'\t' || b == b'\n' {
                after_open += 1;
            } else {
                break;
            }
        }

        // Remove `)` plus left-adjacent whitespace (no newlines, per RuboCop `side: :left`).
        let mut before_close = close_pos;
        while before_close > open_pos + 1 {
            let b = ctx.source.as_bytes()[before_close - 1];
            if b == b' ' || b == b'\t' {
                before_close -= 1;
            } else {
                break;
            }
        }
        // Also swallow a single trailing `\n` after `)` when the `)` was on its own line.
        let mut after_close = close_pos + 1;
        if pn_info.is_multiline && before_close < close_pos.saturating_sub(0) + 1 {
            // nothing — keep as is
        }
        if pn_info.is_multiline
            && after_close < ctx.source.len()
            && ctx.source.as_bytes()[after_close] == b'\n'
        {
            // Trim the newline only if the `)` is the sole token on its line.
            let line_start = ctx.source[..close_pos].rfind('\n').map_or(0, |p| p + 1);
            let before_on_line = &ctx.source[line_start..close_pos];
            if before_on_line.chars().all(|c| c.is_whitespace()) {
                after_close += 1;
            }
        }

        let edits = vec![
            Edit {
                start_offset: open_pos,
                end_offset: after_open,
                replacement: String::new(),
            },
            Edit {
                start_offset: before_close,
                end_offset: after_close,
                replacement: String::new(),
            },
        ];
        Some(Correction { edits })
    }
}

impl Cop for ParenthesesAroundCondition {
    fn name(&self) -> &'static str {
        "Style/ParenthesesAroundCondition"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        if is_ternary_if(node) {
            return vec![];
        }
        let keyword = if let Some(kw_loc) = node.if_keyword_loc() {
            String::from_utf8_lossy(kw_loc.as_slice()).to_string()
        } else {
            "if".to_string()
        };
        let article = if keyword == "while" { "a" } else { "an" };
        self.process(&keyword, article, Some(node.predicate()), 0, ctx)
            .map(|o| vec![o])
            .unwrap_or_default()
    }

    fn check_while(&self, node: &ruby_prism::WhileNode, ctx: &CheckContext) -> Vec<Offense> {
        self.process("while", "a", Some(node.predicate()), 0, ctx)
            .map(|o| vec![o])
            .unwrap_or_default()
    }

    fn check_until(&self, node: &ruby_prism::UntilNode, ctx: &CheckContext) -> Vec<Offense> {
        self.process("until", "an", Some(node.predicate()), 0, ctx)
            .map(|o| vec![o])
            .unwrap_or_default()
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        self.process("unless", "an", Some(node.predicate()), 0, ctx)
            .map(|o| vec![o])
            .unwrap_or_default()
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { allow_in_multiline_conditions: bool, allow_safe_assignment: bool }
impl Default for Cfg {
    fn default() -> Self { Self { allow_in_multiline_conditions: false, allow_safe_assignment: true } }
}

crate::register_cop!("Style/ParenthesesAroundCondition", |cfg| {
    let c: Cfg = cfg.typed("Style/ParenthesesAroundCondition");
    Some(Box::new(ParenthesesAroundCondition::with_config(
        c.allow_in_multiline_conditions,
        c.allow_safe_assignment,
    )))
});
