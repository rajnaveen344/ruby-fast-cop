//! Lint/DuplicateMagicComment cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

const MSG: &str = "Duplicate magic comment detected.";
const MAGIC_KEYS: &[&str] = &[
    "encoding", "frozen_string_literal", "warn_indent",
    "shareable_constant_value", "typed", "warn_past_scope",
];

#[derive(Default)]
pub struct DuplicateMagicComment;

impl DuplicateMagicComment {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateMagicComment {
    fn name(&self) -> &'static str { "Lint/DuplicateMagicComment" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let bytes = source.as_bytes();
        let mut offenses = vec![];
        let mut seen: Vec<String> = vec![];
        let mut line_start = 0usize;
        while line_start < bytes.len() {
            let mut line_end = line_start;
            while line_end < bytes.len() && bytes[line_end] != b'\n' { line_end += 1; }
            let line = &source[line_start..line_end];
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                if trimmed.is_empty() {
                    let next = line_end + if line_end < bytes.len() { 1 } else { 0 };
                    line_start = next;
                    continue;
                }
                break;
            }
            let rest = trimmed[1..].trim_start();
            if let Some(colon) = rest.find(':') {
                let key = rest[..colon].trim().to_lowercase();
                if MAGIC_KEYS.iter().any(|&k| k == key) {
                    if seen.contains(&key) {
                        let del_end = if line_end < bytes.len() { line_end + 1 } else { line_end };
                        let off = ctx.offense_with_range(
                            "Lint/DuplicateMagicComment", MSG, Severity::Warning,
                            line_start, line_end,
                        ).with_correction(Correction::delete(line_start, del_end));
                        offenses.push(off);
                    } else {
                        seen.push(key);
                    }
                }
            }
            line_start = line_end + 1;
        }
        offenses
    }
}

crate::register_cop!("Lint/DuplicateMagicComment", |_cfg| Some(Box::new(DuplicateMagicComment::new())));
