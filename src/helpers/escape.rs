//! Shared escape and delimiter utilities for string/regexp escape cops.

/// Get the matching closing delimiter for a bracket-type opener.
pub fn closing_delimiter(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '<' => Some('>'),
        _ => None,
    }
}

/// Delimiter pair for a string or regexp literal.
#[derive(Debug, Clone, Copy)]
pub struct Delimiters {
    pub open: char,
    pub close: char,
}

impl Delimiters {
    /// Determine delimiters from a percent-literal opening like `%r{`, `%Q[`, `%w(`.
    /// Falls back to the given default (e.g., `'/'` for regexp, `'"'` for strings).
    pub fn from_opening(opening: &str, prefix: &str, default: char) -> Self {
        if opening.starts_with(prefix) && opening.len() >= prefix.len() + 1 {
            let delim_char = opening.as_bytes()[prefix.len()] as char;
            if let Some(c) = closing_delimiter(delim_char) {
                Delimiters { open: delim_char, close: c }
            } else {
                Delimiters { open: delim_char, close: delim_char }
            }
        } else {
            Delimiters { open: default, close: default }
        }
    }
}

/// Skip past a `#{...}` interpolation block starting at `i` (pointing at `#`).
/// Returns the byte position after the closing `}`.
/// `content_end` is the exclusive upper bound for scanning.
pub fn skip_interpolation(bytes: &[u8], i: usize, content_end: usize) -> usize {
    debug_assert!(bytes[i] == b'#' && i + 1 < content_end && bytes[i + 1] == b'{');
    let mut depth = 1usize;
    let mut j = i + 2;
    while j < content_end && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'\\' if j + 1 < content_end => { j += 1; }
            b'"' => {
                j += 1;
                while j < content_end && bytes[j] != b'"' {
                    if bytes[j] == b'\\' && j + 1 < content_end { j += 1; }
                    j += 1;
                }
            }
            b'\'' => {
                j += 1;
                while j < content_end && bytes[j] != b'\'' {
                    if bytes[j] == b'\\' && j + 1 < content_end { j += 1; }
                    j += 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    j
}

/// Check if position `i` starts a `#{` interpolation block.
pub fn is_interpolation_start(bytes: &[u8], i: usize, end: usize) -> bool {
    bytes[i] == b'#' && i + 1 < end && bytes[i + 1] == b'{'
}
