//! Lint/ScriptPermission — flags Ruby files with a shebang that lack execute permission.
//!
//! Port of `lib/rubocop/cop/lint/script_permission.rb` (RuboCop v1.85.0).
//!
//! Checks on `check_program`: if source starts with `#!` and the file on disk is
//! not executable (no x bit set in mode), emit an offense covering the shebang line.
//!
//! Skips when:
//! - `CheckContext.file_path` is `None` (stdin / in-memory input).
//! - Host OS is Windows (no concept of executable permission).
//! - Source does not start with `#!`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct ScriptPermission;

impl ScriptPermission {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ScriptPermission {
    fn name(&self) -> &'static str {
        "Lint/ScriptPermission"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        _node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        // Windows has no execute-bit semantics; mirror RuboCop's Platform.windows? check.
        if cfg!(windows) {
            return vec![];
        }

        let Some(path) = ctx.file_path else {
            return vec![];
        };

        if !ctx.source.starts_with("#!") {
            return vec![];
        }

        if is_executable(path) {
            return vec![];
        }

        // Offense covers the shebang line (first comment) — byte range 0..end-of-first-line.
        let bytes = ctx.source.as_bytes();
        let end = bytes.iter().position(|&b| b == b'\n').unwrap_or(bytes.len());

        let basename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let message = format!("Script file {} doesn't have execute permission.", basename);

        vec![ctx.offense_with_range(
            self.name(),
            &message,
            self.severity(),
            0,
            end,
        )]
    }
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(true) // If we can't stat it, don't flag — matches "assume ok".
}

#[cfg(not(unix))]
fn is_executable(_path: &std::path::Path) -> bool {
    true
}

crate::register_cop!("Lint/ScriptPermission", |_cfg| {
    Some(Box::new(ScriptPermission::new()))
});
