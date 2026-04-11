//! Shared helpers for the SurroundingSpace mixin (RuboCop's
//! `lib/rubocop/cop/mixin/surrounding_space.rb`).
//!
//! Unlike RuboCop which operates on a token stream, we work directly
//! on byte offsets of the opening/closing delimiters and scan the
//! source to detect spaces around them.

/// Whether `bytes[offset]` is an ASCII space or tab.
#[inline]
fn is_space_or_tab(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// True if the delimiter at `[left_end..right_start)` is empty (no bytes
/// or only spaces/tabs/newlines). Equivalent to RuboCop's
/// `empty_brackets?` (which checks that no token lies between them).
pub fn is_empty_between(source: &str, left_end: usize, right_start: usize) -> bool {
    if left_end >= right_start {
        return true;
    }
    source.as_bytes()[left_end..right_start]
        .iter()
        .all(|&b| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r')
}

/// True if the range `[left_end..right_start)` contains exactly one ASCII space.
/// Equivalent to `space_between?`.
pub fn has_exactly_one_space(source: &str, left_end: usize, right_start: usize) -> bool {
    right_start == left_end + 1 && source.as_bytes().get(left_end) == Some(&b' ')
}

/// True if left_end == right_start (no bytes in between). Equivalent to
/// `no_character_between?`.
#[inline]
pub fn no_character_between(left_end: usize, right_start: usize) -> bool {
    left_end == right_start
}

/// Count contiguous spaces/tabs immediately after `offset`.
pub fn count_spaces_after(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut n = 0;
    while offset + n < bytes.len() && is_space_or_tab(bytes[offset + n]) {
        n += 1;
    }
    n
}

/// Count contiguous spaces/tabs immediately before `offset`.
pub fn count_spaces_before(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut n = 0;
    while offset > n && is_space_or_tab(bytes[offset - n - 1]) {
        n += 1;
    }
    n
}

/// True if the next non-space/tab byte after `offset` is a newline.
pub fn next_to_newline_after(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i < bytes.len() && is_space_or_tab(bytes[i]) {
        i += 1;
    }
    i >= bytes.len() || bytes[i] == b'\n' || bytes[i] == b'\r'
}

/// True if the previous non-space/tab byte before `offset` is a newline
/// (or start of source). I.e. the `]` is effectively on its own line.
pub fn prev_to_newline_before(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i > 0 && is_space_or_tab(bytes[i - 1]) {
        i -= 1;
    }
    i == 0 || bytes[i - 1] == b'\n' || bytes[i - 1] == b'\r'
}

/// True if the byte right after `offset` is a `#` (start of a comment).
pub fn next_is_comment(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i < bytes.len() && is_space_or_tab(bytes[i]) {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b'#'
}

/// True if two byte offsets lie on the same source line.
#[inline]
pub fn same_line(source: &str, a: usize, b: usize) -> bool {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    !source.as_bytes()[lo..hi].contains(&b'\n')
}
