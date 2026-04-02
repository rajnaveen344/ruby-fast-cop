//! Shared source text utilities for computing line/column positions,
//! extracting line text, and finding indentation.

/// Compute 0-indexed column from a byte offset, skipping BOM (U+FEFF).
pub fn col_at_offset(source: &str, offset: usize) -> u32 {
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            col = 0;
        } else if ch != '\u{FEFF}' {
            col += 1;
        }
    }
    col
}

/// Compute 1-indexed line number from a byte offset.
pub fn line_at_offset(source: &str, offset: usize) -> u32 {
    let mut line = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

/// Get the byte offset of the start of the line containing `offset`.
pub fn line_start_offset(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |p| p + 1)
}

/// Get the column of the first non-whitespace character on the line containing `offset`.
pub fn first_non_ws_col(source: &str, offset: usize) -> u32 {
    let line_start = line_start_offset(source, offset);
    let bytes = source.as_bytes();
    let mut col = 0u32;
    let mut i = line_start;
    while i < source.len() && bytes[i] != b'\n' {
        let ch = bytes[i];
        // Skip BOM
        if i + 2 < source.len() && ch == 0xEF && bytes[i + 1] == 0xBB && bytes[i + 2] == 0xBF {
            i += 3;
            continue;
        }
        if ch != b' ' && ch != b'\t' {
            return col;
        }
        col += 1;
        i += 1;
    }
    col
}

/// Get byte offset of the start of a 1-indexed line number.
/// Returns 0 for line 1, and `source.len()` if the line is beyond the file.
pub fn line_byte_offset(source: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut count = 0;
    for (i, &b) in source.as_bytes().iter().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == line - 1 {
                return i + 1;
            }
        }
    }
    source.len()
}

/// Get byte offset of the end of a 1-indexed line (after the `\n`).
/// Returns `source.len()` if the line ends at EOF without a newline.
pub fn line_end_byte_offset(source: &str, line: usize) -> usize {
    let mut count = 0;
    for (i, &b) in source.as_bytes().iter().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == line {
                return i + 1;
            }
        }
    }
    source.len()
}

/// Find the byte position of a `#` comment in a line, skipping `#` inside strings.
/// Uses a basic state machine to track single/double-quoted string context.
/// Returns `None` if no comment is found.
pub fn find_comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut delim = b'"';
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' | b'\'' if !in_string => {
                in_string = true;
                delim = b;
            }
            c if in_string && c == delim => {
                in_string = false;
            }
            b'\\' if in_string => {
                i += 1; // skip escaped char
            }
            b'#' if !in_string => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Check if there is a method chain after the given byte offset in source.
/// Looks for `.method`, `&.method`, or `[...]` after skipping whitespace (including newlines).
/// Mirrors RuboCop's `node.chained?` check.
pub fn is_chained_after(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    if i >= bytes.len() {
        return false;
    }
    match bytes[i] {
        b'.' => true,
        b'&' => i + 1 < bytes.len() && bytes[i + 1] == b'.',
        b'[' => true,
        _ => false,
    }
}

/// Extract the text of the line containing `offset` (without trailing newline).
pub fn get_line_text<'a>(source: &'a str, offset: usize) -> &'a str {
    let start = line_start_offset(source, offset);
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |p| start + p);
    &source[start..end]
}
