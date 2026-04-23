//! Layout/LeadingEmptyLines cop
//! Checks for unnecessary blank lines at the beginning of the source.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct LeadingEmptyLines;

impl LeadingEmptyLines {
    pub fn new() -> Self { Self }
}

impl Cop for LeadingEmptyLines {
    fn name(&self) -> &'static str { "Layout/LeadingEmptyLines" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        if source.is_empty() {
            return vec![];
        }

        // Skip BOM if present
        let start = if source.starts_with('\u{FEFF}') { 3 } else { 0 };
        let effective = &source[start..];

        if !effective.starts_with('\n') {
            return vec![];
        }

        // Count leading blank lines (empty or whitespace-only) and find first non-blank line
        let mut blank_end = start;
        for line in effective.split('\n') {
            if line.trim().is_empty() {
                blank_end += line.len() + 1; // +1 for \n
            } else {
                break;
            }
        }

        // If all lines are blank, no offense
        if blank_end >= source.len() {
            return vec![];
        }

        // Find the first non-blank line
        let first_line_start = blank_end;
        let first_line_end = source[first_line_start..]
            .find('\n')
            .map(|i| first_line_start + i)
            .unwrap_or(source.len());

        let msg = "Unnecessary blank line at the beginning of the source.";
        let line_content = &source[first_line_start..first_line_end];
        // RuboCop reports offense end at:
        // - end of line for comments (starts with #)
        // - first space/end for other tokens
        let token_end = if line_content.starts_with('#') {
            line_content.len()
        } else {
            line_content.find(|c: char| c.is_whitespace()).unwrap_or(line_content.len())
        };
        let offense = ctx.offense_with_range(
            self.name(), msg, self.severity(),
            first_line_start,
            first_line_start + token_end,
        ).with_correction(Correction::delete(start, first_line_start));

        vec![offense]
    }
}

crate::register_cop!("Layout/LeadingEmptyLines", |_cfg| Some(Box::new(LeadingEmptyLines::new())));
