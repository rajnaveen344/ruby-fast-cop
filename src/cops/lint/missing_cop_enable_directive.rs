//! Lint/MissingCopEnableDirective - Every rubocop:disable must have a rubocop:enable.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/missing_cop_enable_directive.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::ProgramNode;

pub struct MissingCopEnableDirective {
    max_range: f64, // f64::INFINITY means unlimited
    disabled_in_config: Vec<String>, // cop names explicitly Enabled: false in config
}

impl MissingCopEnableDirective {
    pub fn new(max_range: f64, disabled_in_config: Vec<String>) -> Self {
        Self { max_range, disabled_in_config }
    }
}

impl Default for MissingCopEnableDirective {
    fn default() -> Self {
        Self { max_range: f64::INFINITY, disabled_in_config: Vec::new() }
    }
}

/// Parsed directive: disable or enable for a cop/dept name.
#[derive(Debug)]
struct Directive {
    line: usize, // 1-based
    comment_start: usize,
    comment_end: usize,
    is_enable: bool,
    name: String,
    is_department: bool, // true if no '/' in name
}

/// Parse all `# rubocop:disable/enable NAME` directives in source.
fn scan_directives(src: &str) -> Vec<Directive> {
    let mut result = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut line = 1usize;

    while i < bytes.len() {
        let line_start = i;
        // find end of line
        let mut line_end = i;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }
        let line_str = &src[line_start..line_end];

        // Find `# rubocop:disable` or `# rubocop:enable` anywhere on the line
        if let Some(pos) = line_str.find("rubocop:") {
            let rest = &line_str[pos + 8..]; // after "rubocop:"
            let is_enable = if rest.starts_with("enable") {
                true
            } else if rest.starts_with("disable") || rest.starts_with("todo") {
                false
            } else {
                i = if line_end < bytes.len() { line_end + 1 } else { bytes.len() };
                if line_end < bytes.len() { line += 1; }
                continue;
            };

            let keyword_len = if is_enable { 6 } else if rest.starts_with("todo") { 4 } else { 7 };
            let after_keyword = &rest[keyword_len..].trim_start();

            // Parse comma-separated cop/dept names
            let names_str = *after_keyword;
            let names: Vec<&str> = names_str
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty() && *s != "all")
                .collect();

            // Find comment_start (position of '#')
            let hash_pos = line_str.find('#').unwrap_or(0);
            let comment_start = line_start + hash_pos;
            let comment_end = line_end;

            for name in names {
                let is_department = !name.contains('/');
                result.push(Directive {
                    line,
                    comment_start,
                    comment_end,
                    is_enable,
                    name: name.to_string(),
                    is_department,
                });
            }
        }

        i = if line_end < bytes.len() { line_end + 1 } else { bytes.len() };
        if line_end < bytes.len() { line += 1; }
    }

    result
}

impl Cop for MissingCopEnableDirective {
    fn name(&self) -> &'static str {
        "Lint/MissingCopEnableDirective"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, _node: &ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let src = ctx.source;
        if !src.contains("rubocop:disable") && !src.contains("rubocop:todo") {
            return vec![];
        }

        let directives = scan_directives(src);
        let total_lines = src.lines().count();

        struct DisableEntry {
            name: String,
            disable_line: usize,
            dir_idx: usize,
            is_department: bool,
        }

        let mut stack: Vec<DisableEntry> = Vec::new();
        let mut offenses = Vec::new();

        for (idx, dir) in directives.iter().enumerate() {
            if !dir.is_enable {
                stack.push(DisableEntry {
                    name: dir.name.clone(),
                    disable_line: dir.line,
                    dir_idx: idx,
                    is_department: dir.is_department,
                });
            } else {
                // Matching enable found — check if range was too large
                let pos = stack.iter().rposition(|e| e.name == dir.name);
                if let Some(p) = pos {
                    let entry = stack.remove(p);
                    // Range = from disable_line to enable_line (inclusive on both)
                    // RuboCop formula: range.max - range.min < max_range + 2
                    // range.min = disable_line, range.max = enable_line
                    let range_diff = dir.line - entry.disable_line;
                    if self.max_range != f64::INFINITY && (range_diff as f64) >= self.max_range + 2.0 {
                        let disable_dir = &directives[entry.dir_idx];
                        let msg = self.format_message(&entry.name, entry.is_department, true);
                        offenses.push(ctx.offense_with_range(
                            "Lint/MissingCopEnableDirective",
                            &msg,
                            Severity::Warning,
                            disable_dir.comment_start,
                            disable_dir.comment_end,
                        ));
                    }
                }
                // If no matching disable found → redundant enable (handled by RedundantCopEnableDirective)
            }
        }

        // Remaining stack = disables never re-enabled (range extends to EOF = Float::INFINITY)
        for entry in &stack {
            let dir = &directives[entry.dir_idx];

            // Skip if cop is explicitly disabled in config and range extends to EOF
            // (RuboCop: cop_class disabled in registry for this config + range.max == Infinity)
            if self.disabled_in_config.contains(&entry.name) {
                continue;
            }

            let bounded = self.max_range != f64::INFINITY;
            let msg = self.format_message(&entry.name, entry.is_department, bounded);
            offenses.push(ctx.offense_with_range(
                "Lint/MissingCopEnableDirective",
                &msg,
                Severity::Warning,
                dir.comment_start,
                dir.comment_end,
            ));
        }

        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

impl MissingCopEnableDirective {
    fn format_message(&self, name: &str, is_department: bool, bounded: bool) -> String {
        let dept_name = if is_department {
            name.to_string()
        } else {
            name.to_string()
        };
        let type_str = if is_department { "department" } else { "cop" };
        if bounded {
            format!(
                "Re-enable {} {} within {} lines after disabling it.",
                dept_name, type_str, self.max_range as i64
            )
        } else {
            format!(
                "Re-enable {} {} with `# rubocop:enable` after disabling it.",
                dept_name, type_str
            )
        }
    }
}

crate::register_cop!("Lint/MissingCopEnableDirective", |cfg| {
    let max_range = cfg
        .get_cop_config("Lint/MissingCopEnableDirective")
        .and_then(|c| c.raw.get("MaximumRangeSize"))
        .and_then(|v| match v {
            serde_yaml::Value::Number(n) => {
                if n.is_f64() {
                    n.as_f64()
                } else {
                    n.as_i64().map(|i| i as f64)
                }
            }
            _ => None,
        })
        .unwrap_or(f64::INFINITY);

    let disabled_in_config: Vec<String> = cfg
        .cops
        .iter()
        .filter(|(_, c)| c.enabled == Some(false))
        .map(|(k, _)| k.clone())
        .collect();

    Some(Box::new(MissingCopEnableDirective::new(max_range, disabled_in_config)))
});
