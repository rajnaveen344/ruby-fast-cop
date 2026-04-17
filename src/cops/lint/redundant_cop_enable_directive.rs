//! Lint/RedundantCopEnableDirective cop
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/redundant_cop_enable_directive.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use std::collections::HashMap;

pub struct RedundantCopEnableDirective {
    /// Cop names that are disabled in the user's config. Every such cop starts
    /// with an implicit "disable" count of 1 so a single `# rubocop:enable` is
    /// treated as legitimate.
    disabled_in_config: Vec<String>,
}

impl RedundantCopEnableDirective {
    pub fn new() -> Self {
        Self { disabled_in_config: Vec::new() }
    }

    pub fn with_disabled_in_config(names: Vec<String>) -> Self {
        Self { disabled_in_config: names }
    }
}

impl Default for RedundantCopEnableDirective {
    fn default() -> Self {
        Self::new()
    }
}

/// Parsed `# rubocop:(enable|disable) NAMES` comment.
#[derive(Debug)]
struct Directive {
    /// Line number (1-based).
    line: u32,
    /// Byte offset of first char of comment on that line (`#`).
    comment_start: usize,
    /// Whether this is an enable (vs disable). Only enables generate offenses.
    enable: bool,
    /// Cop/department name tokens as they appear in the comment, paired with their
    /// column (0-based, byte) within the comment text.
    names: Vec<DirName>,
    /// Full text of the comment (no newline).
    text: String,
    /// Whether the directive applies to `all` (was written as `enable all`).
    is_all: bool,
}

#[derive(Debug, Clone)]
struct DirName {
    name: String,
    /// Byte offset in the comment (0-based) where the name starts.
    col_in_comment: usize,
}

/// Scan a source for `# rubocop:(enable|disable) ...` comments.
fn scan_directives(src: &str) -> Vec<Directive> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut line: u32 = 1;
    let mut i: usize = 0;
    while i < bytes.len() {
        // Find start of next line content
        let line_start = i;

        // Skip leading whitespace to find first non-ws
        let mut j = line_start;
        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        // Find end of line
        let mut line_end = line_start;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }

        // Check "comment-only line" condition: first non-ws is `#`
        if j < line_end && bytes[j] == b'#' {
            let comment_text = &src[j..line_end];
            if let Some(dir) = parse_directive(comment_text, line, j) {
                out.push(dir);
            }
        }

        // Advance to next line
        i = if line_end < bytes.len() { line_end + 1 } else { bytes.len() };
        if line_end < bytes.len() {
            line += 1;
        }
    }
    out
}

/// Parse a single `#`-starting comment. Returns `Some` for enable/disable directives.
fn parse_directive(text: &str, line: u32, comment_start: usize) -> Option<Directive> {
    // Expect `# rubocop:enable ...` or `# rubocop:disable ...` (with flexible whitespace).
    // Parse prefix manually.
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] != b'#' {
        return None;
    }
    let mut p = 1;
    // Optional whitespace
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t') {
        p += 1;
    }
    // "rubocop"
    if !text[p..].starts_with("rubocop") {
        return None;
    }
    p += 7;
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t') {
        p += 1;
    }
    if p >= bytes.len() || bytes[p] != b':' {
        return None;
    }
    p += 1;
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t') {
        p += 1;
    }
    // mode
    let mode_start = p;
    while p < bytes.len() && bytes[p].is_ascii_alphabetic() {
        p += 1;
    }
    let mode = &text[mode_start..p];
    let enable = match mode {
        "enable" => true,
        "disable" | "todo" => false,
        _ => return None,
    };
    // Skip whitespace
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t') {
        p += 1;
    }
    // Parse names list: comma-separated NAME tokens
    let mut names = Vec::new();
    let mut is_all = false;
    loop {
        // skip spaces/tabs
        while p < bytes.len() && matches!(bytes[p], b' ' | b'\t') {
            p += 1;
        }
        if p >= bytes.len() {
            break;
        }
        let start = p;
        while p < bytes.len() && is_name_char(bytes[p]) {
            p += 1;
        }
        if start == p {
            break;
        }
        let name = &text[start..p];
        if name == "all" {
            is_all = true;
        } else {
            names.push(DirName { name: name.to_string(), col_in_comment: start });
        }
        // skip spaces/tabs
        while p < bytes.len() && matches!(bytes[p], b' ' | b'\t') {
            p += 1;
        }
        if p < bytes.len() && bytes[p] == b',' {
            p += 1;
            continue;
        } else {
            break;
        }
    }

    Some(Directive {
        line,
        comment_start,
        enable,
        names,
        text: text.to_string(),
        is_all,
    })
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'/' || b == b'_'
}

impl Cop for RedundantCopEnableDirective {
    fn name(&self) -> &'static str {
        "Lint/RedundantCopEnableDirective"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        _node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if !ctx.source.contains("enable") {
            return vec![];
        }

        let directives = scan_directives(ctx.source);
        let mut counts: HashMap<String, i32> = HashMap::new();
        for name in &self.disabled_in_config {
            *counts.entry(name.clone()).or_insert(0) += 1;
        }

        // Process in source order, mirroring CommentConfig#handle_switch / handle_enable_all.
        // Each "extras" entry = (directive_ref, name) - "all" handled specially.
        let mut offenses = Vec::new();

        for dir in &directives {
            if !dir.enable {
                if dir.is_all {
                    // Not handled for redundant detection; `:disable all` disables all,
                    // but we don't track it here.
                    continue;
                }
                for n in &dir.names {
                    *counts.entry(n.name.clone()).or_insert(0) += 1;
                }
                continue;
            }

            // Enable directive
            if dir.is_all {
                // handle_enable_all: count positive counters; decrement each; if none positive, flag all
                let mut enabled_cops = 0;
                for (_, v) in counts.iter_mut() {
                    if *v > 0 {
                        *v -= 1;
                        enabled_cops += 1;
                    }
                }
                if enabled_cops == 0 {
                    offenses.push(self.build_offense_all(dir, ctx));
                }
            } else {
                // handle_switch: iterate names in declaration order
                let mut extras: Vec<DirName> = Vec::new();
                // Track whether ANY expanded cop in this directive matched a disable.
                // Literal model can't expand departments, so approximate: if a dept name
                // is enabled and any disabled counter starts with "DEPT/", note it.
                let mut related_dept_match = false;
                for n in &dir.names {
                    let entry = counts.entry(n.name.clone()).or_insert(0);
                    if *entry > 0 {
                        *entry -= 1;
                    } else {
                        extras.push(n.clone());
                    }
                    if !n.name.contains('/') {
                        let prefix = format!("{}/", n.name);
                        for (k, v) in counts.iter_mut() {
                            if *v > 0 && k.starts_with(&prefix) {
                                *v -= 1;
                                related_dept_match = true;
                                break;
                            }
                        }
                    }
                }
                for x in &extras {
                    offenses.push(self.build_offense(dir, x, &extras, related_dept_match, ctx));
                }
            }
        }

        offenses
    }
}

impl RedundantCopEnableDirective {
    fn build_offense_all(&self, dir: &Directive, ctx: &CheckContext) -> Offense {
        // Locate "all" position in comment text
        let col = dir.text.find("all").unwrap_or(0);
        let start = dir.comment_start + col;
        let end = start + 3;
        let msg = "Unnecessary enabling of all cops.";
        // Correction: remove entire comment + surrounding space
        let correction = correction_remove_whole_comment(dir, ctx);
        ctx.offense_with_range(self.name(), msg, Severity::Warning, start, end)
            .with_correction(correction)
    }

    fn build_offense(
        &self,
        dir: &Directive,
        name: &DirName,
        all_extras: &[DirName],
        related_dept_match: bool,
        ctx: &CheckContext,
    ) -> Offense {
        let start = dir.comment_start + name.col_in_comment;
        let end = start + name.name.len();
        let msg = format!("Unnecessary enabling of {}.", name.name);

        // Correction: whole-line removal only when ALL names redundant and NO related
        // department/cop decrement happened. Otherwise we must leave an empty line so
        // surrounding directives keep working.
        let whole_line = dir.names.len() == all_extras.len() && !dir.is_all && !related_dept_match;
        let correction = if whole_line {
            correction_remove_whole_comment(dir, ctx)
        } else if dir.names.len() == all_extras.len() {
            // keep newline: remove comment text only
            correction_remove_comment_text_only(dir, ctx)
        } else {
            correction_remove_name(dir, name, ctx)
        };

        ctx.offense_with_range(self.name(), &msg, Severity::Warning, start, end)
            .with_correction(correction)
    }
}

/// Remove the comment text but leave the trailing newline (creates an empty line).
fn correction_remove_comment_text_only(dir: &Directive, ctx: &CheckContext) -> Correction {
    let src = ctx.source;
    let line_start = src[..dir.comment_start].rfind('\n').map_or(0, |p| p + 1);
    let end = dir.comment_start + dir.text.len();
    Correction::delete(line_start, end)
}

/// Remove the whole directive comment line including its trailing newline.
fn correction_remove_whole_comment(dir: &Directive, ctx: &CheckContext) -> Correction {
    // Start at beginning of line (may have leading whitespace before `#`).
    let src = ctx.source;
    let line_start = src[..dir.comment_start].rfind('\n').map_or(0, |p| p + 1);
    let bytes = src.as_bytes();
    let mut end = dir.comment_start + dir.text.len();
    if end < bytes.len() && bytes[end] == b'\n' {
        end += 1;
    }
    Correction::delete(line_start, end)
}

/// Remove a single cop name from the directive, handling surrounding commas/whitespace.
fn correction_remove_name(dir: &Directive, name: &DirName, ctx: &CheckContext) -> Correction {
    let comment = &dir.text;
    let bytes = comment.as_bytes();
    let n_start = name.col_in_comment;
    let n_end = n_start + name.name.len();

    // Walk left across whitespace
    let mut b = n_start;
    while b > 0 && matches!(bytes[b - 1], b' ' | b'\t') {
        b -= 1;
    }
    // Walk right across whitespace
    let mut e = n_end;
    while e < bytes.len() && matches!(bytes[e], b' ' | b'\t') {
        e += 1;
    }

    if b > 0 && bytes[b - 1] == b',' {
        // comma before: remove `, NAME` including the comma
        let start = dir.comment_start + (b - 1);
        let end = dir.comment_start + n_end;
        Correction::delete(start, end)
    } else if e < bytes.len() && bytes[e] == b',' {
        // comma after: remove `NAME,` plus optional following space
        let mut start_local = n_start;
        let mut end_local = e + 1;
        if end_local < bytes.len() && bytes[end_local] == b' ' {
            end_local += 1;
            // If comma had no surrounding space before NAME, we offset begin to keep formatting;
            // mirrors RuboCop's range_with_comma_after.
        } else {
            // no space after comma: offset start to keep leading space
            start_local = n_start;
        }
        Correction::delete(dir.comment_start + start_local, dir.comment_start + end_local)
    } else {
        // Only name in list — remove whole comment.
        correction_remove_whole_comment(dir, ctx)
    }
}

crate::register_cop!("Lint/RedundantCopEnableDirective", |cfg| {
    let mut disabled: Vec<String> = cfg
        .cops
        .iter()
        .filter(|(_, c)| c.enabled == Some(false))
        .map(|(k, _)| k.clone())
        .collect();
    disabled.sort();
    Some(Box::new(RedundantCopEnableDirective::with_disabled_in_config(disabled)))
});
