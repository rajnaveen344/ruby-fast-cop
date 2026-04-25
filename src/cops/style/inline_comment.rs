//! Style/InlineComment — checks for trailing inline comments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str = "Avoid trailing inline comments.";

#[derive(Default)]
pub struct InlineComment;

impl InlineComment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for InlineComment {
    fn name(&self) -> &'static str {
        "Style/InlineComment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut offenses = Vec::new();

        for c in result.comments() {
            let loc = c.location();
            let cstart = loc.start_offset();
            let cend = loc.end_offset();
            let text = &ctx.source[cstart..cend];

            // Skip rubocop directive comments
            if text.starts_with("# rubocop:disable") || text.starts_with("# rubocop:enable") {
                continue;
            }

            // Standalone comment: line up to comment start is whitespace only
            let line_start = ctx.line_start(cstart);
            let prefix = &ctx.source[line_start..cstart];
            if prefix.chars().all(char::is_whitespace) {
                continue;
            }

            offenses.push(ctx.offense_with_range(self.name(), MSG, self.severity(), cstart, cend));
        }

        offenses
    }
}

crate::register_cop!("Style/InlineComment", |_cfg| Some(Box::new(InlineComment::new())));
