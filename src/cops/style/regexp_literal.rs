//! Style/RegexpLiteral - Enforces the use of `//` or `%r` around regexp literals.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/regexp_literal.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_USE_SLASHES: &str = "Use `//` around regular expression.";
const MSG_USE_PERCENT_R: &str = "Use `%r` around regular expression.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Slashes,
    PercentR,
    Mixed,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::Slashes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodCallParensStyle {
    RequireParentheses,
    OmitParentheses,
}

impl Default for MethodCallParensStyle {
    fn default() -> Self {
        MethodCallParensStyle::RequireParentheses
    }
}

pub struct RegexpLiteral {
    style: EnforcedStyle,
    allow_inner_slashes: bool,
    /// Preferred delimiters for %r, e.g. ('{', '}') or ('[', ']').
    percent_r_delimiters: (char, char),
    method_call_style: MethodCallParensStyle,
}

impl Default for RegexpLiteral {
    fn default() -> Self {
        Self {
            style: EnforcedStyle::default(),
            allow_inner_slashes: false,
            percent_r_delimiters: ('{', '}'),
            method_call_style: MethodCallParensStyle::default(),
        }
    }
}

impl RegexpLiteral {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        style: EnforcedStyle,
        allow_inner_slashes: bool,
        percent_r_delimiters: (char, char),
        method_call_style: MethodCallParensStyle,
    ) -> Self {
        Self { style, allow_inner_slashes, percent_r_delimiters, method_call_style }
    }
}

impl Cop for RegexpLiteral {
    fn name(&self) -> &'static str {
        "Style/RegexpLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { cop: self, ctx, parent_stack: Vec::new(), offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParentKind {
    Call,
    /// ArgumentsNode and similar wrappers that RuboCop's AST flattens away.
    Transparent,
    Other,
}

struct Visitor<'a> {
    cop: &'a RegexpLiteral,
    ctx: &'a CheckContext<'a>,
    /// Stack of ancestor "kinds" maintained via `visit_branch_node_enter/leave`.
    parent_stack: Vec<ParentKind>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn kind_of(node: &Node<'_>) -> ParentKind {
        match node {
            Node::CallNode { .. } => ParentKind::Call,
            Node::ArgumentsNode { .. } => ParentKind::Transparent,
            _ => ParentKind::Other,
        }
    }

    /// Peek at the innermost non-transparent ancestor above `self` (skipping the
    /// current node's own stack entry and any transparent wrappers).
    fn semantic_parent(&self) -> ParentKind {
        // Skip the current node (last pushed) and walk up.
        let mut it = self.parent_stack.iter().rev();
        it.next(); // current node
        for p in it {
            if *p != ParentKind::Transparent {
                return *p;
            }
        }
        ParentKind::Other
    }
}

impl<'a> Visitor<'a> {
    /// Literal source between opening and closing (excluding delimiters and flags).
    fn inner_source(&self, opening_end: usize, closing_start: usize) -> &str {
        &self.ctx.source[opening_end..closing_start]
    }

    /// Does the regexp body contain an unescaped-or-escaped `/`?
    /// RuboCop's check is simply: source of str-children joined contains '/'.
    fn body_contains_slash(&self, opening_end: usize, closing_start: usize) -> bool {
        let body = self.inner_source(opening_end, closing_start);
        // Only flag `/` outside `#{...}` interpolation. Simple scan.
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if i + 1 < bytes.len() && bytes[i] == b'#' && bytes[i + 1] == b'{' {
                // skip interpolation
                let mut depth = 1;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'{' {
                        depth += 1;
                    } else if bytes[i] == b'}' {
                        depth -= 1;
                    }
                    i += 1;
                }
                continue;
            }
            if bytes[i] == b'/' {
                return true;
            }
            // Include escape sequences in the scan (e.g., `\/`).
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                // Check the escaped char too
                if bytes[i + 1] == b'/' {
                    return true;
                }
                i += 2;
                continue;
            }
            i += 1;
        }
        false
    }

    fn contains_disallowed_slash(&self, opening_end: usize, closing_start: usize) -> bool {
        !self.cop.allow_inner_slashes && self.body_contains_slash(opening_end, closing_start)
    }

    fn is_slash_literal(&self, opening_start: usize) -> bool {
        self.ctx.source.as_bytes().get(opening_start) == Some(&b'/')
    }

    fn is_multiline(&self, start: usize, end: usize) -> bool {
        self.ctx.source[start..end].contains('\n')
    }

    fn allowed_slash_literal(&self, opening_end: usize, closing_start: usize, multiline: bool) -> bool {
        let has_bad_slash = self.contains_disallowed_slash(opening_end, closing_start);
        match self.cop.style {
            EnforcedStyle::Slashes => !has_bad_slash,
            EnforcedStyle::Mixed => !multiline && !has_bad_slash,
            EnforcedStyle::PercentR => false,
        }
    }

    fn allowed_percent_r_literal(
        &self,
        opening_end: usize,
        closing_start: usize,
        multiline: bool,
        parent_is_call: bool,
        body_text: &str,
    ) -> bool {
        let has_bad_slash = self.contains_disallowed_slash(opening_end, closing_start);
        match self.cop.style {
            EnforcedStyle::Slashes => {
                has_bad_slash || self.allowed_omit_parens(parent_is_call, body_text)
            }
            EnforcedStyle::PercentR => true,
            EnforcedStyle::Mixed => {
                multiline || has_bad_slash || self.allowed_omit_parens(parent_is_call, body_text)
            }
        }
    }

    fn allowed_omit_parens(&self, parent_is_call: bool, body_text: &str) -> bool {
        if !parent_is_call {
            return false;
        }
        if body_text.starts_with(' ') || body_text.starts_with('=') {
            return true;
        }
        self.cop.method_call_style == MethodCallParensStyle::OmitParentheses
    }

    /// Compute offense range. For multi-line regexps `from_offsets` widens the
    /// range to the first newline which yields RuboCop's expected `column_end`.
    fn process_regexp(
        &mut self,
        opening_start: usize,
        opening_end: usize,
        closing_start: usize,
        closing_end: usize,
        parent_is_call: bool,
    ) {
        let is_slash = self.is_slash_literal(opening_start);
        let multiline = self.is_multiline(opening_start, closing_end);
        let body_text = self.inner_source(opening_end, closing_start).to_string();

        let message = if is_slash {
            if self.allowed_slash_literal(opening_end, closing_start, multiline) {
                return;
            }
            MSG_USE_PERCENT_R
        } else {
            if self.allowed_percent_r_literal(
                opening_end,
                closing_start,
                multiline,
                parent_is_call,
                &body_text,
            ) {
                return;
            }
            MSG_USE_SLASHES
        };

        let mut offense = self.ctx.offense_with_range(
            "Style/RegexpLiteral",
            message,
            Severity::Convention,
            opening_start,
            closing_end,
        );

        offense = offense.with_correction(self.build_correction(
            opening_start,
            opening_end,
            closing_start,
            closing_end,
            is_slash,
        ));

        self.offenses.push(offense);
    }

    fn build_correction(
        &self,
        opening_start: usize,
        opening_end: usize,
        closing_start: usize,
        closing_end: usize,
        was_slash: bool,
    ) -> Correction {
        let mut edits: Vec<Edit> = Vec::new();

        if was_slash {
            // slash -> %r
            let (open_c, close_c) = self.cop.percent_r_delimiters;
            let new_open = format!("%r{}", open_c);
            edits.push(Edit { start_offset: opening_start, end_offset: opening_end, replacement: new_open });
            // Closing `/` (single char).
            edits.push(Edit { start_offset: closing_start, end_offset: closing_start + 1, replacement: close_c.to_string() });

            // Convert inner `\/` -> `/` unless the new opening delimiter is `/`.
            if open_c != '/' {
                self.collect_unescape_slashes(opening_end, closing_start, &mut edits);
            }
        } else {
            // %r -> /
            edits.push(Edit { start_offset: opening_start, end_offset: opening_end, replacement: "/".to_string() });
            edits.push(Edit { start_offset: closing_start, end_offset: closing_start + 1, replacement: "/".to_string() });

            // Inside %r the `/` is literal; after convert need to escape as `\/`.
            self.collect_escape_slashes(opening_end, closing_start, &mut edits);
        }

        Correction { edits }
    }

    /// Replace `\/` with `/` inside the body.
    fn collect_unescape_slashes(&self, body_start: usize, body_end: usize, edits: &mut Vec<Edit>) {
        let bytes = self.ctx.source.as_bytes();
        let mut i = body_start;
        while i + 1 < body_end {
            if bytes[i] == b'#' && i + 1 < body_end && bytes[i + 1] == b'{' {
                let mut depth = 1;
                i += 2;
                while i < body_end && depth > 0 {
                    if bytes[i] == b'{' {
                        depth += 1;
                    } else if bytes[i] == b'}' {
                        depth -= 1;
                    }
                    i += 1;
                }
                continue;
            }
            if bytes[i] == b'\\' && bytes[i + 1] == b'/' {
                edits.push(Edit {
                    start_offset: i,
                    end_offset: i + 2,
                    replacement: "/".to_string(),
                });
                i += 2;
                continue;
            }
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            i += 1;
        }
    }

    /// Replace `/` with `\/` inside the body.
    fn collect_escape_slashes(&self, body_start: usize, body_end: usize, edits: &mut Vec<Edit>) {
        let bytes = self.ctx.source.as_bytes();
        let mut i = body_start;
        while i < body_end {
            if i + 1 < body_end && bytes[i] == b'#' && bytes[i + 1] == b'{' {
                let mut depth = 1;
                i += 2;
                while i < body_end && depth > 0 {
                    if bytes[i] == b'{' {
                        depth += 1;
                    } else if bytes[i] == b'}' {
                        depth -= 1;
                    }
                    i += 1;
                }
                continue;
            }
            if bytes[i] == b'\\' && i + 1 < body_end {
                i += 2;
                continue;
            }
            if bytes[i] == b'/' {
                edits.push(Edit {
                    start_offset: i,
                    end_offset: i + 1,
                    replacement: "\\/".to_string(),
                });
            }
            i += 1;
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_branch_node_enter(&mut self, node: Node<'_>) {
        self.parent_stack.push(Self::kind_of(&node));
    }

    fn visit_branch_node_leave(&mut self) {
        self.parent_stack.pop();
    }

    fn visit_leaf_node_enter(&mut self, node: Node<'_>) {
        self.parent_stack.push(Self::kind_of(&node));
    }

    fn visit_leaf_node_leave(&mut self) {
        self.parent_stack.pop();
    }

    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        // The innermost parent in the stack is this regexp's own entry; the one
        // before it is the actual parent.
        let parent_is_call = self.semantic_parent() == ParentKind::Call;
        self.process_regexp(
            opening.start_offset(),
            opening.end_offset(),
            closing.start_offset(),
            closing.end_offset(),
            parent_is_call,
        );
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        let parent_is_call = self.semantic_parent() == ParentKind::Call;
        self.process_regexp(
            opening.start_offset(),
            opening.end_offset(),
            closing.start_offset(),
            closing.end_offset(),
            parent_is_call,
        );

        // Recurse into embedded statements to catch nested regexps.
        for part in node.parts().iter() {
            if let Node::EmbeddedStatementsNode { .. } = part {
                ruby_prism::visit_embedded_statements_node(
                    self,
                    &part.as_embedded_statements_node().unwrap(),
                );
            }
        }
    }
}

crate::register_cop!("Style/RegexpLiteral", |cfg| {
    let cop_config = cfg.get_cop_config("Style/RegexpLiteral");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "percent_r" => EnforcedStyle::PercentR,
            "mixed" => EnforcedStyle::Mixed,
            _ => EnforcedStyle::Slashes,
        })
        .unwrap_or(EnforcedStyle::Slashes);
    let allow_inner_slashes = cop_config
        .and_then(|c| c.raw.get("AllowInnerSlashes"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let percent_r_delims = cfg
        .get_cop_config("Style/PercentLiteralDelimiters")
        .and_then(|c| c.raw.get("PreferredDelimiters"))
        .and_then(|v| v.as_mapping())
        .and_then(|m| {
            m.iter().find_map(|(k, v)| {
                if k.as_str() == Some("%r") {
                    v.as_str().and_then(|s| {
                        let mut it = s.chars();
                        let o = it.next()?;
                        let c = it.next()?;
                        Some((o, c))
                    })
                } else {
                    None
                }
            })
        })
        .unwrap_or(('{', '}'));
    let method_call_style = cfg
        .get_cop_config("Style/MethodCallWithArgsParentheses")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "omit_parentheses" => MethodCallParensStyle::OmitParentheses,
            _ => MethodCallParensStyle::RequireParentheses,
        })
        .unwrap_or(MethodCallParensStyle::RequireParentheses);
    Some(Box::new(RegexpLiteral::with_config(
        style, allow_inner_slashes, percent_r_delims, method_call_style,
    )))
});
