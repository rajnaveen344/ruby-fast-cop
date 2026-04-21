//! Layout/ExtraSpacing — flags unnecessary whitespace inside a line of code.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/extra_spacing.rb
//!
//! Uses the shared `preceding_following_alignment` helper (RuboCop's
//! `PrecedingFollowingAlignment` mixin) for `AllowForAlignment`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::preceding_following_alignment::{
    aligned_with_something, AlignRange, AlignmentIndex,
};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const MSG_UNNECESSARY: &str = "Unnecessary spacing detected.";
const MSG_UNALIGNED_ASGN_PRECEDING: &str =
    "`=` is not aligned with the preceding assignment.";

pub struct ExtraSpacing {
    allow_for_alignment: bool,
    allow_before_trailing_comments: bool,
    force_equal_sign_alignment: bool,
}

impl Default for ExtraSpacing {
    fn default() -> Self {
        Self {
            allow_for_alignment: true,
            allow_before_trailing_comments: false,
            force_equal_sign_alignment: false,
        }
    }
}

impl ExtraSpacing {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(
        allow_for_alignment: bool,
        allow_before_trailing_comments: bool,
        force_equal_sign_alignment: bool,
    ) -> Self {
        Self {
            allow_for_alignment,
            allow_before_trailing_comments,
            force_equal_sign_alignment,
        }
    }
}

impl Cop for ExtraSpacing {
    fn name(&self) -> &'static str { "Layout/ExtraSpacing" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // AST pre-pass: collect ignored byte ranges and ignore offsets for `=`.
        let mut prep = PrePass {
            ctx,
            ignored_pair_gaps: Vec::new(),
            opaque_ranges: Vec::new(),
            ignored_eq_offsets: Vec::new(),
            assignment_eq_offsets: Vec::new(),
        };
        prep.visit_program_node(node);

        let idx = AlignmentIndex::build(ctx.source);
        let mut offenses = Vec::new();

        if self.force_equal_sign_alignment {
            offenses.extend(check_force_equal_sign_alignment(ctx, &prep));
        }

        // Token-pair pass: walk line by line.
        offenses.extend(check_extra_spacing_between_tokens(ctx, &idx, &prep, self));

        offenses
    }
}

// ── AST pre-pass ──

struct PrePass<'a> {
    ctx: &'a CheckContext<'a>,
    /// Byte-range `(start_offset, end_offset)` for the gap between a pair's
    /// key and value in a multiline hash. Extra spaces inside this range are
    /// ignored (handled by `Layout/HashAlignment`).
    ignored_pair_gaps: Vec<(usize, usize)>,
    /// Byte ranges for string / heredoc / xstring / regex / symbol content
    /// whose whitespace we must NOT flag. The ranges are inclusive-start,
    /// exclusive-end.
    opaque_ranges: Vec<(usize, usize)>,
    /// Byte offsets of `=` signs that should NOT be treated as assignment
    /// tokens: optional-argument defaults in method definitions, and endless-
    /// method definition `=`.
    ignored_eq_offsets: Vec<usize>,
    /// Byte offsets of `=` signs that ARE assignment tokens (only collected
    /// when we need them for ForceEqualSignAlignment).
    assignment_eq_offsets: Vec<usize>,
}

impl<'a> Visit<'_> for PrePass<'a> {
    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        if self.ctx.same_line(node.location().start_offset(), node.location().end_offset()) {
            // Single-line hash — pairs participate in normal token checking.
            ruby_prism::visit_hash_node(self, node);
            return;
        }
        // Multiline hash — skip the gap between each pair's key and value.
        for el in node.elements().iter() {
            if let Some(pair) = el.as_assoc_node() {
                let k = pair.key().location();
                let v = pair.value().location();
                let kend = k.end_offset();
                let vstart = v.start_offset();
                if vstart > kend {
                    self.ignored_pair_gaps.push((kend, vstart));
                }
            }
        }
        ruby_prism::visit_hash_node(self, node);
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        // Like HashNode but for implicit (method-argument) hashes.
        let elements: Vec<_> = node.elements().iter().collect();
        if elements.is_empty() {
            return;
        }
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        if self.ctx.same_line(start, end) {
            ruby_prism::visit_keyword_hash_node(self, node);
            return;
        }
        for el in elements {
            if let Some(pair) = el.as_assoc_node() {
                let k = pair.key().location();
                let v = pair.value().location();
                let kend = k.end_offset();
                let vstart = v.start_offset();
                if vstart > kend {
                    self.ignored_pair_gaps.push((kend, vstart));
                }
            }
        }
        ruby_prism::visit_keyword_hash_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Endless method: `def foo = expr` — `=` is the equal_loc.
        if let Some(eq) = node.equal_loc() {
            self.ignored_eq_offsets.push(eq.start_offset());
        }
        // Optional arguments inside params (handled by visit_optional_parameter_node).
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode) {
        let eq = node.operator_loc();
        self.ignored_eq_offsets.push(eq.start_offset());
        ruby_prism::visit_optional_parameter_node(self, node);
    }

    fn visit_optional_keyword_parameter_node(
        &mut self,
        node: &ruby_prism::OptionalKeywordParameterNode,
    ) {
        // Keyword optional: `foo(k: default)` — `:` is the separator, not `=`.
        // No `=` to ignore here.
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
    }

    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if let Some(content) = string_content_range(node.opening_loc(), node.closing_loc(), &node.location()) {
            self.opaque_ranges.push(content);
        }
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        // Blindly mask the inside of the string (interpolation embeds may
        // contain spaces, but RuboCop's tokenizer treats interpolation parts
        // as non-ws tokens for alignment; safer to mask the whole thing).
        if let Some(content) = string_content_range(node.opening_loc(), node.closing_loc(), &node.location()) {
            self.opaque_ranges.push(content);
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode) {
        let open = Some(node.opening_loc());
        let close = Some(node.closing_loc());
        if let Some(content) = string_content_range(open, close, &node.location()) {
            self.opaque_ranges.push(content);
        }
        ruby_prism::visit_x_string_node(self, node);
    }

    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let open = Some(node.opening_loc());
        let close = Some(node.closing_loc());
        if let Some(content) = string_content_range(open, close, &node.location()) {
            self.opaque_ranges.push(content);
        }
        ruby_prism::visit_regular_expression_node(self, node);
    }

    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode) {
        if let (Some(_), Some(_)) = (node.opening_loc(), node.closing_loc()) {
            if let Some(content) = string_content_range(node.opening_loc(), node.closing_loc(), &node.location()) {
                self.opaque_ranges.push(content);
            }
        }
        ruby_prism::visit_symbol_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_instance_variable_write_node(self, node);
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_class_variable_write_node(self, node);
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_global_variable_write_node(self, node);
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_constant_write_node(self, node);
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        let op = node.operator_loc();
        self.assignment_eq_offsets.push(op.start_offset());
        ruby_prism::visit_multi_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        self.assignment_eq_offsets.push(node.binary_operator_loc().start_offset());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_instance_variable_operator_write_node(&mut self, node: &ruby_prism::InstanceVariableOperatorWriteNode) {
        self.assignment_eq_offsets.push(node.binary_operator_loc().start_offset());
        ruby_prism::visit_instance_variable_operator_write_node(self, node);
    }
    fn visit_class_variable_operator_write_node(&mut self, node: &ruby_prism::ClassVariableOperatorWriteNode) {
        self.assignment_eq_offsets.push(node.binary_operator_loc().start_offset());
        ruby_prism::visit_class_variable_operator_write_node(self, node);
    }
    fn visit_global_variable_operator_write_node(&mut self, node: &ruby_prism::GlobalVariableOperatorWriteNode) {
        self.assignment_eq_offsets.push(node.binary_operator_loc().start_offset());
        ruby_prism::visit_global_variable_operator_write_node(self, node);
    }
    fn visit_constant_operator_write_node(&mut self, node: &ruby_prism::ConstantOperatorWriteNode) {
        self.assignment_eq_offsets.push(node.binary_operator_loc().start_offset());
        ruby_prism::visit_constant_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }
    fn visit_instance_variable_and_write_node(&mut self, node: &ruby_prism::InstanceVariableAndWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_instance_variable_and_write_node(self, node);
    }
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode) {
        self.assignment_eq_offsets.push(node.binary_operator_loc().start_offset());
        ruby_prism::visit_call_operator_write_node(self, node);
    }
    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_call_or_write_node(self, node);
    }
    fn visit_call_and_write_node(&mut self, node: &ruby_prism::CallAndWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_call_and_write_node(self, node);
    }
    fn visit_index_operator_write_node(&mut self, node: &ruby_prism::IndexOperatorWriteNode) {
        self.assignment_eq_offsets.push(node.binary_operator_loc().start_offset());
        ruby_prism::visit_index_operator_write_node(self, node);
    }
    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_index_or_write_node(self, node);
    }
    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode) {
        self.assignment_eq_offsets.push(node.operator_loc().start_offset());
        ruby_prism::visit_index_and_write_node(self, node);
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Attribute/index setter: `obj.foo = bar` or `obj[i] = bar`. The method
        // name ends in `=`. We need to find the *source* position of the `=`.
        let name_bytes = node.name().as_slice();
        if name_bytes.last() == Some(&b'=')
            && !matches!(
                name_bytes,
                b"==" | b"!=" | b"<=" | b">=" | b"===" | b"<=>"
            )
        {
            // Scan source forward from message_loc (or opening paren) for `=`
            // followed by space/newline/non-`=` (to distinguish from `==`).
            let search_start = node
                .message_loc()
                .map(|l| l.end_offset())
                .unwrap_or_else(|| node.location().start_offset());
            let bytes = self.ctx.source.as_bytes();
            // Go up to the value node's start.
            let value_end = node
                .arguments()
                .and_then(|a| a.arguments().iter().last().map(|n| n.location().start_offset()))
                .unwrap_or_else(|| node.location().end_offset());
            let mut k = search_start;
            while k + 1 < value_end && k < bytes.len() {
                if bytes[k] == b'=' {
                    // Ensure it's not part of `==`.
                    let prev_is_op = k > 0
                        && matches!(bytes[k - 1], b'<' | b'>' | b'!' | b'=' | b'+' | b'-' | b'*' | b'/' | b'%' | b'|' | b'&' | b'^');
                    let next_is_eq = k + 1 < bytes.len() && bytes[k + 1] == b'=';
                    if !prev_is_op && !next_is_eq {
                        self.assignment_eq_offsets.push(k);
                        break;
                    }
                }
                k += 1;
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn string_content_range(
    opening: Option<ruby_prism::Location>,
    closing: Option<ruby_prism::Location>,
    full: &ruby_prism::Location,
) -> Option<(usize, usize)> {
    let s = match opening {
        Some(o) => o.end_offset(),
        None => full.start_offset(),
    };
    let e = match closing {
        Some(c) => c.start_offset(),
        None => full.end_offset(),
    };
    if e > s {
        Some((s, e))
    } else {
        None
    }
}

// ── Token-pair pass ──

fn check_extra_spacing_between_tokens(
    ctx: &CheckContext,
    idx: &AlignmentIndex,
    prep: &PrePass,
    cop: &ExtraSpacing,
) -> Vec<Offense> {
    let mut offenses = Vec::new();
    let source = ctx.source;
    let bytes = source.as_bytes();
    let mut line_start = 0usize;

    // Comments that have been seen aligned with each other — RuboCop's
    // `@aligned_comments = aligned_locations(comments)`. A line `l` is in
    // `aligned_comments` iff there exists an adjacent comment line where the
    // `#` is at the same column.
    let aligned_comments = compute_aligned_comments(idx);

    let mut line_index = 0usize; // 0-indexed
    loop {
        let nl_pos = source[line_start..].find('\n');
        let line_end = nl_pos.map(|p| line_start + p).unwrap_or(source.len());

        scan_line(
            ctx,
            idx,
            prep,
            cop,
            source,
            bytes,
            line_start,
            line_end,
            (line_index + 1) as u32,
            &aligned_comments,
            &mut offenses,
        );

        match nl_pos {
            Some(p) => {
                line_start = line_start + p + 1;
                line_index += 1;
                if line_start >= source.len() {
                    break;
                }
            }
            None => break,
        }
    }

    offenses
}

/// Returns set of 1-indexed line numbers that have a comment aligned with
/// an adjacent comment line at the same column.
fn compute_aligned_comments(idx: &AlignmentIndex) -> std::collections::HashSet<u32> {
    let mut by_lineno: Vec<(u32, u32)> = idx
        .comments_by_line
        .iter()
        .map(|(&l, &c)| (l, c))
        .collect();
    by_lineno.sort_by_key(|x| x.0);
    let mut out = std::collections::HashSet::new();
    for window in by_lineno.windows(2) {
        let (l1, c1) = window[0];
        let (l2, c2) = window[1];
        if c1 == c2 {
            out.insert(l1);
            out.insert(l2);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn scan_line(
    ctx: &CheckContext,
    idx: &AlignmentIndex,
    prep: &PrePass,
    cop: &ExtraSpacing,
    source: &str,
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    lineno: u32,
    aligned_comments: &std::collections::HashSet<u32>,
    offenses: &mut Vec<Offense>,
) {
    // Step 1: find trailing-comment column (if any) using the global idx.
    let comment_col = idx.comments_by_line.get(&lineno).copied();

    // Step 2: walk the line left-to-right, tracking "in-opaque" state via
    // prep.opaque_ranges, and "in-ignored-gap" state via prep.ignored_pair_gaps.
    // Find whitespace runs of length >= 2 between two non-ws tokens.

    // Skip leading whitespace.
    let mut i = line_start;
    while i < line_end && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= line_end {
        return;
    }

    // Advance past the line's first token. `last_tok_end` then tracks the end
    // of the most recently advanced token.
    i = advance_token(bytes, i, line_end, prep);
    let mut last_tok_end: Option<usize> = Some(i);

    while i < line_end {
        // Collect whitespace run.
        let ws_start = i;
        while i < line_end && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        let ws_end = i;
        // If we hit EOL or a comment, stop — trailing whitespace is not flagged.
        if ws_start == ws_end {
            // No whitespace — but we stopped for another reason (e.g. end
            // of token). Advance past the next token.
            let adv = advance_token(bytes, i, line_end, prep);
            if adv == i {
                break;
            }
            i = adv;
            last_tok_end = Some(i);
            continue;
        }
        if ws_end >= line_end {
            break; // trailing whitespace — not an offense
        }
        // If next char starts a `#`-comment:
        let next_is_comment = bytes[ws_end] == b'#'
            && comment_col.map(|c| c as usize == (ws_end - line_start)).unwrap_or(false);

        // Compute the offense range = [tok1_end, tok2_begin - 1) in byte offsets.
        let tok1_end = last_tok_end.unwrap_or(ws_start);
        let tok2_begin = ws_end;
        if tok2_begin <= tok1_end {
            // nothing
        } else if tok2_begin - tok1_end <= 1 {
            // only 1 space → OK
        } else {
            let range_start = tok1_end;
            let range_end = tok2_begin - 1;

            // Skip: ignored hash-pair gap.
            let in_ignored_gap = prep
                .ignored_pair_gaps
                .iter()
                .any(|&(s, e)| range_start >= s && range_start < e);

            // Skip: AllowBeforeTrailingComments when token2 is `#`.
            let skip_trailing_comment =
                cop.allow_before_trailing_comments && next_is_comment;

            // Skip: ForceEqualSignAlignment takes over when token2 is an
            // assignment `=`. The alignment pass handles these offenses.
            let skip_for_force_eq = cop.force_equal_sign_alignment
                && prep
                    .assignment_eq_offsets
                    .iter()
                    .any(|&o| o == ws_end && !prep.ignored_eq_offsets.contains(&o));

            if !in_ignored_gap && !skip_trailing_comment && !skip_for_force_eq {
                // Skip: AllowForAlignment.
                let tok2_text = extract_token_text(source, ws_end, line_end);
                let col_start = (range_start - line_start) as u32;
                let col_end = (range_end - line_start) as u32;

                let is_aligned = if !cop.allow_for_alignment {
                    false
                } else if next_is_comment {
                    aligned_comments.contains(&lineno)
                } else {
                    let tok2_col = (tok2_begin - line_start) as u32;
                    let tok2_last_col = tok2_col + tok2_text.len() as u32;
                    let range = AlignRange {
                        line: lineno,
                        column: tok2_col,
                        last_column: tok2_last_col,
                        source: &source[tok2_begin..tok2_begin + tok2_text.len()],
                    };
                    aligned_with_something(idx, range)
                };

                if !is_aligned {
                    let loc = Location::new(lineno, col_start, lineno, col_end);
                    let correction = Correction::delete(range_start, range_end);
                    offenses.push(
                        Offense::new(
                            "Layout/ExtraSpacing",
                            MSG_UNNECESSARY,
                            Severity::Convention,
                            loc,
                            ctx.filename,
                        )
                        .with_correction(correction),
                    );
                }
            }
        }

        // Past the whitespace; advance over the next token.
        if next_is_comment {
            break; // comment runs to EOL
        }
        let adv = advance_token(bytes, ws_end, line_end, prep);
        if adv == ws_end {
            break;
        }
        i = adv;
        last_tok_end = Some(i);
    }
}

/// Extract the "next Ruby token" starting at `start` for alignment purposes.
/// Mirrors what RuboCop's tokenizer would return:
///   - a multi-char `=`-family operator (`+=`, `||=`, `<<`, `==`, ...) if present
///   - a word (alphanumeric/underscore) run for identifiers, keywords, numbers
///   - a single character for other punctuation (`.`, `#`, `"`, `'`, `(`, etc.)
fn extract_token_text(source: &str, start: usize, line_end: usize) -> &str {
    let bytes = source.as_bytes();
    if start >= line_end {
        return &source[start..start];
    }
    // `=`-family / op-assign / comparison — longest match first.
    const EQ_TOKENS: &[&str] = &[
        "<<=", ">>=", "**=", "===", "||=", "&&=",
        "==", "!=", "<=", ">=", "<<", "+=", "-=", "*=", "/=", "%=", "|=", "&=", "^=", "=",
    ];
    for tok in EQ_TOKENS {
        let tb = tok.as_bytes();
        if line_end - start >= tb.len() && &bytes[start..start + tb.len()] == tb {
            return &source[start..start + tb.len()];
        }
    }
    let first = bytes[start];
    if first.is_ascii_alphanumeric() || first == b'_' {
        let mut e = start;
        while e < line_end && (bytes[e].is_ascii_alphanumeric() || bytes[e] == b'_') {
            e += 1;
        }
        return &source[start..e];
    }
    &source[start..start + 1]
}

/// Advance past one "token" at `i`, respecting opaque ranges. If `i` is inside
/// an opaque range, skip to its end; otherwise step over the contiguous non-ws
/// span. Returns the byte offset just past the token.
fn advance_token(bytes: &[u8], i: usize, line_end: usize, prep: &PrePass) -> usize {
    if i >= line_end {
        return i;
    }
    // If we're inside an opaque range, jump to its end.
    for &(s, e) in &prep.opaque_ranges {
        if i >= s && i < e {
            return e.min(line_end);
        }
    }
    let mut k = i;
    while k < line_end && bytes[k] != b' ' && bytes[k] != b'\t' {
        // If we encounter the start of an opaque range mid-token, jump past it.
        let mut jumped = false;
        for &(s, e) in &prep.opaque_ranges {
            if k == s {
                k = e.min(line_end);
                jumped = true;
                break;
            }
        }
        if !jumped {
            k += 1;
        }
    }
    k
}

// ── ForceEqualSignAlignment pass ──

fn check_force_equal_sign_alignment(ctx: &CheckContext, prep: &PrePass) -> Vec<Offense> {
    // Gather `(line, eq_start_offset, eq_len)` for every assignment `=`-token.
    let mut assign_tokens: Vec<(u32, usize, usize)> = Vec::new();
    // Count `=` operator length. For plain `=` it's 1; for `+=` / `<<=` / etc.
    // The byte pointed to by `operator_loc.start_offset()` is the `=` or the
    // start of the longer operator, depending on the node. For write-nodes
    // with `operator_loc`, the loc covers the full operator. So we need the
    // full operator range — re-derive it from the source by scanning backward
    // from `start` is hard, so: for each node kind we already captured
    // `start_offset()`; now compute the token length by scanning forward from
    // that offset matching equals-family characters.
    for &eq_start in &prep.assignment_eq_offsets {
        if prep.ignored_eq_offsets.contains(&eq_start) {
            continue;
        }
        let len = equals_token_length(ctx.source, eq_start);
        let line = ctx.line_of(eq_start) as u32;
        assign_tokens.push((line, eq_start, len));
    }

    // Only keep the FIRST assignment per line. RuboCop: `tokens.uniq(&:line)`.
    assign_tokens.sort_by_key(|t| (t.0, t.1));
    let mut seen_lines = std::collections::HashSet::new();
    assign_tokens.retain(|t| seen_lines.insert(t.0));

    let mut assignment_lines: std::collections::HashSet<u32> =
        assign_tokens.iter().map(|t| t.0).collect();
    let _ = &mut assignment_lines;

    let mut offenses = Vec::new();
    let mut corrected: std::collections::HashSet<usize> = std::collections::HashSet::new();

    let source = ctx.source;
    let line_count = source.lines().count() as u32;

    // Build a quick "text of line N" lookup.
    let line_texts: Vec<&str> = source.split('\n').collect();

    for &(tok_line, tok_start, tok_len) in &assign_tokens {
        // `check_assignment`:
        //   aligned_with_preceding_equals_operator → returns :yes/:no/:none
        //   if :no → offense.
        //   Line range is `token.line.downto(1)` — INCLUDES the token's line.
        let preceding_lines: Vec<u32> = (1..=tok_line).rev().collect();
        let status = aligned_with_equals_sign(
            ctx,
            &line_texts,
            &assign_tokens,
            tok_line,
            tok_start,
            tok_len,
            &preceding_lines,
        );
        if status != AlignStatus::No {
            continue;
        }
        // Offense on the `=` token.
        let line = tok_line;
        let col_start = ctx.col_of(tok_start) as u32;
        let col_end = col_start + tok_len as u32;
        let loc = Location::new(line, col_start, line, col_end);
        // Correction: compute alignment column using `align_equal_signs`.
        let correction = align_equal_signs(
            ctx,
            &line_texts,
            &assign_tokens,
            tok_line,
            line_count,
            &mut corrected,
        );
        let mut off = Offense::new(
            "Layout/ExtraSpacing",
            MSG_UNALIGNED_ASGN_PRECEDING,
            Severity::Convention,
            loc,
            ctx.filename,
        );
        if let Some(c) = correction {
            off = off.with_correction(c);
        }
        offenses.push(off);
    }

    offenses
}

fn equals_token_length(source: &str, start: usize) -> usize {
    // Operator tokens that end in `=`:
    const CANDIDATES: &[&str] = &[
        "<<=", ">>=", "**=", "||=", "&&=",
        "+=", "-=", "*=", "/=", "%=", "|=", "&=", "^=", "=",
    ];
    let bytes = source.as_bytes();
    for tok in CANDIDATES {
        let tb = tok.as_bytes();
        if bytes.len() - start < tb.len() {
            continue;
        }
        if &bytes[start..start + tb.len()] == tb {
            return tb.len();
        }
    }
    1
}

#[derive(Debug, PartialEq, Eq)]
enum AlignStatus {
    Yes,
    No,
    None,
}

#[allow(clippy::too_many_arguments)]
fn aligned_with_equals_sign(
    ctx: &CheckContext,
    line_texts: &[&str],
    all_tokens: &[(u32, usize, usize)],
    tok_line: u32,
    tok_start: usize,
    tok_len: usize,
    line_range: &[u32],
) -> AlignStatus {
    let tok_line_indent = indent_of(line_texts, tok_line);
    let assignment_lines = relevant_assignment_lines(line_texts, all_tokens, line_range, tok_line);
    // RuboCop `assignment_lines[1]` — the SECOND element.
    let Some(&relevant_line_number) = assignment_lines.get(1) else {
        return AlignStatus::None;
    };
    let relevant_indent = indent_of(line_texts, relevant_line_number);
    if relevant_indent < tok_line_indent {
        return AlignStatus::None;
    }
    // RuboCop: `aligned_equals_operator?(token.pos, relevant_line_number)`
    //   ≈ "does the first equals-family token on relevant_line_number end at
    //     the same column as our `tok`?"
    let tok_last_col = (ctx.col_of(tok_start) + tok_len) as u32;
    // Find the first `=`-family assignment token on `relevant_line_number`.
    let other = all_tokens.iter().find(|t| t.0 == relevant_line_number);
    if let Some(&(_, o_start, o_len)) = other {
        let o_last_col = (ctx.col_of(o_start) + o_len) as u32;
        if o_last_col == tok_last_col {
            return AlignStatus::Yes;
        } else {
            return AlignStatus::No;
        }
    }
    AlignStatus::None
}

fn indent_of(line_texts: &[&str], lineno: u32) -> u32 {
    let Some(line) = line_texts.get((lineno - 1) as usize) else {
        return 0;
    };
    line.bytes()
        .position(|b| b != b' ' && b != b'\t')
        .map(|p| p as u32)
        .unwrap_or(0)
}

fn is_blank(line_texts: &[&str], lineno: u32) -> bool {
    match line_texts.get((lineno - 1) as usize) {
        Some(line) => line.bytes().all(|b| b == b' ' || b == b'\t'),
        None => true,
    }
}

fn relevant_assignment_lines(
    line_texts: &[&str],
    all_tokens: &[(u32, usize, usize)],
    line_range: &[u32],
    starting_line: u32,
) -> Vec<u32> {
    let mut out = Vec::new();
    let original_indent = indent_of(line_texts, starting_line);
    let mut relevant_line_indent_at_level = true;
    let assignment_lines_set: std::collections::HashSet<u32> =
        all_tokens.iter().map(|t| t.0).collect();

    for &lineno in line_range {
        let current_indent = indent_of(line_texts, lineno);
        let blank = is_blank(line_texts, lineno);
        if (current_indent < original_indent && !blank)
            || (relevant_line_indent_at_level && blank)
        {
            break;
        }
        if assignment_lines_set.contains(&lineno) && current_indent == original_indent {
            out.push(lineno);
        }
        if !blank {
            relevant_line_indent_at_level = current_indent == original_indent;
        }
    }
    out
}

fn align_equal_signs(
    ctx: &CheckContext,
    line_texts: &[&str],
    all_tokens: &[(u32, usize, usize)],
    tok_line: u32,
    last_line_number: u32,
    corrected: &mut std::collections::HashSet<usize>,
) -> Option<Correction> {
    // Build the set of "relevant" lines (preceding + following at same indent).
    let preceding: Vec<u32> = (1..tok_line).rev().collect();
    let following: Vec<u32> = ((tok_line)..=last_line_number).collect();
    let mut lines: Vec<u32> =
        relevant_assignment_lines(line_texts, all_tokens, &preceding, tok_line);
    lines.extend(relevant_assignment_lines(
        line_texts,
        all_tokens,
        &following,
        tok_line,
    ));
    lines.sort();
    lines.dedup();

    let tokens: Vec<&(u32, usize, usize)> =
        all_tokens.iter().filter(|t| lines.contains(&t.0)).collect();
    if tokens.is_empty() {
        return None;
    }
    // `align_column`: where would `=` end if we removed extra spaces before it?
    let align_to = tokens
        .iter()
        .map(|&&(line, start, len)| align_column(ctx, line_texts, line, start, len))
        .max()
        .unwrap_or(0);

    let mut edits = Vec::new();
    for &&(_line, start, len) in &tokens {
        if !corrected.insert(start) {
            continue;
        }
        let last_col = (ctx.col_of(start) + len) as u32;
        let diff = align_to as i32 - last_col as i32;
        if diff > 0 {
            edits.push(crate::offense::Edit {
                start_offset: start,
                end_offset: start,
                replacement: " ".repeat(diff as usize),
            });
        } else if diff < 0 {
            let remove = (-diff) as usize;
            edits.push(crate::offense::Edit {
                start_offset: start.saturating_sub(remove),
                end_offset: start,
                replacement: String::new(),
            });
        }
    }
    if edits.is_empty() {
        None
    } else {
        Some(Correction { edits })
    }
}

fn align_column(ctx: &CheckContext, line_texts: &[&str], line: u32, start: usize, len: usize) -> u32 {
    // RuboCop: `asgn_token.pos.last_column - spaces + 1`, where `spaces` is the
    // count of trailing spaces between the preceding token and the `=`.
    let line_text = line_texts.get((line - 1) as usize).copied().unwrap_or("");
    let col = ctx.col_of(start) as usize;
    let leading = &line_text[..col.min(line_text.len())];
    let spaces = leading.len() - leading.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
    let last_column = col + len;
    (last_column - spaces + 1) as u32
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_for_alignment: bool,
    allow_before_trailing_comments: bool,
    force_equal_sign_alignment: bool,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            allow_for_alignment: true,
            allow_before_trailing_comments: false,
            force_equal_sign_alignment: false,
        }
    }
}

crate::register_cop!("Layout/ExtraSpacing", |cfg| {
    let c: Cfg = cfg.typed("Layout/ExtraSpacing");
    Some(Box::new(ExtraSpacing::with_config(
        c.allow_for_alignment,
        c.allow_before_trailing_comments,
        c.force_equal_sign_alignment,
    )))
});
