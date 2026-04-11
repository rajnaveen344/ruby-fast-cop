use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_element_indentation::{
    EnforcedStyle, IndentBaseType, ParentPairInfo, indent_base,
};
use crate::offense::{Location, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    SpecialInsideParentheses,
    Consistent,
    AlignBraces,
}

impl From<Style> for EnforcedStyle {
    fn from(s: Style) -> Self {
        match s {
            Style::SpecialInsideParentheses => EnforcedStyle::SpecialInsideParentheses,
            Style::Consistent => EnforcedStyle::Consistent,
            Style::AlignBraces => EnforcedStyle::BraceAlignment,
        }
    }
}

pub struct FirstHashElementIndentation {
    style: Style,
    indentation_width: usize,
    /// HashAlignment cop's EnforcedColonStyle / EnforcedHashRocketStyle.
    /// When either is "separator", the first pair's `base offset` is computed
    /// using the longest key instead of the first key.
    hash_alignment_colon_separator: bool,
    hash_alignment_rocket_separator: bool,
}

impl FirstHashElementIndentation {
    pub fn new(
        style: Style,
        width: Option<usize>,
        hash_alignment_colon_separator: bool,
        hash_alignment_rocket_separator: bool,
    ) -> Self {
        Self {
            style,
            indentation_width: width.unwrap_or(2),
            hash_alignment_colon_separator,
            hash_alignment_rocket_separator,
        }
    }
}

impl Cop for FirstHashElementIndentation {
    fn name(&self) -> &'static str {
        "Layout/FirstHashElementIndentation"
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
            style: self.style,
            indentation_width: self.indentation_width,
            colon_separator: self.hash_alignment_colon_separator,
            rocket_separator: self.hash_alignment_rocket_separator,
            offenses: Vec::new(),
            parent_call_parens: Vec::new(),
            parent_pair: None,
            in_interpolation: false,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

/// Info about a call whose arguments are currently being visited.
#[derive(Debug, Clone, Copy)]
struct ParenInfo {
    /// 0-indexed column of the `(`.
    col: usize,
    /// 1-indexed line number of the `(`.
    line: usize,
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: Style,
    indentation_width: usize,
    colon_separator: bool,
    rocket_separator: bool,
    offenses: Vec<Offense>,
    /// Stack of active parenthesized-call contexts (each entry is the call's
    /// `(` column). Top of stack is the innermost active call whose arguments
    /// we are currently walking.
    parent_call_parens: Vec<ParenInfo>,
    /// Info about the outer hash pair if the current hash is being visited
    /// as the value of an AssocNode.
    parent_pair: Option<ParentPairInfo>,
    in_interpolation: bool,
}

impl<'a> Visitor<'a> {
    fn check_hash(&mut self, node: &ruby_prism::HashNode) {
        let open_loc = node.opening_loc();
        let close_loc = node.closing_loc();
        let left_brace_off = open_loc.start_offset();
        let right_brace_off = close_loc.start_offset();
        let left_brace_col = self.ctx.col_of(left_brace_off);
        let left_brace_line_start = self.ctx.line_start(left_brace_off);
        let left_brace_line = self.ctx.line_of(left_brace_off);

        // Only fire left_paren if the call's `(` is on the same line as `{`.
        let left_paren = self
            .parent_call_parens
            .last()
            .copied()
            .and_then(|p| if p.line == left_brace_line { Some(p.col) } else { None });

        let parent_pair = self.parent_pair;

        let elements: Vec<Node> = node.elements().iter().collect();
        let first_pair_node = elements.iter().find(|e| e.as_assoc_node().is_some());

        if let Some(first) = first_pair_node {
            let assoc = first.as_assoc_node().unwrap();
            let first_start = assoc.location().start_offset();
            let first_end = assoc.location().end_offset();
            // If the first pair is on the same line as the left brace, accept.
            if self.ctx.same_line(first_start, left_brace_off) {
                // Still check right brace.
            } else {
                // Determine offset based on separator-style (longest key - first key).
                let offset = if self.is_separator_style(&assoc) {
                    self.longest_key_minus_first(node)
                } else {
                    0
                };
                self.check_first(
                    first_start,
                    first_end,
                    offset,
                    left_brace_col,
                    left_brace_line_start,
                    left_paren,
                    parent_pair,
                );
            }
        }

        // Check right brace: only if the right brace begins its line.
        self.check_right_brace(
            right_brace_off,
            left_brace_col,
            left_brace_line_start,
            left_paren,
            parent_pair,
        );
    }

    fn is_separator_style(&self, assoc: &ruby_prism::AssocNode) -> bool {
        // RuboCop distinguishes by operator type (`:` vs `=>`).
        // Colon style: operator_loc is None (colon is part of the symbol key).
        // Rocket style: operator_loc is Some("=>")
        if assoc.operator_loc().is_some() {
            self.rocket_separator
        } else {
            self.colon_separator
        }
    }

    fn longest_key_minus_first(&self, node: &ruby_prism::HashNode) -> usize {
        let mut longest = 0usize;
        let mut first_len = 0usize;
        let mut seen_first = false;
        for e in node.elements().iter() {
            if let Some(assoc) = e.as_assoc_node() {
                let key = assoc.key();
                let klen = key.location().end_offset() - key.location().start_offset();
                if !seen_first {
                    first_len = klen;
                    seen_first = true;
                }
                if klen > longest {
                    longest = klen;
                }
            }
        }
        longest.saturating_sub(first_len)
    }

    fn check_first(
        &mut self,
        first_start: usize,
        first_end: usize,
        offset: usize,
        left_brace_col: usize,
        left_brace_line_start: usize,
        left_paren: Option<usize>,
        parent_pair: Option<ParentPairInfo>,
    ) {
        let actual_col = self.ctx.col_of(first_start);
        let (base_col, base_type) = indent_base(
            self.ctx,
            self.style.into(),
            left_brace_col,
            left_brace_line_start,
            left_paren,
            parent_pair,
        );
        let expected = base_col + self.indentation_width + offset;
        if actual_col == expected {
            return;
        }

        let msg = format!(
            "Use {} spaces for indentation in a hash, relative to {}.",
            self.indentation_width,
            base_description(base_type),
        );
        let location = Location::from_offsets(self.ctx.source, first_start, first_end);
        self.offenses.push(Offense::new(
            "Layout/FirstHashElementIndentation",
            msg,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    fn check_right_brace(
        &mut self,
        right_brace_off: usize,
        left_brace_col: usize,
        left_brace_line_start: usize,
        left_paren: Option<usize>,
        parent_pair: Option<ParentPairInfo>,
    ) {
        // Skip if the right brace is not the first non-ws char on its line
        // (i.e. it follows the last value on the same line).
        if !self.ctx.begins_its_line(right_brace_off) {
            return;
        }
        let right_col = self.ctx.col_of(right_brace_off);
        let (expected, base_type) = indent_base(
            self.ctx,
            self.style.into(),
            left_brace_col,
            left_brace_line_start,
            left_paren,
            parent_pair,
        );
        if expected == right_col {
            return;
        }
        let msg = right_brace_message(base_type).to_string();
        let location = Location::from_offsets(self.ctx.source, right_brace_off, right_brace_off + 1);
        self.offenses.push(Offense::new(
            "Layout/FirstHashElementIndentation",
            msg,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }
}

fn base_description(t: IndentBaseType) -> &'static str {
    match t {
        IndentBaseType::LeftBraceOrBracket => "the position of the opening brace",
        IndentBaseType::FirstColumnAfterLeftParenthesis => {
            "the first position after the preceding left parenthesis"
        }
        IndentBaseType::ParentHashKey => "the parent hash key",
        IndentBaseType::StartOfLine => "the start of the line where the left curly brace is",
    }
}

fn right_brace_message(t: IndentBaseType) -> &'static str {
    match t {
        IndentBaseType::LeftBraceOrBracket => "Indent the right brace the same as the left brace.",
        IndentBaseType::FirstColumnAfterLeftParenthesis => {
            "Indent the right brace the same as the first position after the preceding left parenthesis."
        }
        IndentBaseType::ParentHashKey => "Indent the right brace the same as the parent hash key.",
        IndentBaseType::StartOfLine => {
            "Indent the right brace the same as the start of the line where the left brace is."
        }
    }
}

impl<'a> Visitor<'a> {
    fn walk_assoc_elements(&mut self, elements: &[Node]) {
        for (idx, elem) in elements.iter().enumerate() {
            if let Some(assoc) = elem.as_assoc_node() {
                // Determine parent-pair info for any hash/array value inside this pair:
                let pair_col = self.ctx.col_of(assoc.location().start_offset());
                let key_line = self.ctx.line_of(assoc.key().location().start_offset());
                let value_line = self.ctx.line_of(assoc.value().location().start_offset());
                let key_and_value_same_line = key_line == value_line;

                let pair_last_line = self
                    .ctx
                    .line_of(assoc.location().end_offset().saturating_sub(1));
                let mut has_right_sibling_on_later_line = false;
                for sib in elements.iter().skip(idx + 1) {
                    let sib_first_line = self.ctx.line_of(sib.location().start_offset());
                    if sib_first_line > pair_last_line {
                        has_right_sibling_on_later_line = true;
                        break;
                    }
                }

                let info = ParentPairInfo {
                    pair_column: pair_col,
                    key_and_value_same_line,
                    has_right_sibling_on_later_line,
                };

                let old = self.parent_pair.take();
                self.visit(&assoc.key());
                self.parent_pair = Some(info);
                self.visit(&assoc.value());
                self.parent_pair = old;
            } else {
                let old = self.parent_pair.take();
                self.visit(elem);
                self.parent_pair = old;
            }
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        self.check_hash(node);
        let elements: Vec<Node> = node.elements().iter().collect();
        self.walk_assoc_elements(&elements);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.in_interpolation {
            ruby_prism::visit_call_node(self, node);
            return;
        }
        // Visit the receiver without the parent-call context.
        let old_stack_len = self.parent_call_parens.len();
        if let Some(recv) = node.receiver() {
            self.visit(&recv);
        }

        // If this call has `(`, push it as parent-call context while walking args+block.
        let has_parens = node.opening_loc().is_some();
        if has_parens {
            let open = node.opening_loc().unwrap();
            let col = self.ctx.col_of(open.start_offset());
            let line = self.ctx.line_of(open.start_offset());
            self.parent_call_parens.push(ParenInfo { col, line });
        }
        // Walk arguments. For hash args, we must NOT pass parent_pair down
        // (a hash as call arg is not a pair value).
        let old_parent_pair = self.parent_pair.take();
        if let Some(args) = node.arguments() {
            for a in args.arguments().iter() {
                self.visit(&a);
            }
        }
        if let Some(block) = node.block() {
            self.visit(&block);
        }
        self.parent_pair = old_parent_pair;
        if has_parens {
            self.parent_call_parens.pop();
        }
        debug_assert_eq!(self.parent_call_parens.len(), old_stack_len);
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        // Implicit keyword hash (no braces) — not checked directly, but we
        // still set parent_pair info when descending so nested hashes whose
        // parent is a keyword-hash pair can use the ParentHashKey base.
        let elements: Vec<Node> = node.elements().iter().collect();
        self.walk_assoc_elements(&elements);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let old = self.in_interpolation;
        self.in_interpolation = true;
        ruby_prism::visit_interpolated_string_node(self, node);
        self.in_interpolation = old;
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        let old = self.in_interpolation;
        self.in_interpolation = true;
        ruby_prism::visit_interpolated_symbol_node(self, node);
        self.in_interpolation = old;
    }
}
