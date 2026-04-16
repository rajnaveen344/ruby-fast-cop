//! Style/WhileUntilModifier - Checks for while/until statements that would fit
//! on one line as modifier form.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/while_until_modifier.rb
//! Mixin: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/statement_modifier.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/WhileUntilModifier";
const DEFAULT_MAX_LINE_LENGTH: usize = 80;
const MSG: &str = "Favor modifier `%KEYWORD%` usage when having a single-line body.";

pub struct WhileUntilModifier {
    max_line_length: usize,
    line_length_enabled: bool,
}

impl Default for WhileUntilModifier {
    fn default() -> Self {
        Self {
            max_line_length: DEFAULT_MAX_LINE_LENGTH,
            line_length_enabled: true,
        }
    }
}

impl WhileUntilModifier {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(max_line_length: usize, line_length_enabled: bool) -> Self {
        Self { max_line_length, line_length_enabled }
    }
}

impl Cop for WhileUntilModifier {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            max_line_length: self.max_line_length,
            line_length_enabled: self.line_length_enabled,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    max_line_length: usize,
    line_length_enabled: bool,
}

impl<'a> Visitor<'a> {
    fn check_loop(
        &mut self,
        keyword: &str,
        keyword_loc: ruby_prism::Location,
        predicate: &Node,
        statements: &Option<ruby_prism::StatementsNode>,
        end_keyword_loc: Option<ruby_prism::Location>,
        node_start: usize,
        node_end: usize,
    ) {
        // modifier_form? — no end keyword = already modifier form
        let is_modifier = end_keyword_loc.is_none();
        if is_modifier { return; }

        // non_eligible_body
        let body_items: Vec<Node> = statements
            .as_ref()
            .map(|s| s.body().iter().collect())
            .unwrap_or_default();
        if body_items.is_empty() { return; }
        if body_items.len() > 1 { return; } // begin_type equivalent — multiple stmts

        // nonempty_line_count > 3
        let node_src = &self.ctx.source[node_start..node_end];
        let non_empty_line_count = node_src.lines().filter(|l| !l.trim().is_empty()).count();
        if non_empty_line_count > 3 { return; }

        // line_with_comment on last line (end line)
        if let Some(end_loc) = &end_keyword_loc {
            let last_line = self.ctx.line_of(end_loc.start_offset());
            if self.line_has_comment(last_line) {
                // but only if first_line_comment + code_after_end, or if end line itself has comment
                // processed_source.line_with_comment?(node.loc.last_line) — any comment on that line
                return;
            }
        }

        // first_line_comment + code_after
        let first_line_has_comment = self.line_has_comment(self.ctx.line_of(keyword_loc.start_offset()));
        if first_line_has_comment && self.has_code_after_end(&end_keyword_loc) {
            return;
        }

        // body contains comment
        if let Some(stmts) = statements {
            let s_start = stmts.location().start_offset();
            let s_end = stmts.location().end_offset();
            if self.region_contains_comment(s_start, s_end) {
                return;
            }
        }

        // condition has lvasgn
        if has_lvasgn_in_condition(predicate) { return; }

        // modifier fits on single line
        if !self.modifier_fits(keyword, keyword_loc.start_offset(), predicate, &body_items, &end_keyword_loc) {
            return;
        }

        let msg = MSG.replace("%KEYWORD%", keyword);
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, &msg, Severity::Convention,
            keyword_loc.start_offset(), keyword_loc.end_offset(),
        ));
    }

    fn modifier_fits(
        &self,
        keyword: &str,
        keyword_start: usize,
        predicate: &Node,
        body_items: &[Node],
        end_keyword_loc: &Option<ruby_prism::Location>,
    ) -> bool {
        if !self.line_length_enabled { return true; }

        let body = &body_items[0];
        let body_src = &self.ctx.source[body.location().start_offset()..body.location().end_offset()];
        let cond_src = &self.ctx.source[predicate.location().start_offset()..predicate.location().end_offset()];

        let keyword_col = self.ctx.col_of(keyword_start);
        let line_text = self.ctx.line_text(keyword_start);
        let code_before = &line_text[..keyword_col.min(line_text.len())];

        let expression = format!("{} {} {}", body_src, keyword, cond_src);
        let needs_parens = self.needs_parenthesization(keyword_start);
        let expression = if needs_parens { format!("({})", expression) } else { expression };

        let first_line_comment = self.first_line_comment_text(keyword_start);
        let expression = match &first_line_comment {
            Some(c) => format!("{} {}", expression, c),
            None => expression,
        };

        let code_after = self.code_after_end_str(end_keyword_loc);
        let full = match &code_after {
            Some(after) => format!("{}{}{}", code_before, expression, after),
            None => format!("{}{}", code_before, expression),
        };
        full.len() <= self.max_line_length
    }

    fn needs_parenthesization(&self, keyword_start: usize) -> bool {
        // scan back through whitespace, find significant token
        let bytes = self.ctx.source.as_bytes();
        let mut i = keyword_start;
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b' ' | b'\t' | b'\n' | b'\r' => continue,
                b'=' => {
                    if i > 0 && matches!(bytes[i - 1], b'=' | b'!' | b'>' | b'<') {
                        return false;
                    }
                    return true;
                }
                b'[' | b'(' => return true,
                b',' => return true,
                b':' => {
                    if i > 0 && bytes[i - 1] == b':' { return false; }
                    return true;
                }
                b'+' | b'-' | b'*' | b'/' | b'%' | b'^' => return true,
                b'|' | b'&' => return true,
                _ => return false,
            }
        }
        false
    }

    fn first_line_comment_text(&self, keyword_start: usize) -> Option<String> {
        let first_line = self.ctx.line_text(keyword_start);
        if let Some(hash_pos) = source::find_comment_start(first_line) {
            let comment = first_line[hash_pos..].trim_end();
            if !is_cop_directive(comment) {
                return Some(comment.to_string());
            }
        }
        None
    }

    fn code_after_end_str(&self, end_keyword_loc: &Option<ruby_prism::Location>) -> Option<String> {
        if let Some(end_loc) = end_keyword_loc {
            let end_line = self.ctx.line_text(end_loc.start_offset());
            let end_col = self.ctx.col_of(end_loc.start_offset());
            if end_col + 3 <= end_line.len() {
                let after = &end_line[end_col + 3..];
                if !after.trim().is_empty() {
                    return Some(after.to_string());
                }
            }
        }
        None
    }

    fn has_code_after_end(&self, end_keyword_loc: &Option<ruby_prism::Location>) -> bool {
        self.code_after_end_str(end_keyword_loc).is_some()
    }

    fn line_has_comment(&self, line_num: usize) -> bool {
        let line_offset = source::line_byte_offset(self.ctx.source, line_num);
        let line_text = self.ctx.line_text(line_offset);
        source::find_comment_start(line_text).is_some()
    }

    fn region_contains_comment(&self, start: usize, end: usize) -> bool {
        let start_line = self.ctx.line_of(start);
        let end_line = self.ctx.line_of(end);
        for line_num in start_line..=end_line {
            let line_offset = source::line_byte_offset(self.ctx.source, line_num);
            let line_text = self.ctx.line_text(line_offset);
            if source::find_comment_start(line_text).is_some() {
                return true;
            }
        }
        false
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        self.check_loop(
            "while",
            node.keyword_loc(),
            &node.predicate(),
            &node.statements(),
            node.closing_loc(),
            node.location().start_offset(),
            node.location().end_offset(),
        );
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.check_loop(
            "until",
            node.keyword_loc(),
            &node.predicate(),
            &node.statements(),
            node.closing_loc(),
            node.location().start_offset(),
            node.location().end_offset(),
        );
        ruby_prism::visit_until_node(self, node);
    }
}

fn has_lvasgn_in_condition(node: &Node) -> bool {
    struct F { found: bool }
    impl Visit<'_> for F {
        fn visit_local_variable_write_node(&mut self, _: &ruby_prism::LocalVariableWriteNode) {
            self.found = true;
        }
        fn visit_local_variable_operator_write_node(&mut self, _: &ruby_prism::LocalVariableOperatorWriteNode) {
            self.found = true;
        }
        fn visit_local_variable_or_write_node(&mut self, _: &ruby_prism::LocalVariableOrWriteNode) {
            self.found = true;
        }
        fn visit_local_variable_and_write_node(&mut self, _: &ruby_prism::LocalVariableAndWriteNode) {
            self.found = true;
        }
    }
    let mut f = F { found: false };
    f.visit(node);
    f.found
}

fn is_cop_directive(comment: &str) -> bool {
    let normalized = comment.replace(' ', "");
    normalized.contains("rubocop:disable") || normalized.contains("rubocop:todo")
}
