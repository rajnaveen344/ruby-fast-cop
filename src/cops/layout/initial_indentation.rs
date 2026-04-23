//! Layout/InitialIndentation cop
//! Checks for indentation of the first non-blank non-comment line in the file.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/initial_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct InitialIndentation;

impl InitialIndentation {
    pub fn new() -> Self { Self }
}

impl Cop for InitialIndentation {
    fn name(&self) -> &'static str { "Layout/InitialIndentation" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        if source.is_empty() {
            return vec![];
        }

        // Skip BOM if present (UTF-8 BOM is 3 bytes: 0xEF 0xBB 0xBF)
        let bom_len = if source.starts_with('\u{FEFF}') { 3usize } else { 0 };
        let effective = &source[bom_len..];

        // Find the first non-blank, non-comment line
        // (like RuboCop's first_token which skips # comments)
        let mut line_abs_start = bom_len; // absolute offset of current line start
        let mut first_code_line: Option<&str> = None;
        let mut first_code_line_abs: usize = 0;

        for line in effective.split('\n') {
            let trimmed = line.trim_start();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                first_code_line = Some(line);
                first_code_line_abs = line_abs_start;
                break;
            }
            line_abs_start += line.len() + 1; // +1 for '\n'
        }

        let line = match first_code_line {
            Some(l) => l,
            None => return vec![],
        };

        // Count leading whitespace bytes on this line
        // For BOM lines: the BOM chars (3 bytes) don't count as whitespace indent
        let indent_bytes: usize = line.chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(|c| c.len_utf8())
            .sum();

        if indent_bytes == 0 {
            return vec![];
        }

        // Token starts at: line_abs_start + indent_bytes
        let token_abs_start = first_code_line_abs + indent_bytes;

        // Find end of first token (up to next whitespace or end of line)
        let line_after_indent = &line[indent_bytes..];
        let token_len = line_after_indent
            .find(|c: char| c.is_whitespace())
            .unwrap_or(line_after_indent.len());
        let token_abs_end = token_abs_start + token_len;

        let msg = "Indentation of first line in file detected.";
        let offense = ctx.offense_with_range(
            self.name(), msg, self.severity(),
            token_abs_start,
            token_abs_end,
        );
        // Note: correction not implemented (RuboCop removes leading whitespace before the token)

        vec![offense]
    }
}

crate::register_cop!("Layout/InitialIndentation", |_cfg| Some(Box::new(InitialIndentation::new())));
