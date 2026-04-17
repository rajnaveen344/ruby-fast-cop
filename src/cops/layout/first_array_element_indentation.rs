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
    AlignBrackets,
}

impl From<Style> for EnforcedStyle {
    fn from(s: Style) -> Self {
        match s {
            Style::SpecialInsideParentheses => EnforcedStyle::SpecialInsideParentheses,
            Style::Consistent => EnforcedStyle::Consistent,
            Style::AlignBrackets => EnforcedStyle::BraceAlignment,
        }
    }
}

pub struct FirstArrayElementIndentation {
    style: Style,
    indentation_width: usize,
}

impl FirstArrayElementIndentation {
    pub fn new(style: Style, width: Option<usize>) -> Self {
        Self {
            style,
            indentation_width: width.unwrap_or(2),
        }
    }
}

impl Cop for FirstArrayElementIndentation {
    fn name(&self) -> &'static str {
        "Layout/FirstArrayElementIndentation"
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
            offenses: Vec::new(),
            parent_call_parens: Vec::new(),
            parent_pair: None,
            in_interpolation: false,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Debug, Clone, Copy)]
struct ParenInfo {
    col: usize,
    line: usize,
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: Style,
    indentation_width: usize,
    offenses: Vec<Offense>,
    parent_call_parens: Vec<ParenInfo>,
    parent_pair: Option<ParentPairInfo>,
    in_interpolation: bool,
}

impl<'a> Visitor<'a> {
    fn check_array(&mut self, node: &ruby_prism::ArrayNode) {
        let open_loc = match node.opening_loc() {
            Some(l) => l,
            None => return, // `%w(...)` literals etc. have openings but some forms may not.
        };
        let close_loc = match node.closing_loc() {
            Some(l) => l,
            None => return,
        };
        // Only check real `[...]` arrays — skip `%w[]`, `%i[]`, etc.
        let open_slice = open_loc.as_slice();
        if open_slice != b"[" {
            return;
        }

        let left_bracket_off = open_loc.start_offset();
        let right_bracket_off = close_loc.start_offset();
        let left_bracket_col = self.ctx.col_of(left_bracket_off);
        let left_bracket_line_start = self.ctx.line_start(left_bracket_off);
        let left_bracket_line = self.ctx.line_of(left_bracket_off);

        let left_paren = self
            .parent_call_parens
            .last()
            .copied()
            .and_then(|p| if p.line == left_bracket_line { Some(p.col) } else { None });

        let parent_pair = self.parent_pair;

        let elements: Vec<Node> = node.elements().iter().collect();
        if let Some(first) = elements.first() {
            let first_start = first.location().start_offset();
            let first_end = first.location().end_offset();
            if !self.ctx.same_line(first_start, left_bracket_off) {
                self.check_first(
                    first_start,
                    first_end,
                    left_bracket_col,
                    left_bracket_line_start,
                    left_paren,
                    parent_pair,
                );
            }
        }

        self.check_right_bracket(
            right_bracket_off,
            left_bracket_col,
            left_bracket_line_start,
            left_paren,
            parent_pair,
        );
    }

    fn check_first(
        &mut self,
        first_start: usize,
        first_end: usize,
        left_bracket_col: usize,
        left_bracket_line_start: usize,
        left_paren: Option<usize>,
        parent_pair: Option<ParentPairInfo>,
    ) {
        let actual_col = self.ctx.col_of(first_start);
        let (base_col, base_type) = indent_base(
            self.ctx,
            self.style.into(),
            left_bracket_col,
            left_bracket_line_start,
            left_paren,
            parent_pair,
        );
        let expected = base_col + self.indentation_width;
        if actual_col == expected {
            return;
        }

        let msg = format!(
            "Use {} spaces for indentation in an array, relative to {}.",
            self.indentation_width,
            base_description(base_type),
        );
        let location = Location::from_offsets(self.ctx.source, first_start, first_end);
        self.offenses.push(Offense::new(
            "Layout/FirstArrayElementIndentation",
            msg,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    fn check_right_bracket(
        &mut self,
        right_bracket_off: usize,
        left_bracket_col: usize,
        left_bracket_line_start: usize,
        left_paren: Option<usize>,
        parent_pair: Option<ParentPairInfo>,
    ) {
        if !self.ctx.begins_its_line(right_bracket_off) {
            return;
        }
        let right_col = self.ctx.col_of(right_bracket_off);
        let (expected, base_type) = indent_base(
            self.ctx,
            self.style.into(),
            left_bracket_col,
            left_bracket_line_start,
            left_paren,
            parent_pair,
        );
        if expected == right_col {
            return;
        }
        let msg = right_bracket_message(base_type).to_string();
        let location =
            Location::from_offsets(self.ctx.source, right_bracket_off, right_bracket_off + 1);
        self.offenses.push(Offense::new(
            "Layout/FirstArrayElementIndentation",
            msg,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    fn walk_assoc_elements(&mut self, elements: &[Node]) {
        for (idx, elem) in elements.iter().enumerate() {
            if let Some(assoc) = elem.as_assoc_node() {
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

fn base_description(t: IndentBaseType) -> &'static str {
    match t {
        IndentBaseType::LeftBraceOrBracket => "the position of the opening bracket",
        IndentBaseType::FirstColumnAfterLeftParenthesis => {
            "the first position after the preceding left parenthesis"
        }
        IndentBaseType::ParentHashKey => "the parent hash key",
        IndentBaseType::StartOfLine => "the start of the line where the left square bracket is",
    }
}

fn right_bracket_message(t: IndentBaseType) -> &'static str {
    match t {
        IndentBaseType::LeftBraceOrBracket => "Indent the right bracket the same as the left bracket.",
        IndentBaseType::FirstColumnAfterLeftParenthesis => {
            "Indent the right bracket the same as the first position after the preceding left parenthesis."
        }
        IndentBaseType::ParentHashKey => "Indent the right bracket the same as the parent hash key.",
        IndentBaseType::StartOfLine => {
            "Indent the right bracket the same as the start of the line where the left bracket is."
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        self.check_array(node);
        // Walk children. parent_pair does not apply inside array elements.
        let old = self.parent_pair.take();
        ruby_prism::visit_array_node(self, node);
        self.parent_pair = old;
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        let elements: Vec<Node> = node.elements().iter().collect();
        self.walk_assoc_elements(&elements);
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        let elements: Vec<Node> = node.elements().iter().collect();
        self.walk_assoc_elements(&elements);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.in_interpolation {
            ruby_prism::visit_call_node(self, node);
            return;
        }
        if let Some(recv) = node.receiver() {
            self.visit(&recv);
        }
        let has_parens = node.opening_loc().is_some();
        if has_parens {
            let open = node.opening_loc().unwrap();
            let col = self.ctx.col_of(open.start_offset());
            let line = self.ctx.line_of(open.start_offset());
            self.parent_call_parens.push(ParenInfo { col, line });
        }
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

crate::register_cop!("Layout/FirstArrayElementIndentation", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/FirstArrayElementIndentation");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "consistent" => Style::Consistent,
            "align_brackets" => Style::AlignBrackets,
            _ => Style::SpecialInsideParentheses,
        })
        .unwrap_or(Style::SpecialInsideParentheses);
    let width = cop_config
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_i64())
        .map(|v| v as usize);
    Some(Box::new(FirstArrayElementIndentation::new(style, width)))
});
