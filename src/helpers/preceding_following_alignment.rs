//! Shared helper porting RuboCop's `PrecedingFollowingAlignment` mixin.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/preceding_following_alignment.rb
//!
//! Provides `aligned_with_something?` and friends used by
//! `Layout/ExtraSpacing` and `Layout/SpaceBeforeFirstArg` (and eventually
//! other cops that want "allow extra space when vertically aligning with
//! something on a preceding/following line" semantics).
//!
//! A `range` here is `(line_1_indexed, col_start_0_indexed, col_last_0_indexed_exclusive)`.
//! RuboCop's `range.last_column` is the column just after the token (exclusive end).
//! We keep the same convention: `last_column = col_start + token_text.len()`.

/// The source text of a range: `(line_1_indexed, col_start, last_column, text_bytes)`.
/// The text is the exact literal source of the token whose alignment we're
/// checking (e.g. `"name"`, `"="`, `"+="`, `"#"` for a comment).
#[derive(Debug, Clone, Copy)]
pub struct AlignRange<'a> {
    pub line: u32,       // 1-indexed
    pub column: u32,     // 0-indexed start
    pub last_column: u32, // exclusive end column
    pub source: &'a str, // the literal token text
}

impl<'a> AlignRange<'a> {
    #[inline]
    pub fn size(&self) -> usize {
        (self.last_column - self.column) as usize
    }
}

/// Lightweight, line-indexed metadata about the source. Computed once per
/// file, then shared between multiple `aligned_with_something?` calls.
pub struct AlignmentIndex<'a> {
    /// Source split into lines (no trailing `\n`).
    pub lines: Vec<&'a str>,
    /// For each (1-indexed) comment line: the 0-indexed column of `#`.
    /// Keyed by line number.
    pub comments_by_line: std::collections::HashMap<u32, u32>,
    /// Set of 1-indexed lines where the comment begins its line (comment is
    /// the first token). These lines are skipped when scanning adjacent lines
    /// for alignment — matches RuboCop's `aligned_comment_lines` behavior.
    pub aligned_comment_lines: std::collections::HashSet<u32>,
    /// Per-line, the starting 0-indexed column of every `=`-family operator
    /// (`=`, `==`, `===`, `!=`, `<=`, `>=`, `<<`, op-assign like `+=`) plus
    /// its exclusive end column. Used for `aligned_equals_operator?`.
    pub equals_tokens_by_line: std::collections::HashMap<u32, Vec<(u32, u32, String)>>,
}

impl<'a> AlignmentIndex<'a> {
    /// Build an `AlignmentIndex` for the given source. This is a best-effort
    /// single pass that handles `#` comments outside of single/double-quoted
    /// strings and simple regex bodies. Heredocs are NOT currently masked
    /// (RuboCop tokenizer handles this automatically, but we don't need perfect
    /// accuracy here: `aligned_with_something?` is only consulted for alignment
    /// on the specific line pair we care about).
    pub fn build(source: &'a str) -> Self {
        let lines: Vec<&str> = source.split('\n').collect();

        let mut comments_by_line = std::collections::HashMap::new();
        let mut aligned_comment_lines = std::collections::HashSet::new();
        let mut equals_tokens_by_line: std::collections::HashMap<u32, Vec<(u32, u32, String)>> =
            std::collections::HashMap::new();

        for (idx, line) in lines.iter().enumerate() {
            let lineno = (idx + 1) as u32;
            // Find `#` comment start (outside strings).
            if let Some(col) = find_comment_start(line) {
                comments_by_line.insert(lineno, col as u32);
                // `begins_its_line?`: the `#` is at or after only whitespace.
                let prefix = &line[..col];
                if prefix.bytes().all(|b| b == b' ' || b == b'\t') {
                    aligned_comment_lines.insert(lineno);
                }
            }

            // Find equals-family tokens on the line (ignoring strings/comments).
            let eq_tokens = find_equals_tokens(line);
            if !eq_tokens.is_empty() {
                equals_tokens_by_line.insert(lineno, eq_tokens);
            }
        }

        Self { lines, comments_by_line, aligned_comment_lines, equals_tokens_by_line }
    }

    /// Column of the first non-whitespace char on `line_1_indexed`, or `None`
    /// if the line is blank.
    pub fn indent(&self, lineno: u32) -> Option<u32> {
        let line = self.lines.get((lineno - 1) as usize)?;
        line.bytes()
            .position(|b| b != b' ' && b != b'\t')
            .map(|p| p as u32)
    }

    /// Whether `lineno` (1-indexed) is blank.
    pub fn is_blank(&self, lineno: u32) -> bool {
        match self.lines.get((lineno - 1) as usize) {
            Some(line) => line.bytes().all(|b| b == b' ' || b == b'\t'),
            None => true,
        }
    }

    /// Get line text (1-indexed), or empty string if out of bounds.
    pub fn line_text(&self, lineno: u32) -> &'a str {
        self.lines.get((lineno - 1) as usize).copied().unwrap_or("")
    }
}

/// Port of `aligned_with_something?(range)`. Returns true if `range` is
/// vertically aligned with a token on a preceding or following non-blank line.
pub fn aligned_with_something(idx: &AlignmentIndex, range: AlignRange) -> bool {
    aligned_with_adjacent_line(idx, range, aligned_token)
}

/// Port of `aligned_with_operator?(range)`.
pub fn aligned_with_operator(idx: &AlignmentIndex, range: AlignRange) -> bool {
    aligned_with_adjacent_line(idx, range, aligned_operator)
}

type Predicate = fn(&AlignmentIndex, AlignRange, &str, u32) -> bool;

fn aligned_with_adjacent_line(
    idx: &AlignmentIndex,
    range: AlignRange,
    predicate: Predicate,
) -> bool {
    // Try any line in the preceding range (range.line-1 .. 1, descending) and
    // following range (range.line+1 .. last_line, ascending), without any
    // indent filter.
    let total_lines = idx.lines.len() as u32;
    let pre: Vec<u32> = (1..range.line).rev().collect();
    let post: Vec<u32> = ((range.line + 1)..=total_lines).collect();

    if aligned_with_any_line(idx, &pre, range, None, predicate)
        || aligned_with_any_line(idx, &post, range, None, predicate)
    {
        return true;
    }

    // Fallback: restrict to lines with the same base indentation as the
    // checked line.
    let base_indent = idx.indent(range.line);
    if base_indent.is_none() {
        return false;
    }
    aligned_with_any_line(idx, &pre, range, base_indent, predicate)
        || aligned_with_any_line(idx, &post, range, base_indent, predicate)
}

fn aligned_with_any_line(
    idx: &AlignmentIndex,
    line_nos: &[u32],
    range: AlignRange,
    indent: Option<u32>,
    predicate: Predicate,
) -> bool {
    for &lineno in line_nos {
        if idx.aligned_comment_lines.contains(&lineno) {
            continue;
        }
        let line = idx.line_text(lineno);
        let Some(first_non_ws) = line.bytes().position(|b| b != b' ' && b != b'\t') else {
            continue; // blank line: skip
        };
        if let Some(req) = indent {
            if req as usize != first_non_ws {
                continue;
            }
        }
        return predicate(idx, range, line, lineno);
    }
    false
}

fn aligned_token(idx: &AlignmentIndex, range: AlignRange, line: &str, lineno: u32) -> bool {
    aligned_words(range, line) || aligned_equals_operator(idx, range, lineno)
}

fn aligned_operator(idx: &AlignmentIndex, range: AlignRange, line: &str, lineno: u32) -> bool {
    aligned_identical(range, line) || aligned_equals_operator(idx, range, lineno)
}

fn aligned_words(range: AlignRange, line: &str) -> bool {
    // RuboCop: `/\s\S/.match?(line[left_edge - 1, 2])` OR
    //          `token == line[left_edge, token.length]`.
    let left = range.column as usize;
    let bytes = line.as_bytes();

    if left >= 1 && left < bytes.len() {
        let a = bytes[left - 1];
        let b = bytes[left];
        let is_ws = |c: u8| c == b' ' || c == b'\t';
        if is_ws(a) && !is_ws(b) {
            return true;
        }
    }

    let tok = range.source;
    if left + tok.len() <= bytes.len() && &line[left..left + tok.len()] == tok {
        return true;
    }
    false
}

fn aligned_identical(range: AlignRange, line: &str) -> bool {
    let left = range.column as usize;
    let size = range.size();
    left + size <= line.len() && &line[left..left + size] == range.source
}

fn aligned_equals_operator(idx: &AlignmentIndex, range: AlignRange, lineno: u32) -> bool {
    let Some(tokens) = idx.equals_tokens_by_line.get(&lineno) else {
        return false;
    };
    let Some(first) = tokens.first() else {
        return false;
    };
    let (tok_start, tok_end, tok_str) = (first.0, first.1, first.2.as_str());

    // aligned_with_preceding_equals: range ends with `=` and last_column matches token end column.
    let ends_in_eq = range.source.as_bytes().last() == Some(&b'=');
    if ends_in_eq && range.last_column == tok_end {
        return true;
    }
    // aligned_with_append_operator:
    //   (range == "<<" && token.equal_sign?) ||
    //   (range ends in "=" && token == "<<") matched on last_column.
    let range_is_shift = range.source == "<<";
    let token_is_shift = tok_str == "<<";
    let token_has_eq = tok_str.as_bytes().last() == Some(&b'=') || tok_str == "<<";
    if (range_is_shift && token_has_eq) || (ends_in_eq && token_is_shift) {
        if range.last_column == tok_end {
            return true;
        }
    }
    let _ = tok_start; // unused but documented
    false
}

// ── Simple line-level tokenizers for `#` comments and `=`-family tokens ──

/// Find column of `#` that starts a trailing comment on the given line (outside
/// of strings/regex). Handles single quotes, double quotes (no interpolation
/// depth tracking needed for this purpose), and simple regex literals.
/// Returns `None` if none found.
pub fn find_comment_start(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    let mut i = 0;
    let mut in_sq = false;
    let mut in_dq = false;
    let mut in_re = false;
    while i < b.len() {
        let c = b[i];
        if in_sq {
            if c == b'\\' && i + 1 < b.len() {
                i += 2;
                continue;
            }
            if c == b'\'' {
                in_sq = false;
            }
        } else if in_dq {
            if c == b'\\' && i + 1 < b.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_dq = false;
            }
        } else if in_re {
            if c == b'\\' && i + 1 < b.len() {
                i += 2;
                continue;
            }
            if c == b'/' {
                in_re = false;
            }
        } else {
            match c {
                b'#' => return Some(i),
                b'\'' => in_sq = true,
                b'"' => in_dq = true,
                b'/' => {
                    // Heuristic: treat `/` as a regex start only if the previous
                    // non-space char suggests an operand context. We won't be
                    // perfect — false-negatives on comment detection are only
                    // a problem if the comment sits after a regex literal; that
                    // edge case doesn't appear in the fixture set.
                    if looks_like_regex_start(b, i) {
                        in_re = true;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn looks_like_regex_start(bytes: &[u8], i: usize) -> bool {
    // Walk backwards over spaces: if prev non-ws is alphanumeric or `)`/`]`,
    // treat `/` as division; else treat as regex.
    let mut j = i;
    while j > 0 {
        j -= 1;
        let c = bytes[j];
        if c == b' ' || c == b'\t' {
            continue;
        }
        return matches!(
            c,
            b'(' | b',' | b'=' | b'!' | b'&' | b'|' | b':' | b';' | b'{' | b'?' | b'<' | b'>'
        );
    }
    true
}

/// Find `=`-family tokens on a line (outside strings/comments): `=`, `==`,
/// `===`, `!=`, `<=`, `>=`, `<<`, `||=`, `&&=`, `+=`, `-=`, `*=`, `/=`, `%=`,
/// `**=`, `<<=`, `>>=`, `|=`, `&=`, `^=`.
fn find_equals_tokens(line: &str) -> Vec<(u32, u32, String)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_sq = false;
    let mut in_dq = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_sq {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'\'' {
                in_sq = false;
            }
            i += 1;
            continue;
        }
        if in_dq {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_dq = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => {
                in_sq = true;
                i += 1;
            }
            b'"' => {
                in_dq = true;
                i += 1;
            }
            b'#' => break,
            _ => {
                // Try to match an equals-family token starting here.
                if let Some(tok) = match_equals_token(bytes, i) {
                    let start = i as u32;
                    let end = (i + tok.len()) as u32;
                    out.push((start, end, tok.to_string()));
                    i += tok.len();
                    continue;
                }
                i += 1;
            }
        }
    }
    out
}

fn match_equals_token(bytes: &[u8], i: usize) -> Option<&'static str> {
    // Order: longest first.
    const CANDIDATES: &[&str] = &[
        "<<=", ">>=", "**=", "===", "||=", "&&=",
        "==", "!=", "<=", ">=", "<<", "+=", "-=", "*=", "/=", "%=", "|=", "&=", "^=", "=",
    ];
    for tok in CANDIDATES {
        let tb = tok.as_bytes();
        if bytes.len() - i < tb.len() {
            continue;
        }
        if &bytes[i..i + tb.len()] != tb {
            continue;
        }
        // For single-char `=`, avoid misreading part of `==`, `<=`, etc. — we
        // only accept `=` if it's NOT part of a longer token (the for-loop
        // already tried longer candidates first, so if we reach `=` it's a
        // standalone `=`). We also must not treat `=` as a comparison when
        // it's actually the `!` prefix consumed — but we ensured `!=` matches
        // earlier. Filter `=` when followed by `=` or preceded by `<`/`>`/`!`:
        if *tok == "=" {
            let next_is_eq = i + 1 < bytes.len() && bytes[i + 1] == b'=';
            let prev = if i > 0 { Some(bytes[i - 1]) } else { None };
            if next_is_eq {
                continue;
            }
            if matches!(prev, Some(b'<' | b'>' | b'!' | b'+' | b'-' | b'*' | b'/' | b'%' | b'|' | b'&' | b'^')) {
                continue;
            }
        }
        return Some(tok);
    }
    None
}
