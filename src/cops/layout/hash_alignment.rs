//! Layout/HashAlignment - Checks alignment of hash keys, separators, and values.
//!
//! Translated from RuboCop's Layout/HashAlignment cop + HashAlignmentStyles mixin.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

// ── Configuration enums ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlignmentStyle {
    Key,
    Separator,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastArgumentHashStyle {
    AlwaysInspect,
    AlwaysIgnore,
    IgnoreImplicit,
    IgnoreExplicit,
}

// ── Messages ──

const MSG_KEY: &str =
    "Align the keys of a hash literal if they span more than one line.";
const MSG_SEPARATOR: &str =
    "Align the separators of a hash literal if they span more than one line.";
const MSG_TABLE: &str =
    "Align the keys and values of a hash literal if they span more than one line.";
const MSG_KWSPLAT: &str =
    "Align keyword splats with the rest of the hash if it spans more than one line.";

fn message_for(style: AlignmentStyle) -> &'static str {
    match style {
        AlignmentStyle::Key => MSG_KEY,
        AlignmentStyle::Separator => MSG_SEPARATOR,
        AlignmentStyle::Table => MSG_TABLE,
    }
}

// ── Cop struct ──

pub struct HashAlignment {
    rocket_styles: Vec<AlignmentStyle>,
    colon_styles: Vec<AlignmentStyle>,
    last_arg_style: LastArgumentHashStyle,
    /// When Layout/ArgumentAlignment uses "with_fixed_indentation", skip keyword
    /// hashes that are method-call arguments (alignment is handled by that cop).
    argument_alignment_fixed: bool,
}

impl HashAlignment {
    pub fn new(
        rocket_styles: Vec<AlignmentStyle>,
        colon_styles: Vec<AlignmentStyle>,
        last_arg_style: LastArgumentHashStyle,
    ) -> Self {
        Self {
            rocket_styles,
            colon_styles,
            last_arg_style,
            argument_alignment_fixed: false,
        }
    }

    pub fn with_argument_alignment_fixed(mut self, fixed: bool) -> Self {
        self.argument_alignment_fixed = fixed;
        self
    }
}

impl Cop for HashAlignment {
    fn name(&self) -> &'static str {
        "Layout/HashAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = HashAlignmentVisitor {
            ctx,
            rocket_styles: &self.rocket_styles,
            colon_styles: &self.colon_styles,
            last_arg_style: self.last_arg_style,
            argument_alignment_fixed: self.argument_alignment_fixed,
            offenses: Vec::new(),
            ignored_hashes: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

// ── Visitor ──

struct HashAlignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    rocket_styles: &'a [AlignmentStyle],
    colon_styles: &'a [AlignmentStyle],
    last_arg_style: LastArgumentHashStyle,
    argument_alignment_fixed: bool,
    offenses: Vec<Offense>,
    ignored_hashes: Vec<usize>,
}

/// Info about a single pair/splat element in a hash.
/// Key length follows RuboCop convention: for colon pairs, excludes trailing `:`.
#[derive(Debug)]
struct PairInfo {
    node_start: usize,
    node_end: usize,
    /// Byte offset of key end (for computing separator ws range)
    key_end_offset: usize,
    key_col: usize,
    /// RuboCop-compatible key length (excludes trailing colon for symbol keys)
    key_len: usize,
    is_rocket: bool,
    is_kwsplat: bool,
    operator_col: Option<usize>,
    operator_end_col: Option<usize>,
    /// Byte offset of operator start (for rocket ws editing)
    operator_start_offset: Option<usize>,
    /// Byte offset of operator end (for value ws editing)
    operator_end_offset: Option<usize>,
    value_col: Option<usize>,
    /// Byte offset of value start
    value_start_offset: Option<usize>,
    value_on_new_line: bool,
    value_omission: bool,
    begins_line: bool,
    /// End offset limited to first line (for offense range)
    first_line_end: usize,
}

impl<'a> HashAlignmentVisitor<'a> {
    fn check_hash_elements(&mut self, elements: &[Node]) {
        let pairs = self.collect_pair_infos(elements);
        if pairs.is_empty() {
            return;
        }

        // Skip single-line hashes
        let first_line = self.ctx.line_of(pairs[0].node_start);
        let last_line = self.ctx.line_of(pairs.last().unwrap().node_end);
        if first_line == last_line {
            return;
        }

        // Need at least one non-kwsplat pair
        let first_pair_idx = match pairs.iter().position(|p| !p.is_kwsplat) {
            Some(i) => i,
            None => return,
        };

        // Determine hash-level properties
        let has_rocket = pairs.iter().any(|p| !p.is_kwsplat && p.is_rocket);
        let has_colon = pairs.iter().any(|p| !p.is_kwsplat && !p.is_rocket);
        let mixed_delimiters = has_rocket && has_colon;
        let pairs_on_same_line = self.has_pairs_on_same_line(&pairs);
        let value_alignment_checkable = !mixed_delimiters && !pairs_on_same_line;

        // Guard: at least one alignment per separator type must be checkable
        // (KeyAlignment is always checkable; Table/Separator need value_alignment_checkable)
        let is_checkable = |style: &AlignmentStyle| -> bool {
            *style == AlignmentStyle::Key || value_alignment_checkable
        };
        if has_rocket && !self.rocket_styles.iter().any(is_checkable) {
            return;
        }
        if has_colon && !self.colon_styles.iter().any(is_checkable) {
            return;
        }

        self.check_pairs_alignment(&pairs, first_pair_idx);
    }

    fn has_pairs_on_same_line(&self, pairs: &[PairInfo]) -> bool {
        // RuboCop's same_line? checks if last_line of pair A == first_line of pair B
        let non_kwsplat: Vec<usize> = pairs.iter()
            .enumerate()
            .filter(|(_, p)| !p.is_kwsplat)
            .map(|(i, _)| i)
            .collect();
        for w in non_kwsplat.windows(2) {
            let end_line = self.ctx.line_of(pairs[w[0]].node_end.saturating_sub(1));
            let start_line = self.ctx.line_of(pairs[w[1]].node_start);
            if end_line == start_line {
                return true;
            }
        }
        false
    }

    fn collect_pair_infos(&self, elements: &[Node]) -> Vec<PairInfo> {
        let mut infos = Vec::new();
        for elem in elements {
            if let Some(assoc) = elem.as_assoc_node() {
                let key = assoc.key();
                let value = assoc.value();
                let key_start = key.location().start_offset();
                let key_end = key.location().end_offset();
                let node_end = assoc.location().end_offset();
                let key_col = self.ctx.col_of(key_start);

                let is_rocket = if let Some(op_loc) = assoc.operator_loc() {
                    self.ctx.src(op_loc.start_offset(), op_loc.end_offset()) == "=>"
                } else {
                    false
                };

                let value_start = value.location().start_offset();
                let value_end = value.location().end_offset();
                let value_omission = value_start == key_start && value_end == key_end;

                // Prism includes trailing colon in symbol keys; RuboCop does not
                let prism_key_len = key_end - key_start;
                let key_len = if is_rocket { prism_key_len } else { prism_key_len.saturating_sub(1) };

                let (operator_col, operator_end_col, operator_start_offset, operator_end_offset) =
                    if let Some(op_loc) = assoc.operator_loc() {
                        (
                            Some(self.ctx.col_of(op_loc.start_offset())),
                            Some(self.ctx.col_of(op_loc.end_offset())),
                            Some(op_loc.start_offset()),
                            Some(op_loc.end_offset()),
                        )
                    } else {
                        // Colon style: colon is at key_end - 1
                        let colon_col = self.ctx.col_of(key_end - 1);
                        (Some(colon_col), Some(colon_col + 1), None, None)
                    };

                let value_col = if value_omission { None } else { Some(self.ctx.col_of(value_start)) };
                let value_start_offset = if value_omission { None } else { Some(value_start) };
                let value_on_new_line = !value_omission
                    && self.ctx.line_of(key_start) != self.ctx.line_of(value_start);

                // Limit offense range to first line of pair (for multi-line nodes)
                let first_line_end = self.ctx.source[key_start..].find('\n')
                    .map_or(node_end, |p| key_start + p)
                    .min(node_end);

                infos.push(PairInfo {
                    node_start: key_start, node_end, key_end_offset: key_end,
                    key_col, key_len, is_rocket,
                    is_kwsplat: false, operator_col, operator_end_col,
                    operator_start_offset, operator_end_offset,
                    value_col, value_start_offset,
                    value_on_new_line, value_omission,
                    begins_line: self.ctx.begins_its_line(key_start),
                    first_line_end,
                });
            } else if let Some(splat) = elem.as_assoc_splat_node() {
                let start = splat.location().start_offset();
                let end = splat.location().end_offset();
                infos.push(PairInfo {
                    node_start: start, node_end: end, key_end_offset: end,
                    key_col: self.ctx.col_of(start), key_len: 0,
                    is_rocket: false, is_kwsplat: true,
                    operator_col: None, operator_end_col: None,
                    operator_start_offset: None, operator_end_offset: None,
                    value_col: None, value_start_offset: None,
                    value_on_new_line: false, value_omission: false,
                    begins_line: self.ctx.begins_its_line(start),
                    first_line_end: end,
                });
            } else if matches!(elem, Node::ForwardingArgumentsNode { .. }) {
                let start = elem.location().start_offset();
                let end = elem.location().end_offset();
                infos.push(PairInfo {
                    node_start: start, node_end: end, key_end_offset: end,
                    key_col: self.ctx.col_of(start), key_len: 0,
                    is_rocket: false, is_kwsplat: true,
                    operator_col: None, operator_end_col: None,
                    operator_start_offset: None, operator_end_offset: None,
                    value_col: None, value_start_offset: None,
                    value_on_new_line: false, value_omission: false,
                    begins_line: self.ctx.begins_its_line(start),
                    first_line_end: end,
                });
            }
        }
        infos
    }

    fn check_pairs_alignment(&mut self, pairs: &[PairInfo], first_pair_idx: usize) {
        let first_pair = &pairs[first_pair_idx];

        // Compute hash-level metrics for table alignment
        let all_non_kwsplat: Vec<&PairInfo> = pairs.iter().filter(|p| !p.is_kwsplat).collect();
        let max_key_width = all_non_kwsplat.iter().map(|p| p.key_len).max().unwrap_or(0);
        let max_delimiter_width = all_non_kwsplat.iter().map(|p| {
            if p.is_rocket { 4 } else { 2 } // " => " or ": "
        }).max().unwrap_or(2);

        let rocket_styles = self.rocket_styles.to_vec();
        let colon_styles = self.colon_styles.to_vec();

        // Map (style, pair_idx) → key_delta. Mirrors RuboCop's column_deltas:
        // tracks per-style/per-pair delta for any pair that had a non-good_alignment
        // delta under that style. Used at registration time to build corrections
        // using the FIRST configured style's delta (not the winning style's).
        let mut deltas_by: HashMap<(AlignmentStyle, usize), i64> = HashMap::new();
        let mut offending_idx_by: HashMap<AlignmentStyle, Vec<usize>> = HashMap::new();
        let mut kwsplat_offenses: Vec<Offense> = Vec::new();

        let styles_for = |pair: &PairInfo| -> &[AlignmentStyle] {
            if pair.is_rocket { &rocket_styles } else { &colon_styles }
        };

        // Initialize all style buckets so styles with 0 offenses are still candidates
        for &style in styles_for(first_pair) {
            offending_idx_by.entry(style).or_default();
        }
        for pair in pairs.iter() {
            if !pair.is_kwsplat {
                for &style in styles_for(pair) {
                    offending_idx_by.entry(style).or_default();
                }
            }
        }

        // Check first pair (only separator/value spacing, key is reference)
        for &style in styles_for(first_pair) {
            let delta = self.first_pair_deltas(first_pair, style, max_key_width, max_delimiter_width);
            if !all_zero(&delta) {
                deltas_by.insert((style, first_pair_idx), 0);
                offending_idx_by.entry(style).or_default().push(first_pair_idx);
            }
        }

        // Check all children
        for (idx, pair) in pairs.iter().enumerate() {
            if idx == first_pair_idx {
                continue;
            }
            if pair.is_kwsplat {
                if pair.begins_line {
                    let delta = first_pair.key_col as i64 - pair.key_col as i64;
                    if delta != 0 {
                        kwsplat_offenses.push(
                            self.make_offense_corrected(MSG_KWSPLAT, pair, first_pair.key_col),
                        );
                    }
                }
                continue;
            }
            for &style in styles_for(pair) {
                let delta = self.pair_deltas(first_pair, pair, style, max_key_width, max_delimiter_width);
                if !all_zero(&delta) {
                    deltas_by.insert((style, idx), delta.key);
                    offending_idx_by.entry(style).or_default().push(idx);
                }
            }
        }

        // Register kwsplat offenses (always reported)
        self.offenses.extend(kwsplat_offenses);

        // Pick alignment style with fewest offenses (config order tie-break).
        let style_order: Vec<AlignmentStyle> = {
            let mut order = Vec::new();
            for &s in styles_for(first_pair) {
                if !order.contains(&s) { order.push(s); }
            }
            for pair in pairs.iter() {
                if !pair.is_kwsplat {
                    for &s in styles_for(pair) {
                        if !order.contains(&s) { order.push(s); }
                    }
                }
            }
            order
        };
        let mut sorted_styles: Vec<(AlignmentStyle, Vec<usize>)> = offending_idx_by.into_iter().collect();
        sorted_styles.sort_by_key(|(style, idxs)| {
            let order_idx = style_order.iter().position(|s| s == style).unwrap_or(usize::MAX);
            (idxs.len(), order_idx)
        });

        let Some((winning_style, offending_idxs)) = sorted_styles.into_iter().next() else {
            return;
        };

        // Build offenses with the winning style's MESSAGE but the FIRST configured
        // style's correction (mirrors RuboCop's `column_deltas[alignment_for(o).first.class]`).
        for idx in offending_idxs {
            let pair = &pairs[idx];
            let msg = message_for(winning_style);
            let first_style = styles_for(pair)[0];
            let correction_delta = deltas_by.get(&(first_style, idx)).copied();
            let offense = self.make_offense(msg, pair);
            let offense = match correction_delta {
                Some(key_delta) => {
                    match self.build_pair_correction(
                        pair, first_pair, first_style, key_delta,
                        max_key_width, max_delimiter_width,
                    ) {
                        Some(c) => offense.with_correction(c),
                        None => offense,
                    }
                }
                None => offense,
            };
            self.offenses.push(offense);
        }
    }

    fn make_offense(&self, msg: &str, pair: &PairInfo) -> Offense {
        self.ctx.offense_with_range(
            "Layout/HashAlignment", msg, Severity::Convention,
            pair.node_start, pair.first_line_end,
        )
    }

    /// Build correction edits for a pair.
    /// `first_pair` = the reference pair for alignment.
    /// `style` = the chosen alignment style.
    /// Edits in descending offset order.
    fn build_pair_correction(
        &self,
        pair: &PairInfo,
        first_pair: &PairInfo,
        style: AlignmentStyle,
        key_delta: i64,
        max_key_width: usize,
        max_delimiter_width: usize,
    ) -> Option<Correction> {
        let mut edits: Vec<crate::offense::Edit> = Vec::new();

        // 1. Key indent edit
        if key_delta != 0 && pair.begins_line {
            let line_start = self.ctx.line_start(pair.node_start);
            let new_col = (pair.key_col as i64 + key_delta).max(0) as usize;
            edits.push(crate::offense::Edit {
                start_offset: line_start,
                end_offset: pair.node_start,
                replacement: " ".repeat(new_col),
            });
        }

        // 2. Separator gap edit (rocket pairs only)
        if pair.is_rocket {
            if let Some(op_start) = pair.operator_start_offset {
                let key_end = pair.key_end_offset;
                let current_gap = (op_start - key_end) as i64;
                // After key moves, new_key_end_col = key_col + key_delta + key_len
                let new_key_end_col = pair.key_col as i64 + key_delta + pair.key_len as i64;
                let new_op_col = match style {
                    AlignmentStyle::Key => {
                        // 1 space between key and `=>`
                        new_key_end_col + 1
                    }
                    AlignmentStyle::Table => {
                        // separator at first_key_col + max_key_width + 1
                        first_pair.key_col as i64 + max_key_width as i64 + 1
                    }
                    AlignmentStyle::Separator => {
                        // separator aligns to first pair's separator column
                        first_pair.operator_col.unwrap_or(0) as i64
                    }
                };
                let new_gap = (new_op_col - new_key_end_col).max(1);
                if new_gap != current_gap {
                    edits.push(crate::offense::Edit {
                        start_offset: key_end,
                        end_offset: op_start,
                        replacement: " ".repeat(new_gap as usize),
                    });
                }
            }
        }

        // 3. Value gap edit (same-line values only)
        if !pair.value_on_new_line && !pair.value_omission {
            let (after_op_off, after_op_col_fn): (Option<usize>, Box<dyn Fn() -> i64>) =
                if pair.is_rocket {
                    // new after_op col = new_op_start_col + op_len(2)
                    let new_key_end_col = pair.key_col as i64 + key_delta + pair.key_len as i64;
                    let new_op_start = match style {
                        AlignmentStyle::Key => new_key_end_col + 1,
                        AlignmentStyle::Table => {
                            // Same formula as step 2: op at first_key_col + max_key_width + 1
                            first_pair.key_col as i64 + max_key_width as i64 + 1
                        }
                        AlignmentStyle::Separator => first_pair.operator_col.unwrap_or(0) as i64,
                    };
                    let new_after_op = new_op_start + 2; // `=>` is 2 chars
                    (pair.operator_end_offset, Box::new(move || new_after_op))
                } else {
                    // Colon pair: after_op = key_end (colon is part of key)
                    // new after_op col = key_col + key_delta + prism_key_len_including_colon
                    // key_end_offset is after the colon. key_len = prism_key_len - 1 (colon excluded).
                    // So prism_key_len_including_colon = key_len + 1.
                    let new_after_op = pair.key_col as i64 + key_delta + pair.key_len as i64 + 1;
                    (Some(pair.key_end_offset), Box::new(move || new_after_op))
                };

            if let (Some(after_op), Some(val_start)) = (after_op_off, pair.value_start_offset) {
                if after_op <= val_start {
                    let current_gap = (val_start - after_op) as i64;
                    let new_after_op = after_op_col_fn();
                    let new_val_col = match style {
                        AlignmentStyle::Key => {
                            // 1 space after separator
                            new_after_op + 1
                        }
                        AlignmentStyle::Table => {
                            // value at first_key_col + max_key_width + max_delimiter_width
                            first_pair.key_col as i64 + max_key_width as i64 + max_delimiter_width as i64
                        }
                        AlignmentStyle::Separator => {
                            // value aligns to first pair's value column
                            first_pair.value_col.unwrap_or(0) as i64
                        }
                    };
                    let new_gap = (new_val_col - new_after_op).max(1);
                    if new_gap != current_gap {
                        edits.push(crate::offense::Edit {
                            start_offset: after_op,
                            end_offset: val_start,
                            replacement: " ".repeat(new_gap as usize),
                        });
                    }
                }
            }
        }

        if edits.is_empty() { return None; }
        edits.sort_by(|a, b| b.start_offset.cmp(&a.start_offset));
        Some(Correction { edits })
    }

    /// Make offense with correction using precomputed deltas.
    fn make_offense_with_deltas(
        &self, msg: &str, pair: &PairInfo, first_pair: &PairInfo,
        style: AlignmentStyle, key_delta: i64,
        max_key_width: usize, max_delimiter_width: usize,
    ) -> Offense {
        let offense = self.make_offense(msg, pair);
        match self.build_pair_correction(pair, first_pair, style, key_delta, max_key_width, max_delimiter_width) {
            Some(c) => offense.with_correction(c),
            None => offense,
        }
    }

    /// Make offense with correction for kwsplat (key indent only).
    fn make_offense_corrected(&self, msg: &str, pair: &PairInfo, expected_col: usize) -> Offense {
        let offense = self.make_offense(msg, pair);
        if !pair.begins_line { return offense; }
        let line_start = self.ctx.line_start(pair.node_start);
        let correction = Correction::replace(line_start, pair.node_start, " ".repeat(expected_col));
        offense.with_correction(correction)
    }

    // ── Delta computation ──

    fn first_pair_deltas(
        &self, pair: &PairInfo, style: AlignmentStyle,
        max_key_width: usize, max_delimiter_width: usize,
    ) -> Deltas {
        match style {
            AlignmentStyle::Key => {
                Deltas {
                    key: 0,
                    separator: self.key_separator_delta(pair),
                    value: self.key_value_delta(pair),
                }
            }
            AlignmentStyle::Table => {
                let sep_delta = self.table_separator_delta_for(pair.key_col, pair, max_key_width, 0);
                let val_delta = self.table_value_delta_for(pair.key_col, pair, max_key_width, max_delimiter_width) - sep_delta;
                Deltas { key: 0, separator: sep_delta, value: val_delta }
            }
            AlignmentStyle::Separator => {
                Deltas { key: 0, separator: 0, value: 0 }
            }
        }
    }

    fn pair_deltas(
        &self, first: &PairInfo, current: &PairInfo, style: AlignmentStyle,
        max_key_width: usize, max_delimiter_width: usize,
    ) -> Deltas {
        match style {
            AlignmentStyle::Key => {
                if !current.begins_line {
                    return Deltas { key: 0, separator: 0, value: 0 };
                }
                Deltas {
                    key: first.key_col as i64 - current.key_col as i64,
                    separator: self.key_separator_delta(current),
                    value: self.key_value_delta(current),
                }
            }
            AlignmentStyle::Table => {
                let key_delta = first.key_col as i64 - current.key_col as i64;
                let sep_delta = self.table_separator_delta_for(first.key_col, current, max_key_width, key_delta);
                let val_delta = self.table_value_delta_for(first.key_col, current, max_key_width, max_delimiter_width) - key_delta - sep_delta;
                Deltas { key: key_delta, separator: sep_delta, value: val_delta }
            }
            AlignmentStyle::Separator => {
                let key_delta = (first.key_col + first.key_len) as i64 - (current.key_col + current.key_len) as i64;
                let sep_delta = self.separator_sep_delta(first, current) - key_delta;
                let val_delta = self.separator_value_delta(first, current) - key_delta - sep_delta;
                Deltas { key: key_delta, separator: sep_delta, value: val_delta }
            }
        }
    }

    // ── Key alignment: keys left-aligned, single space around separators ──

    fn key_separator_delta(&self, pair: &PairInfo) -> i64 {
        if pair.is_rocket {
            if let Some(op_col) = pair.operator_col {
                let correct = pair.key_col + pair.key_len + 1;
                return correct as i64 - op_col as i64;
            }
        }
        0
    }

    fn key_value_delta(&self, pair: &PairInfo) -> i64 {
        if pair.value_on_new_line || pair.value_omission { return 0; }
        if let (Some(op_end), Some(val_col)) = (pair.operator_end_col, pair.value_col) {
            return (op_end + 1) as i64 - val_col as i64;
        }
        0
    }

    // ── Table alignment: keys left-aligned, separators/values column-aligned ──

    fn table_separator_delta_for(
        &self, first_key_col: usize, current: &PairInfo, max_key_width: usize, key_delta: i64,
    ) -> i64 {
        if current.is_rocket {
            if let Some(op_col) = current.operator_col {
                let correct = first_key_col + max_key_width + 1;
                return correct as i64 - op_col as i64 - key_delta;
            }
        }
        0
    }

    fn table_value_delta_for(
        &self, first_key_col: usize, current: &PairInfo,
        max_key_width: usize, max_delimiter_width: usize,
    ) -> i64 {
        if current.value_omission { return 0; }
        if let Some(val_col) = current.value_col {
            let correct = first_key_col + max_key_width + max_delimiter_width;
            return correct as i64 - val_col as i64;
        }
        0
    }

    // ── Separator alignment: separators column-aligned, keys right-aligned ──

    fn separator_sep_delta(&self, first: &PairInfo, current: &PairInfo) -> i64 {
        if current.is_rocket {
            if let (Some(f_op), Some(c_op)) = (first.operator_col, current.operator_col) {
                return f_op as i64 - c_op as i64;
            }
        }
        0
    }

    fn separator_value_delta(&self, first: &PairInfo, current: &PairInfo) -> i64 {
        if current.value_omission { return 0; }
        if let (Some(f_val), Some(c_val)) = (first.value_col, current.value_col) {
            return f_val as i64 - c_val as i64;
        }
        0
    }

    // ── Last-argument hash handling ──

    fn process_call_arguments(&mut self, args: &[Node]) {
        if args.is_empty() { return; }
        let last_arg = &args[args.len() - 1];

        if let Some(hash) = last_arg.as_hash_node() {
            let should_ignore = match self.last_arg_style {
                LastArgumentHashStyle::AlwaysInspect => false,
                LastArgumentHashStyle::AlwaysIgnore => true,
                LastArgumentHashStyle::IgnoreExplicit => true,
                LastArgumentHashStyle::IgnoreImplicit => false,
            };
            if should_ignore {
                self.ignored_hashes.push(hash.location().start_offset());
            }
        } else if let Some(kwh) = last_arg.as_keyword_hash_node() {
            // When ArgumentAlignment uses with_fixed_indentation, ignore keyword
            // hashes whose first element is on the same line as a preceding
            // positional argument (i.e. inline kwargs following other args).
            // This prevents conflicts between HashAlignment and ArgumentAlignment.
            if self.argument_alignment_fixed && args.len() > 1 {
                let left_sibling = &args[args.len() - 2];
                let sib_end_line = self.ctx.line_of(left_sibling.location().end_offset().saturating_sub(1));
                let kwh_start_line = self.ctx.line_of(kwh.location().start_offset());
                if sib_end_line == kwh_start_line {
                    self.ignored_hashes.push(kwh.location().start_offset());
                    return;
                }
            }
            // When ArgumentAlignment uses with_fixed_indentation and there are
            // no positional args, ignore if the first kwarg is on the call line.
            if self.argument_alignment_fixed && args.len() == 1 {
                // The kwh IS the only argument. Check if it starts on the same
                // line as the method call (looking at the call node's message location).
                // We don't have access to the call node here, but we can check if
                // the kwh starts on the same line as content before it on that line
                // (i.e., not on a new indented line). We approximate by checking
                // if kwh doesn't begin its line.
                let kwh_start = kwh.location().start_offset();
                if !self.ctx.begins_its_line(kwh_start) {
                    self.ignored_hashes.push(kwh_start);
                    return;
                }
            }
            let should_ignore = match self.last_arg_style {
                LastArgumentHashStyle::AlwaysInspect => false,
                LastArgumentHashStyle::AlwaysIgnore => true,
                LastArgumentHashStyle::IgnoreImplicit => true,
                LastArgumentHashStyle::IgnoreExplicit => false,
            };
            if should_ignore {
                self.ignored_hashes.push(kwh.location().start_offset());
                return;
            }
            // If the first element of the keyword hash doesn't begin its line
            // (e.g., preceded by a positional arg or follows the call on the same line),
            // and there's a left sibling argument on the same line, skip it.
            // This mirrors RuboCop's autocorrect_incompatible_with_other_cops? check.
            if args.len() > 1 {
                let left_sibling = &args[args.len() - 2];
                let sib_end_line = self.ctx.line_of(left_sibling.location().end_offset().saturating_sub(1));
                let kwh_start_line = self.ctx.line_of(kwh.location().start_offset());
                if sib_end_line == kwh_start_line {
                    self.ignored_hashes.push(kwh.location().start_offset());
                }
            }
        }
    }
}

#[derive(Debug)]
struct Deltas {
    key: i64,
    separator: i64,
    value: i64,
}

fn all_zero(d: &Deltas) -> bool {
    d.key == 0 && d.separator == 0 && d.value == 0
}

impl Visit<'_> for HashAlignmentVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            self.process_call_arguments(&arg_list);
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            self.process_call_arguments(&arg_list);
        }
        ruby_prism::visit_super_node(self, node);
    }

    fn visit_yield_node(&mut self, node: &ruby_prism::YieldNode) {
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            self.process_call_arguments(&arg_list);
        }
        ruby_prism::visit_yield_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        let start = node.location().start_offset();
        if !self.ignored_hashes.contains(&start) {
            let elements: Vec<_> = node.elements().iter().collect();
            if !elements.is_empty() {
                self.check_hash_elements(&elements);
            }
        }
        ruby_prism::visit_hash_node(self, node);
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        let start = node.location().start_offset();
        if !self.ignored_hashes.contains(&start) {
            let elements: Vec<_> = node.elements().iter().collect();
            if !elements.is_empty() {
                self.check_hash_elements(&elements);
            }
        }
        ruby_prism::visit_keyword_hash_node(self, node);
    }
}

crate::register_cop!("Layout/HashAlignment", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/HashAlignment");

    let parse_styles = |key: &str, default: &str| -> Vec<AlignmentStyle> {
        let raw = cop_config.and_then(|c| c.raw.get(key));
        let strings: Vec<String> = if let Some(val) = raw {
            if let Some(s) = val.as_str() {
                vec![s.to_string()]
            } else if let Some(seq) = val.as_sequence() {
                seq.iter().filter_map(|v| v.as_str().map(String::from)).collect()
            } else {
                vec![default.to_string()]
            }
        } else {
            vec![default.to_string()]
        };
        strings.iter().filter_map(|s| match s.as_str() {
            "key" => Some(AlignmentStyle::Key),
            "separator" => Some(AlignmentStyle::Separator),
            "table" => Some(AlignmentStyle::Table),
            _ => None,
        }).collect()
    };

    let rocket_styles = parse_styles("EnforcedHashRocketStyle", "key");
    let colon_styles = parse_styles("EnforcedColonStyle", "key");

    let last_arg_style = cop_config
        .and_then(|c| c.raw.get("EnforcedLastArgumentHashStyle"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "always_ignore" => LastArgumentHashStyle::AlwaysIgnore,
            "ignore_implicit" => LastArgumentHashStyle::IgnoreImplicit,
            "ignore_explicit" => LastArgumentHashStyle::IgnoreExplicit,
            _ => LastArgumentHashStyle::AlwaysInspect,
        })
        .unwrap_or(LastArgumentHashStyle::AlwaysInspect);

    if rocket_styles.is_empty() || colon_styles.is_empty() {
        return None;
    }

    let arg_align_config = cfg.get_cop_config("Layout/ArgumentAlignment");
    let arg_align_fixed = arg_align_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| s == "with_fixed_indentation")
        .unwrap_or(false);

    Some(Box::new(HashAlignment::new(
        rocket_styles,
        colon_styles,
        last_arg_style,
    ).with_argument_alignment_fixed(arg_align_fixed)))
});
