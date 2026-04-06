//! Layout/HashAlignment - Checks alignment of hash keys, separators, and values.
//!
//! Translated from RuboCop's Layout/HashAlignment cop + HashAlignmentStyles mixin.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
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
        }
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
    offenses: Vec<Offense>,
    ignored_hashes: Vec<usize>,
}

/// Info about a single pair/splat element in a hash.
/// Key length follows RuboCop convention: for colon pairs, excludes trailing `:`.
#[derive(Debug)]
struct PairInfo {
    node_start: usize,
    node_end: usize,
    key_col: usize,
    /// RuboCop-compatible key length (excludes trailing colon for symbol keys)
    key_len: usize,
    is_rocket: bool,
    is_kwsplat: bool,
    operator_col: Option<usize>,
    operator_end_col: Option<usize>,
    value_col: Option<usize>,
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

                let (operator_col, operator_end_col) = if let Some(op_loc) = assoc.operator_loc() {
                    (
                        Some(self.ctx.col_of(op_loc.start_offset())),
                        Some(self.ctx.col_of(op_loc.end_offset())),
                    )
                } else {
                    // Colon style: colon is at key_end - 1
                    let colon_col = self.ctx.col_of(key_end - 1);
                    (Some(colon_col), Some(colon_col + 1))
                };

                let value_col = if value_omission { None } else { Some(self.ctx.col_of(value_start)) };
                let value_on_new_line = !value_omission
                    && self.ctx.line_of(key_start) != self.ctx.line_of(value_start);

                // Limit offense range to first line of pair (for multi-line nodes)
                let first_line_end = self.ctx.source[key_start..].find('\n')
                    .map_or(node_end, |p| key_start + p)
                    .min(node_end);

                infos.push(PairInfo {
                    node_start: key_start, node_end, key_col, key_len, is_rocket,
                    is_kwsplat: false, operator_col, operator_end_col, value_col,
                    value_on_new_line, value_omission,
                    begins_line: self.ctx.begins_its_line(key_start),
                    first_line_end,
                });
            } else if let Some(splat) = elem.as_assoc_splat_node() {
                let start = splat.location().start_offset();
                let end = splat.location().end_offset();
                infos.push(PairInfo {
                    node_start: start, node_end: end,
                    key_col: self.ctx.col_of(start), key_len: 0,
                    is_rocket: false, is_kwsplat: true,
                    operator_col: None, operator_end_col: None, value_col: None,
                    value_on_new_line: false, value_omission: false,
                    begins_line: self.ctx.begins_its_line(start),
                    first_line_end: end,
                });
            } else if matches!(elem, Node::ForwardingArgumentsNode { .. }) {
                let start = elem.location().start_offset();
                let end = elem.location().end_offset();
                infos.push(PairInfo {
                    node_start: start, node_end: end,
                    key_col: self.ctx.col_of(start), key_len: 0,
                    is_rocket: false, is_kwsplat: true,
                    operator_col: None, operator_end_col: None, value_col: None,
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

        // Bucket offenses by alignment style (mirrors RuboCop's offenses_by)
        let mut offenses_by: HashMap<AlignmentStyle, Vec<Offense>> = HashMap::new();
        let mut kwsplat_offenses: Vec<Offense> = Vec::new();

        let styles_for = |pair: &PairInfo| -> &[AlignmentStyle] {
            if pair.is_rocket { &rocket_styles } else { &colon_styles }
        };

        // Initialize all style buckets so styles with 0 offenses are still candidates
        for &style in styles_for(first_pair) {
            offenses_by.entry(style).or_default();
        }
        for pair in pairs.iter() {
            if !pair.is_kwsplat {
                for &style in styles_for(pair) {
                    offenses_by.entry(style).or_default();
                }
            }
        }

        // Check first pair (only separator/value spacing, key is reference)
        for &style in styles_for(first_pair) {
            let delta = self.first_pair_deltas(first_pair, style, max_key_width, max_delimiter_width);
            if !all_zero(&delta) {
                offenses_by.entry(style).or_default().push(
                    self.make_offense(message_for(style), first_pair),
                );
            }
        }

        // Check all children
        for pair in pairs.iter() {
            if std::ptr::eq(pair, first_pair) {
                continue;
            }
            if pair.is_kwsplat {
                if pair.begins_line {
                    let delta = first_pair.key_col as i64 - pair.key_col as i64;
                    if delta != 0 {
                        kwsplat_offenses.push(self.make_offense(MSG_KWSPLAT, pair));
                    }
                }
                continue;
            }
            for &style in styles_for(pair) {
                let delta = self.pair_deltas(first_pair, pair, style, max_key_width, max_delimiter_width);
                if !all_zero(&delta) {
                    offenses_by.entry(style).or_default().push(
                        self.make_offense(message_for(style), pair),
                    );
                }
            }
        }

        // Register kwsplat offenses (always reported)
        self.offenses.extend(kwsplat_offenses);

        // Pick alignment style with fewest offenses.
        // On tie, prefer the first style encountered (mirrors Ruby hash insertion order).
        // RuboCop iterates styles per-pair, so the first style configured comes first.
        let mut sorted_styles: Vec<(AlignmentStyle, Vec<Offense>)> = offenses_by.into_iter().collect();
        // Stable sort: styles with fewer offenses come first.
        // On tie, maintain original config order by using style_order index.
        let style_order: Vec<AlignmentStyle> = {
            let mut order = Vec::new();
            // First pair's styles come first (they're inserted first in the map)
            for &s in styles_for(first_pair) {
                if !order.contains(&s) { order.push(s); }
            }
            // Then other pair styles
            for pair in pairs.iter() {
                if !pair.is_kwsplat {
                    for &s in styles_for(pair) {
                        if !order.contains(&s) { order.push(s); }
                    }
                }
            }
            order
        };
        sorted_styles.sort_by_key(|(style, offenses)| {
            let order_idx = style_order.iter().position(|s| s == style).unwrap_or(usize::MAX);
            (offenses.len(), order_idx)
        });
        if let Some((_style, offenses)) = sorted_styles.into_iter().next() {
            self.offenses.extend(offenses);
        }
    }

    fn make_offense(&self, msg: &str, pair: &PairInfo) -> Offense {
        self.ctx.offense_with_range(
            "Layout/HashAlignment", msg, Severity::Convention,
            pair.node_start, pair.first_line_end,
        )
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
