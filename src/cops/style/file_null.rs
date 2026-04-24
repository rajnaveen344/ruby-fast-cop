//! Style/FileNull cop
//!
//! Suggests `File::NULL` instead of hardcoded `/dev/null`, `NUL`, or `NUL:`.
//! `NUL` alone is only flagged if the file also contains `/dev/null` (to avoid
//! false positives on unrelated strings).
//! Strings inside arrays/hashes are skipped.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct FileNull;

impl FileNull {
    pub fn new() -> Self {
        Self
    }
}

fn is_null_device(v: &str) -> Option<&'static str> {
    // Returns Some(category) where category is "devnull" or "nul".
    if v.is_empty() {
        return None;
    }
    let lower = v.to_ascii_lowercase();
    if lower == "/dev/null" {
        return Some("devnull");
    }
    if lower == "nul" {
        return Some("nul");
    }
    if lower == "nul:" {
        return Some("nul_colon");
    }
    None
}

impl Cop for FileNull {
    fn name(&self) -> &'static str {
        "Style/FileNull"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        // Pass 1: does file contain any `/dev/null` literal string?
        let mut pre = PreScan { has_devnull: false };
        pre.visit_program_node(node);

        let mut v = Visitor {
            ctx,
            has_devnull: pre.has_devnull,
            skip_depth: 0,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct PreScan {
    has_devnull: bool,
}

impl Visit<'_> for PreScan {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        let bytes = node.unescaped();
        if let Ok(s) = std::str::from_utf8(bytes) {
            if !s.is_empty() && s.to_ascii_lowercase() == "/dev/null" {
                self.has_devnull = true;
            }
        }
        ruby_prism::visit_string_node(self, node);
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    has_devnull: bool,
    skip_depth: u32,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        self.skip_depth += 1;
        ruby_prism::visit_array_node(self, node);
        self.skip_depth -= 1;
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        // Direct children are pairs; strings inside pairs should be skipped.
        self.skip_depth += 1;
        ruby_prism::visit_hash_node(self, node);
        self.skip_depth -= 1;
    }

    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if self.skip_depth == 0 {
            let bytes = node.unescaped();
            if let Ok(s) = std::str::from_utf8(bytes) {
                if let Some(cat) = is_null_device(s) {
                    if cat == "devnull" || cat == "nul_colon" || self.has_devnull {
                        let loc = node.location();
                        let start = loc.start_offset();
                        let end = loc.end_offset();
                        let msg = format!("Use `File::NULL` instead of `{}`.", s);
                        self.offenses.push(
                            self.ctx
                                .offense_with_range(
                                    "Style/FileNull",
                                    &msg,
                                    Severity::Convention,
                                    start,
                                    end,
                                )
                                .with_correction(Correction::replace(start, end, "File::NULL")),
                        );
                    }
                }
            }
        }
        ruby_prism::visit_string_node(self, node);
    }
}

crate::register_cop!("Style/FileNull", |_cfg| Some(Box::new(FileNull::new())));
