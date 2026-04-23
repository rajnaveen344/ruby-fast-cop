//! Migration/DepartmentName cop
//! Checks that cop names in rubocop:enable/disable/todo comments include the department.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/migration/department_name.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
// Known cop name → department mappings for legacy cop names
const LEGACY_COPS: &[(&str, &str)] = &[
    ("SingleSpaceBeforeFirstArg", "Style"),
    ("Alias", "Style"),
    ("AlignArray", "Layout"),
    ("AlignHash", "Layout"),
    ("AlignParameters", "Layout"),
    ("BlockComments", "Style"),
    ("LineLength", "Layout"),
    ("EndOfLine", "Layout"),
    ("Tab", "Layout"),
    ("TrailingWhitespace", "Layout"),
    ("EmptyLines", "Layout"),
    ("CommentIndentation", "Layout"),
];

#[derive(Default)]
pub struct DepartmentName;

impl DepartmentName {
    pub fn new() -> Self { Self }
}

impl Cop for DepartmentName {
    fn name(&self) -> &'static str { "Migration/DepartmentName" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let mut offenses = Vec::new();

        for (line_idx, line) in source.lines().enumerate() {
            // Look for rubocop:disable/enable/todo comments
            // Pattern: # rubocop[spaces]:[spaces](disable|enable|todo)[spaces]CopName, ...
            if let Some(rubocop_pos) = find_rubocop_directive(line) {
                let directive_start = rubocop_pos;
                let after_directive = &line[directive_start..];

                // Find the action (disable/enable/todo)
                let action_start = after_directive.find(':').map(|i| directive_start + i + 1);
                if action_start.is_none() { continue; }
                let action_start = action_start.unwrap();

                // Skip spaces after colon
                let rest = &line[action_start..];
                let spaces = rest.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                let after_colon = action_start + spaces;
                let action_text = &line[after_colon..];

                // Check for valid actions
                let action_end = action_text
                    .find(|c: char| !c.is_alphabetic())
                    .map(|i| after_colon + i)
                    .unwrap_or(line.len());
                let action = &line[after_colon..action_end];
                if !matches!(action, "disable" | "enable" | "todo") {
                    continue;
                }

                // Now parse cop names after the action
                let cops_str = &line[action_end..];
                // Find where cop names start (skip spaces)
                let cops_offset = action_end + cops_str.chars().take_while(|c| *c == ' ').count();
                let cops_text = cops_str.trim_start();
                if cops_text.is_empty() { continue; }

                // Calculate absolute line start once
                let line_abs_start: usize = source.lines()
                    .take(line_idx)
                    .map(|l| l.len() + 1)
                    .sum();

                // Split by comma and check each cop name
                let mut char_pos = cops_offset;
                for part in cops_text.split(',') {
                    let leading_space = part.len() - part.trim_start().len();
                    let trimmed = part.trim();

                    if trimmed.is_empty() {
                        char_pos += part.len() + 1;
                        continue;
                    }

                    // Stop if part contains unexpected characters (like -- or # in rest)
                    if contain_unexpected_char_for_dept(trimmed) {
                        break;
                    }

                    // Skip if it's a valid token: has /, is "all", is a department name,
                    // or contains non-word chars (like "Style:Alias" with colon)
                    if !is_missing_department(trimmed) {
                        char_pos += part.len() + 1;
                        continue;
                    }

                    // This cop name is missing department - emit offense
                    let cop_start_in_line = char_pos + leading_space;
                    let cop_end_in_line = cop_start_in_line + trimmed.len();

                    let cop_abs_start = line_abs_start + cop_start_in_line;
                    let cop_abs_end = line_abs_start + cop_end_in_line;

                    // Check if there's a department we can suggest
                    let correction = if let Some(&(_, dept)) = LEGACY_COPS.iter().find(|(n, _)| *n == trimmed) {
                        let qualified = format!("{}/{}", dept, trimmed);
                        Some(Correction::replace(cop_abs_start, cop_abs_end, qualified))
                    } else {
                        None
                    };

                    let msg = "Department name is missing.";
                    let offense = ctx.offense_with_range(
                        "Migration/DepartmentName", msg, Severity::Convention,
                        cop_abs_start,
                        cop_abs_end,
                    );
                    let offense = if let Some(corr) = correction {
                        offense.with_correction(corr)
                    } else {
                        offense
                    };
                    offenses.push(offense);

                    char_pos += part.len() + 1; // +1 for comma
                }
            }
        }

        offenses
    }
}

/// Find position of "rubocop" in a comment directive, handling spaces around ":"
/// Returns byte offset within the line of "rubocop" if found
fn find_rubocop_directive(line: &str) -> Option<usize> {
    // Find # character
    let hash_pos = line.find('#')?;
    let after_hash = &line[hash_pos + 1..];
    // Skip spaces
    let spaces = after_hash.chars().take_while(|c| *c == ' ' || *c == '\t').count();
    let keyword_start = hash_pos + 1 + spaces;
    let keyword = &line[keyword_start..];

    // Check if it starts with "rubocop"
    if keyword.starts_with("rubocop") {
        let after_rubocop = &keyword[7..]; // "rubocop" is 7 chars
        // Must have optional spaces then ':'
        let colon_pos = after_rubocop.find(':');
        if colon_pos.is_some() {
            return Some(hash_pos);
        }
    }
    None
}

/// Known department names in RuboCop
const DEPARTMENT_NAMES: &[&str] = &[
    "Lint", "Style", "Layout", "Metrics", "Naming", "Bundler",
    "Gemspec", "Security", "Migration", "InternalAffairs", "Performance",
    "Rails", "RSpec",
];

/// Check if a cop token name represents a missing department (should be flagged)
/// Returns true if the name looks like a bare cop name without a department
fn is_missing_department(name: &str) -> bool {
    // Has '/' → department present, no offense
    if name.contains('/') { return false; }
    // Is "all" → no offense
    if name == "all" { return false; }
    // Contains non-word chars (not A-Za-z0-9_) → unusual format, no offense
    // (e.g. "Style:Alias" with colon, or "--" trailer)
    if name.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_') { return false; }
    // Is a known department name → no offense (lone department is valid)
    if DEPARTMENT_NAMES.contains(&name) { return false; }
    // Must start with uppercase to look like a cop name
    if !name.starts_with(|c: char| c.is_ascii_uppercase()) { return false; }
    // Looks like a bare cop name → flag it
    true
}

/// Detect unexpected characters that should stop the scan (like "--" in comments)
fn contain_unexpected_char_for_dept(name: &str) -> bool {
    // If name contains chars not in [A-Za-z/, ] it signals end of cop list
    name.chars().any(|c| !c.is_ascii_alphabetic() && c != '/' && c != ' ' && c != '_')
}

crate::register_cop!("Migration/DepartmentName", |_cfg| Some(Box::new(DepartmentName::new())));
