//! Shared helpers for `Layout/Multiline*LineBreaks` and `Layout/First*LineBreak` cops.
//!
//! Ports two RuboCop mixins:
//! - `MultilineElementLineBreaks` — flag children sharing a line with another child
//! - `FirstElementLineBreak` — flag first child not on its own line after the opener
//!
//! Both mixins emit corrections that insert `\n` before the offending child.

use crate::cops::CheckContext;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

/// 1-indexed line number at byte offset.
#[inline]
pub fn line_of(src: &str, offset: usize) -> usize {
    1 + src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// True if all `children` start and end on the same line.
/// When `ignore_last`, only compares first vs last `first_line` (multiline final allowed).
pub fn all_on_same_line(src: &str, children: &[Node], ignore_last: bool) -> bool {
    if children.is_empty() {
        return true;
    }
    let first = children.first().unwrap().location();
    let last = children.last().unwrap().location();
    let first_line = line_of(src, first.start_offset());
    if ignore_last {
        let last_first_line = line_of(src, last.start_offset());
        first_line == last_first_line
    } else {
        let last_last_line = line_of(src, last.end_offset().saturating_sub(1));
        first_line == last_last_line
    }
}

/// Port of RuboCop's `MultilineElementLineBreaks#check_line_breaks`.
///
/// For each child whose first_line ≤ the previous child's last_line,
/// emit an offense (and a correction inserting `\n` before the child).
pub fn check_multiline_breaks(
    ctx: &CheckContext,
    cop_name: &'static str,
    msg: &'static str,
    children: &[Node],
    ignore_last: bool,
) -> Vec<Offense> {
    if all_on_same_line(ctx.source, children, ignore_last) {
        return vec![];
    }

    // Pass 1 (RuboCop semantics): mark children sharing a line with a previous child.
    // This produces the offense set RuboCop's `expect_offense` would capture.
    let mut offended_idx: Vec<usize> = Vec::new();
    let mut last_seen_line: i64 = -1;
    for (i, child) in children.iter().enumerate() {
        let cloc = child.location();
        let fl = line_of(ctx.source, cloc.start_offset()) as i64;
        let ll = line_of(ctx.source, cloc.end_offset().saturating_sub(1)) as i64;
        if last_seen_line >= fl {
            offended_idx.push(i);
        } else {
            last_seen_line = ll;
        }
    }
    if offended_idx.is_empty() {
        return vec![];
    }

    // Pass 2 (fixed-point): every position needing a `\n` post-iterative-correction.
    // Each prior inserted `\n` shifts subsequent children down by one line; track via
    // `inserted`. Offending child's post-correction span recomputed from its original
    // height.
    let mut all_break_idx: Vec<usize> = Vec::new();
    let mut last_seen_line: i64 = -1;
    let mut inserted: i64 = 0;
    for (i, child) in children.iter().enumerate() {
        let cloc = child.location();
        let orig_fl = line_of(ctx.source, cloc.start_offset()) as i64;
        let orig_ll = line_of(ctx.source, cloc.end_offset().saturating_sub(1)) as i64;
        let adj_fl = orig_fl + inserted;
        let adj_ll = orig_ll + inserted;
        if last_seen_line >= adj_fl {
            all_break_idx.push(i);
            let new_fl = last_seen_line + 1;
            last_seen_line = new_fl + (orig_ll - orig_fl);
            inserted += 1;
        } else {
            last_seen_line = adj_ll;
        }
    }

    // Build offenses: one per RuboCop-spec offense, each with a `\n` insert.
    // Bundle any extra fixed-point inserts (in `all_break_idx` but not `offended_idx`)
    // into the last offense's Correction so apply_corrections produces the final source.
    let extra_idx: Vec<usize> = all_break_idx
        .iter()
        .copied()
        .filter(|i| !offended_idx.contains(i))
        .collect();

    let mut offenses: Vec<Offense> = offended_idx
        .iter()
        .map(|&i| {
            let cloc = children[i].location();
            let off = ctx.offense_with_range(
                cop_name,
                msg,
                Severity::Convention,
                cloc.start_offset(),
                cloc.end_offset(),
            );
            off.with_correction(Correction::insert(cloc.start_offset(), "\n"))
        })
        .collect();

    if !extra_idx.is_empty() {
        let mut edits: Vec<Edit> = extra_idx
            .iter()
            .map(|&i| Edit {
                start_offset: children[i].location().start_offset(),
                end_offset: children[i].location().start_offset(),
                replacement: "\n".to_string(),
            })
            .collect();
        // Attach to last offense; preserve its existing edit too.
        if let Some(last) = offenses.last_mut() {
            if let Some(corr) = last.correction.as_mut() {
                corr.edits.append(&mut edits);
            } else {
                last.correction = Some(Correction { edits });
            }
        }
    }

    offenses
}

/// Port of RuboCop's `FirstElementLineBreak#check_children_line_break`.
///
/// `start_offset` is the byte offset to compare against (typically the
/// container start — array `[`, hash `{`, call name, def name).
/// Emits an offense on the lexically-first child if it shares a line with
/// `start_offset` AND another child is on a later line.
///
/// `ignore_last` (`AllowMultilineFinalElement`): when true, the "later line"
/// check uses `first_line` of children rather than `last_line`, allowing
/// the final element to span multiple lines.
pub fn check_first_element_break(
    ctx: &CheckContext,
    cop_name: &'static str,
    msg: &'static str,
    start_offset: usize,
    children: &[Node],
    ignore_last: bool,
) -> Vec<Offense> {
    if children.is_empty() {
        return vec![];
    }

    let start_line = line_of(ctx.source, start_offset);

    // Find min by first_line — the lexically first child; ties broken by source order.
    let min = children
        .iter()
        .min_by_key(|c| line_of(ctx.source, c.location().start_offset()))
        .unwrap();
    let min_loc = min.location();
    let min_first_line = line_of(ctx.source, min_loc.start_offset());
    if start_line != min_first_line {
        return vec![];
    }

    let max_line = children
        .iter()
        .map(|c| {
            let l = c.location();
            if ignore_last {
                line_of(ctx.source, l.start_offset())
            } else {
                line_of(ctx.source, l.end_offset().saturating_sub(1))
            }
        })
        .max()
        .unwrap();
    if start_line == max_line {
        return vec![];
    }

    let mut off = ctx.offense_with_range(
        cop_name,
        msg,
        Severity::Convention,
        min_loc.start_offset(),
        min_loc.end_offset(),
    );
    off = off.with_correction(Correction::insert(min_loc.start_offset(), "\n"));
    vec![off]
}

/// `method_uses_parens?` from RuboCop's `FirstElementLineBreak`.
///
/// Returns true if the line up to `limit_col` (column of the first child)
/// matches `\s*\(\s*$` — i.e. there's an opening paren followed by only
/// whitespace at line end. Handles `foo(`, `super(`, `def foo(`.
pub fn method_uses_parens(src: &str, container_start: usize, limit_offset: usize) -> bool {
    // Need source line containing container_start, then chars up to col of limit_offset.
    let line_start = src[..container_start].rfind('\n').map_or(0, |p| p + 1);
    let line_end = src[container_start..]
        .find('\n')
        .map_or(src.len(), |p| container_start + p);
    let limit_col = limit_offset.saturating_sub(line_start);
    let line = &src[line_start..line_end];
    if limit_col > line.len() {
        return false;
    }
    let prefix = &line[..limit_col];
    // Match /\s*\(\s*$/ — strip trailing whitespace, last non-space must be `(`.
    let trimmed = prefix.trim_end();
    trimmed.ends_with('(')
}

/// `assignment_on_same_line?` from `FirstArrayElementLineBreak`.
///
/// True if the line containing `node_start` ends with `\s*=\s*$` up to the
/// node's column. Used to detect masgn / send implicit-array RHS.
pub fn assignment_on_same_line(src: &str, node_start: usize) -> bool {
    let line_start = src[..node_start].rfind('\n').map_or(0, |p| p + 1);
    let prefix = &src[line_start..node_start];
    // Match /\s*=\s*$/ — last non-whitespace char must be `=`.
    let trimmed = prefix.trim_end();
    trimmed.ends_with('=')
}
