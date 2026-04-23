//! Lint/EmptyFile cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/empty_file.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

pub struct EmptyFile {
    allow_comments: bool,
}

impl EmptyFile {
    pub fn new(allow_comments: bool) -> Self {
        Self { allow_comments }
    }
}

impl Default for EmptyFile {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Cop for EmptyFile {
    fn name(&self) -> &'static str { "Lint/EmptyFile" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if ctx.source.is_empty() {
            return vec![ctx.offense_with_range(
                "Lint/EmptyFile",
                "Empty file detected.",
                Severity::Warning,
                0,
                0,
            )];
        }

        if !self.allow_comments {
            // Check if file contains only comments and blank lines
            let only_comments = ctx.source.lines().all(|line| {
                let trimmed = line.trim();
                trimmed.is_empty() || trimmed.starts_with('#')
            });
            if only_comments {
                return vec![ctx.offense_with_range(
                    "Lint/EmptyFile",
                    "Empty file detected.",
                    Severity::Warning,
                    0,
                    0,
                )];
            }
        }

        vec![]
    }
}

crate::register_cop!("Lint/EmptyFile", |cfg| {
    let allow = cfg.get_cop_config("Lint/EmptyFile")
        .and_then(|c| c.raw.get("AllowComments"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(EmptyFile::new(allow)))
});
