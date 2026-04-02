//! Shared helpers for Metrics/*Length cops (BlockLength, ClassLength, MethodLength).
//!
//! Mirrors RuboCop's `CodeLength` mixin — line counting, comment filtering,
//! array/hash folding, and bracket matching.

/// Convert a byte offset to a 0-indexed line number.
pub fn line_number_at(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
}

/// Find the byte offset of the end of the line containing `start`.
pub fn find_end_of_first_line(source: &str, start: usize) -> usize {
    source
        .as_bytes()
        .iter()
        .skip(start)
        .position(|&b| b == b'\n')
        .map_or(source.len(), |p| start + p)
}

/// Find the line index where a bracket opened at `start` is closed.
pub fn find_closing_bracket(
    lines: &[&str],
    start: usize,
    end: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 0;
    for i in start..end {
        if let Some(line) = lines.get(i) {
            for ch in line.chars() {
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
            }
        }
    }
    None
}

/// Count non-blank, non-comment body lines between `body_start..body_end` (0-indexed line numbers).
///
/// - `count_comments`: if true, comment lines count toward the total
/// - `count_as_one`: fold multi-line constructs (e.g. "array", "hash") into 1 line
/// - `excluded`: ranges `(start_line, end_line)` to skip (e.g. nested class/module bodies)
pub fn count_body_lines(
    lines: &[&str],
    body_start: usize,
    body_end: usize,
    count_comments: bool,
    count_as_one: &[String],
    excluded: &[(usize, usize)],
) -> usize {
    if count_as_one.is_empty() {
        count_simple(lines, body_start, body_end, count_comments, excluded)
    } else {
        count_with_folds(lines, body_start, body_end, count_comments, count_as_one, excluded)
    }
}

fn count_simple(
    lines: &[&str],
    body_start: usize,
    body_end: usize,
    count_comments: bool,
    excluded: &[(usize, usize)],
) -> usize {
    (body_start..body_end)
        .filter(|&i| {
            if excluded.iter().any(|&(s, e)| i >= s && i < e) {
                return false;
            }
            lines.get(i).map_or(false, |line| {
                let t = line.trim();
                !t.is_empty() && (count_comments || !t.starts_with('#'))
            })
        })
        .count()
}

fn count_with_folds(
    lines: &[&str],
    body_start: usize,
    body_end: usize,
    count_comments: bool,
    count_as_one: &[String],
    excluded: &[(usize, usize)],
) -> usize {
    let mut count = 0;
    let mut i = body_start;
    while i < body_end {
        if let Some(&(_, end)) = excluded.iter().find(|&&(s, e)| i >= s && i < e) {
            i = end;
            continue;
        }
        let trimmed = match lines.get(i) {
            Some(l) => l.trim(),
            None => break,
        };
        if trimmed.is_empty() || (!count_comments && trimmed.starts_with('#')) {
            i += 1;
            continue;
        }
        let mut folded = false;
        for &(tag, open, close) in &[("array", '[', ']'), ("hash", '{', '}')] {
            if count_as_one.iter().any(|s| s == tag) && trimmed.contains(open) {
                if let Some(end_idx) = find_closing_bracket(lines, i, body_end, open, close) {
                    count += 1;
                    i = end_idx + 1;
                    folded = true;
                    break;
                }
            }
        }
        if folded {
            continue;
        }
        count += 1;
        i += 1;
    }
    count
}
