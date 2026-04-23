//! Layout/EmptyLines - Checks for consecutive empty lines (more than one).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

#[derive(Default)]
pub struct EmptyLines;

impl EmptyLines {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EmptyLines {
    fn name(&self) -> &'static str {
        "Layout/EmptyLines"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;

        // Quick check: need at least \n\n\n to have consecutive blank lines
        if !source.contains("\n\n\n") {
            return vec![];
        }

        // Collect which lines contain tokens (non-blank, non-comment content).
        // Lines that are purely whitespace are "empty" from RuboCop's perspective.
        // Comments count as tokens too (they prevent blank-line violation).
        //
        // RuboCop collects token lines then finds gaps > 1 between consecutive token lines.
        // Within each gap, it flags line pairs (line_n, line_n+1) that are both empty.
        //
        // Additionally, strings/heredocs with blank lines inside must be skipped.

        let lines: Vec<&str> = source.lines().collect();
        let n = lines.len();

        // Determine which lines are inside string literals or heredocs
        // Strategy: parse byte ranges of string content using a simple scanner
        let inside_string = build_inside_string_mask(source, &lines);

        // Determine which lines have "tokens" (non-empty content that's not purely inside a string)
        // A line has a token if it's not blank, OR if it has a comment.
        // Actually RuboCop uses processed_source.tokens to collect token line numbers.
        // We approximate: a line has a token if stripped is non-empty AND not fully inside a string literal.
        //
        // For our purposes: a line is "empty" for the gap check if it is blank (only whitespace).
        // Comment-only lines are NOT empty (they contain tokens in RuboCop's model).

        let mut token_lines: Vec<usize> = Vec::new(); // 0-based line indices with tokens

        for (i, line) in lines.iter().enumerate() {
            if inside_string[i] {
                // Lines fully inside a string: treated as having content (not empty for gap calc)
                // Actually RuboCop skips them because they're inside tokens.
                // We need to include them as "occupied" to not flag them.
                token_lines.push(i);
                continue;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                token_lines.push(i);
            }
        }

        if token_lines.len() < 2 {
            return vec![];
        }

        let mut offenses = Vec::new();

        // For each pair of consecutive token lines, check if gap > 1
        for window in token_lines.windows(2) {
            let prev_token_line = window[0];
            let curr_token_line = window[1];
            let gap = curr_token_line - prev_token_line; // number of lines between them (exclusive)

            if gap < 2 {
                continue; // 0 or 1 blank line — fine
            }

            // Flag the extra blank lines: lines at indices prev_token_line+2 .. curr_token_line
            // RuboCop flags using `previous_and_current_lines_empty?` which walks the gap
            // and flags each line where both it and its predecessor are empty.
            // Effectively: flag lines at prev_token_line+2 through curr_token_line-1 (0-based)
            // i.e., the second and subsequent blank lines in each run.
            for blank_line_idx in (prev_token_line + 2)..curr_token_line {
                // Both blank_line_idx and blank_line_idx-1 must be blank
                let line_a = lines[blank_line_idx - 1].trim();
                let line_b = lines[blank_line_idx].trim();
                if line_a.is_empty() && line_b.is_empty() {
                    // Compute byte offset for this line
                    let line_num = (blank_line_idx + 1) as u32;
                    // byte offset of this line start
                    let line_start: usize = lines[..blank_line_idx].iter().map(|l| l.len() + 1).sum();
                    let line_end = line_start + lines[blank_line_idx].len() + 1;

                    let correction = Correction::delete(line_start, line_end);
                    offenses.push(Offense::new(
                        self.name(),
                        "Extra blank line detected.",
                        Severity::Convention,
                        Location::new(line_num, 0, line_num, 1),
                        ctx.filename,
                    ).with_correction(correction));
                }
            }
        }

        offenses
    }
}

/// Returns a bool per line: true if the line is fully inside a multi-line string literal or heredoc.
/// We use a simple heuristic: track double/single quoted multi-line strings and heredocs.
fn build_inside_string_mask(source: &str, lines: &[&str]) -> Vec<bool> {
    let n = lines.len();
    let mut mask = vec![false; n];

    // Build per-line byte ranges
    let mut line_starts = Vec::with_capacity(n);
    let mut off = 0usize;
    for line in lines {
        line_starts.push(off);
        off += line.len() + 1; // +1 for '\n'
    }

    // Find multi-line strings: any line fully enclosed between start/end of a string.
    // Strategy: scan source for string boundaries.
    // This is approximate — good enough to handle the fixture cases.

    // Detect heredoc body lines
    // Heredoc: <<-DELIM or <<~DELIM or <<DELIM on a line, body follows, ends at DELIM line.
    let mut in_heredoc = false;
    let mut heredoc_delim = String::new();
    let mut heredoc_squiggly = false;

    // Also detect multi-line string literals (lines between opening/closing quote).
    // We track which lines are inside by scanning byte ranges of StringNode etc.
    // For simplicity, detect runs of lines inside double-quoted strings via quote counting.

    // Pass: detect heredoc body lines
    for (i, line) in lines.iter().enumerate() {
        if in_heredoc {
            let trimmed = if heredoc_squiggly {
                line.trim_start()
            } else {
                line.trim_end()
            };
            if trimmed == heredoc_delim {
                in_heredoc = false;
                // The delimiter line itself is NOT inside the heredoc body
            } else {
                mask[i] = true;
            }
            continue;
        }

        // Check for heredoc opener on this line
        // Look for <<~DELIM or <<-DELIM or <<DELIM
        if let Some((delim, squiggly)) = find_heredoc_opener(line) {
            in_heredoc = true;
            heredoc_delim = delim;
            heredoc_squiggly = squiggly;
        }
    }

    // Pass: detect multi-line double-quoted strings
    // Simple: count whether we're between unescaped " that appear at end of a line
    // and opening " that appeared at end of a previous line.
    // This is a rough heuristic.
    let bytes = source.as_bytes();
    let mut in_str = false;
    let mut str_start_line: usize = 0;
    let mut j = 0usize;

    fn line_of(pos: usize, line_starts: &[usize]) -> usize {
        line_starts.partition_point(|&s| s <= pos).saturating_sub(1)
    }

    while j < bytes.len() {
        match bytes[j] {
            b'\\' if in_str => {
                j += 2;
                continue;
            }
            b'"' if !in_str => {
                in_str = true;
                str_start_line = line_of(j, &line_starts);
            }
            b'"' if in_str => {
                let end_line = line_of(j, &line_starts);
                // Mark all lines strictly between str_start_line and end_line as inside string
                if end_line > str_start_line + 1 {
                    for li in (str_start_line + 1)..end_line {
                        mask[li] = true;
                    }
                }
                in_str = false;
            }
            b'#' if !in_str => {
                // Skip to end of line (comment)
                while j < bytes.len() && bytes[j] != b'\n' {
                    j += 1;
                }
                continue;
            }
            _ => {}
        }
        j += 1;
    }

    mask
}

fn find_heredoc_opener(line: &str) -> Option<(String, bool)> {
    // Look for <<~, <<-, or << followed by identifier or quoted string
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] == b'<' {
            let mut k = i + 2;
            let squiggly;
            if k < bytes.len() && bytes[k] == b'~' {
                squiggly = true;
                k += 1;
            } else if k < bytes.len() && bytes[k] == b'-' {
                squiggly = false;
                k += 1;
            } else {
                squiggly = false;
            }
            // Skip optional quote
            let quote = if k < bytes.len() && (bytes[k] == b'\'' || bytes[k] == b'"' || bytes[k] == b'`') {
                let q = bytes[k];
                k += 1;
                Some(q)
            } else {
                None
            };
            // Collect identifier
            let start = k;
            while k < bytes.len() && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_') {
                k += 1;
            }
            if k > start {
                let delim = std::str::from_utf8(&bytes[start..k]).unwrap_or("").to_string();
                // Skip closing quote
                if let Some(q) = quote {
                    if k < bytes.len() && bytes[k] == q {
                        // ok
                    }
                }
                return Some((delim, squiggly));
            }
        }
        i += 1;
    }
    None
}

crate::register_cop!("Layout/EmptyLines", |_cfg| {
    Some(Box::new(EmptyLines::new()))
});
