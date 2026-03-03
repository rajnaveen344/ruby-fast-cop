//! Autocorrection application logic.
//!
//! Uses a Ruff-style forward-walk algorithm: sort edits ascending by offset,
//! walk forward with a cursor, copy unchanged gaps, apply replacements, skip overlaps.

use crate::offense::Offense;

/// Result of applying corrections, with statistics.
#[derive(Debug)]
pub struct CorrectionResult {
    /// The corrected source code.
    pub output: String,
    /// Number of edits successfully applied.
    pub applied_count: usize,
    /// Number of edits skipped due to overlap.
    pub skipped_count: usize,
}

/// Apply corrections using a forward-walk algorithm (Ruff-style).
///
/// 1. Collect all edits from offenses that have corrections
/// 2. Sort ascending by start_offset (ties broken by end_offset)
/// 3. Walk forward with a cursor, copying unchanged gaps and applying replacements
/// 4. Skip overlapping edits (where start < cursor position)
pub fn apply_corrections_detailed(source: &str, offenses: &[Offense]) -> CorrectionResult {
    let mut edits: Vec<_> = offenses
        .iter()
        .filter_map(|o| o.correction.as_ref())
        .flat_map(|c| c.edits.iter())
        .collect();

    if edits.is_empty() {
        return CorrectionResult {
            output: source.to_string(),
            applied_count: 0,
            skipped_count: 0,
        };
    }

    // Sort ascending by start_offset, then by end_offset for ties
    edits.sort_by(|a, b| {
        a.start_offset
            .cmp(&b.start_offset)
            .then(a.end_offset.cmp(&b.end_offset))
    });

    let source_bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0usize;
    let mut applied_count = 0usize;
    let mut skipped_count = 0usize;

    for edit in &edits {
        let start = edit.start_offset.min(source_bytes.len());
        let end = edit.end_offset.min(source_bytes.len());

        if start < cursor {
            // Overlapping edit — skip it
            skipped_count += 1;
            continue;
        }

        // Copy the unchanged gap between cursor and this edit's start
        if start > cursor {
            output.push_str(&source[cursor..start]);
        }

        // Apply the replacement
        output.push_str(&edit.replacement);
        cursor = end;
        applied_count += 1;
    }

    // Copy any remaining source after the last edit
    if cursor < source_bytes.len() {
        output.push_str(&source[cursor..]);
    }

    CorrectionResult {
        output,
        applied_count,
        skipped_count,
    }
}

/// Apply corrections from offenses to produce corrected source code.
///
/// This is a backward-compatible wrapper around `apply_corrections_detailed()`.
pub fn apply_corrections(source: &str, offenses: &[Offense]) -> String {
    apply_corrections_detailed(source, offenses).output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offense::{Correction, Location, Severity};

    fn make_offense(correction: Correction) -> Offense {
        Offense::new(
            "Test/Cop",
            "test message",
            Severity::Convention,
            Location::new(1, 0, 1, 1),
            "test.rb",
        )
        .with_correction(correction)
    }

    #[test]
    fn no_corrections() {
        let source = "hello world";
        let result = apply_corrections_detailed(source, &[]);
        assert_eq!(result.output, "hello world");
        assert_eq!(result.applied_count, 0);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn single_replace() {
        let source = "foo bar baz";
        let offenses = vec![make_offense(Correction::replace(4, 7, "qux"))];
        let result = apply_corrections_detailed(source, &offenses);
        assert_eq!(result.output, "foo qux baz");
        assert_eq!(result.applied_count, 1);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn multiple_non_overlapping() {
        let source = "aaa bbb ccc";
        let offenses = vec![
            make_offense(Correction::replace(0, 3, "AAA")),
            make_offense(Correction::replace(8, 11, "CCC")),
        ];
        let result = apply_corrections_detailed(source, &offenses);
        assert_eq!(result.output, "AAA bbb CCC");
        assert_eq!(result.applied_count, 2);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn overlapping_edits_skip() {
        let source = "abcdefghij";
        // Two edits that overlap: [2..6) and [4..8)
        let offenses = vec![
            make_offense(Correction::replace(2, 6, "XX")),
            make_offense(Correction::replace(4, 8, "YY")),
        ];
        let result = apply_corrections_detailed(source, &offenses);
        // First edit applied: "ab" + "XX" + cursor at 6
        // Second edit starts at 4 < cursor 6, skipped
        // Remainder from 6: "ghij"
        assert_eq!(result.output, "abXXghij");
        assert_eq!(result.applied_count, 1);
        assert_eq!(result.skipped_count, 1);
    }

    #[test]
    fn insert_edit() {
        let source = "hello world";
        let offenses = vec![make_offense(Correction::insert(5, " beautiful"))];
        let result = apply_corrections_detailed(source, &offenses);
        assert_eq!(result.output, "hello beautiful world");
        assert_eq!(result.applied_count, 1);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn delete_edit() {
        let source = "hello cruel world";
        // Delete " cruel" (bytes 5..11)
        let offenses = vec![make_offense(Correction::delete(5, 11))];
        let result = apply_corrections_detailed(source, &offenses);
        assert_eq!(result.output, "hello world");
        assert_eq!(result.applied_count, 1);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn backward_compat_wrapper() {
        let source = "foo bar";
        let offenses = vec![make_offense(Correction::replace(4, 7, "baz"))];
        let output = apply_corrections(source, &offenses);
        assert_eq!(output, "foo baz");
    }

    #[test]
    fn adjacent_edits_no_overlap() {
        let source = "aabbcc";
        // Two edits that are adjacent: [0..2) and [2..4)
        let offenses = vec![
            make_offense(Correction::replace(0, 2, "AA")),
            make_offense(Correction::replace(2, 4, "BB")),
        ];
        let result = apply_corrections_detailed(source, &offenses);
        assert_eq!(result.output, "AABBcc");
        assert_eq!(result.applied_count, 2);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn edit_at_end_of_source() {
        let source = "hello";
        let offenses = vec![make_offense(Correction::insert(5, "!"))];
        let result = apply_corrections_detailed(source, &offenses);
        assert_eq!(result.output, "hello!");
        assert_eq!(result.applied_count, 1);
        assert_eq!(result.skipped_count, 0);
    }
}
