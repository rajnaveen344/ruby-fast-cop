//! Style/MultilineWhenThen cop
//!
//! Flags `when ... then` in multiline when branches.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Do not use `then` for multiline `when` statement.";

#[derive(Default)]
pub struct MultilineWhenThen;

impl MultilineWhenThen {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for MultilineWhenThen {
    fn name(&self) -> &'static str {
        "Style/MultilineWhenThen"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = WhenThenVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        ruby_prism::visit_program_node(&mut visitor, &result.node().as_program_node().unwrap());
        visitor.offenses
    }
}

struct WhenThenVisitor<'a> {
    cop: &'a MultilineWhenThen,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl WhenThenVisitor<'_> {
    fn check_when(&mut self, node: &ruby_prism::WhenNode) {
        let then_loc = match node.then_keyword_loc() {
            Some(t) => t,
            None => return,
        };
        let then_start = then_loc.start_offset();
        let then_end = then_loc.end_offset();

        // The `then` keyword must be at end of the when line (not inline with body on same line)
        let then_line = self.line_number(then_start);
        let when_end = node.location().end_offset();

        // Check if body starts on a different line from `then`
        // Get body (statements)
        let body_stmts = node.statements();

        // Case 1: `when bar then\nbody` — then is at end of line, body is on next line(s)
        // Case 2: `when bar then body` — single-line, no offense
        // Case 3: `when bar\n  then body` — `then` is on its own line indented

        match &body_stmts {
            None => {
                // Empty body: `when bar then\nend`
                // then is on its own line relative to end — check if then has newline after
                let source_after_then = &self.ctx.source[then_end..];
                if source_after_then.starts_with('\n') || source_after_then.is_empty() {
                    // multiline (empty body follows)
                    self.emit(then_start, then_end, node);
                }
            }
            Some(stmts) => {
                let parts: Vec<_> = stmts.body().iter().collect();
                if parts.is_empty() {
                    return;
                }
                let first_stmt = &parts[0];
                let stmt_line = self.line_number(first_stmt.location().start_offset());

                // Get the line of the last condition
                let when_cond_line = {
                    let last_cond = node.conditions().iter().last();
                    match last_cond {
                        Some(c) => self.line_number(c.location().end_offset().saturating_sub(1)),
                        None => 0,
                    }
                };

                if stmt_line > then_line {
                    // body on line after `then` → multiline, offense
                    self.emit(then_start, then_end, node);
                } else if stmt_line == then_line && then_line > when_cond_line {
                    // `then` is on its own line (indented-then case): `when bar\n  then stmt`
                    // Even if body is single-line (on same line as then), it's still multiline
                    self.emit(then_start, then_end, node);
                } else if stmt_line == then_line {
                    // `when bar then stmt` — inline, no offense
                    // UNLESS the body spans multiple lines
                    let last_stmt = parts.last().unwrap();
                    let last_line = self.line_number(last_stmt.location().end_offset().saturating_sub(1));
                    if last_line > then_line {
                        // body spans multiple lines starting same line as `then`
                        // Only offense if `then` is on a separate line from conditions
                        if then_line > when_cond_line {
                            self.emit(then_start, then_end, node);
                        }
                        // else: `when bar then multiline_call(arg1,\n  arg2)` → no offense
                    }
                    // else: pure single-line → no offense
                }
            }
        }
    }

    fn emit(&mut self, then_start: usize, then_end: usize, node: &ruby_prism::WhenNode) {
        // Correction: remove ` then` (or `then ` if it starts the line)
        // Find what to delete:
        // If `then` is at end of line after conditions: delete ` then` + trailing whitespace
        // If `then` is at start of indented line: delete `  then ` keeping stmt
        let source = self.ctx.source;

        // Determine if `then` is at the beginning of its line (indented-then case)
        let then_col = self.col_of(then_start);
        let is_line_start = then_col > 0 && source[..then_start].bytes().rev()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count() == then_col;

        let correction = if is_line_start {
            // Delete from start of this line's indentation through `then ` (keep the stmt)
            let line_start = then_start - then_col;
            // After `then ` there may be a space
            let after_then = &source[then_end..];
            let spaces_after = after_then.bytes().take_while(|&b| b == b' ').count();
            // Keep: one space before stmt (stmt was after `then `)
            // Delete: `  then ` → replace with ` ` (one space, since `then` preceded by indent)
            // Actually RuboCop correction: replace `\n  then ` → `\n ` (keep one less space)
            // Simpler: delete from line_start to then_end+spaces_after, replace with `\n` + (then_col-1) spaces
            let del_start = line_start;
            let del_end = then_end + spaces_after;
            let indent = " ".repeat(then_col.saturating_sub(1));
            Correction::replace(del_start, del_end, indent)
        } else {
            // Delete ` then` at end of conditions line
            // Find the space before `then`
            let space_before = if then_start > 0 && source.as_bytes()[then_start - 1] == b' ' {
                1
            } else {
                0
            };
            Correction::replace(then_start - space_before, then_end, String::new())
        };

        self.offenses.push(
            self.ctx.offense_with_range(self.cop.name(), MSG, self.cop.severity(), then_start, then_end)
                .with_correction(correction)
        );
    }

    fn line_number(&self, offset: usize) -> usize {
        self.ctx.source[..offset].bytes().filter(|&b| b == b'\n').count() + 1
    }

    fn col_of(&self, offset: usize) -> usize {
        let line_start = self.ctx.source[..offset].rfind('\n').map_or(0, |p| p + 1);
        offset - line_start
    }
}

impl Visit<'_> for WhenThenVisitor<'_> {
    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode) {
        self.check_when(node);
        ruby_prism::visit_when_node(self, node);
    }
}

crate::register_cop!("Style/MultilineWhenThen", |_cfg| {
    Some(Box::new(MultilineWhenThen::new()))
});
