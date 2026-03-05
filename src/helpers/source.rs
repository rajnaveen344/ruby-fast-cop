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

/// Extract the text of the line containing `offset` (without trailing newline).
pub fn get_line_text<'a>(source: &'a str, offset: usize) -> &'a str {
    let start = line_start_offset(source, offset);
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |p| start + p);
    &source[start..end]
}
