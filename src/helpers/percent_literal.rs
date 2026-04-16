//! Helper for detecting and classifying Ruby percent literals.
//!
//! Mirrors RuboCop's `RuboCop::Cop::PercentLiteral` mixin: extracts the
//! percent-literal "type" (e.g. `%w`, `%W`, `%i`, `%I`, `%q`, `%Q`) from
//! the node's opening delimiter source.

/// Extract the percent-literal type prefix (e.g. `"%w"`, `"%Q"`, `"%"`) from a
/// source slice starting with `%`. Returns `None` if the slice does not start
/// with `%`.
///
/// For `%w(...)` returns `Some("%w")`. For `%(...)` returns `Some("%")`.
/// For `%Q[...]` returns `Some("%Q")`.
pub fn percent_type(source_slice: &str) -> Option<&str> {
    let bytes = source_slice.as_bytes();
    if bytes.is_empty() || bytes[0] != b'%' {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    // `%` (no letter) is the plain string percent literal.
    Some(&source_slice[..i])
}

/// Returns `true` if a slice of source starts with `%` (i.e. is a percent
/// literal based on its opening location source).
pub fn is_percent_literal(source_slice: &str) -> bool {
    source_slice.starts_with('%')
}

/// Return the opening delimiter character for a percent literal source slice,
/// e.g. `(` for `%w(...)`, `[` for `%i[...]`, `/` for `%r/.../`.
/// Returns `None` if the slice is too short.
pub fn opening_delimiter_char(source_slice: &str) -> Option<char> {
    let ty = percent_type(source_slice)?;
    source_slice[ty.len()..].chars().next()
}
