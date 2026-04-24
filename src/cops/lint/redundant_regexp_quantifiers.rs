//! Lint/RedundantRegexpQuantifiers - Detects redundant nested quantifiers like `(?:a+)+`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/redundant_regexp_quantifiers.rb
//!
//! RuboCop uses the `regexp_parser` gem which we don't have in Rust. We
//! implement a small targeted parser that identifies (?:...)-wrapped
//! expressions with greedy quantifiers and merges them with a redundantly
//! quantifiable inner child.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct RedundantRegexpQuantifiers;

impl RedundantRegexpQuantifiers {
    pub fn new() -> Self {
        Self
    }
}

/// An expression parsed from regexp content.
#[derive(Debug, Clone)]
struct Expr {
    kind: ExprKind,
    /// Byte offset where the expression (without quantifier) starts.
    start: usize,
    /// Byte offset where the expression ends (exclusive), BEFORE the quantifier.
    end: usize,
    /// Offsets of the quantifier, if any: (q_start, q_end).
    quantifier: Option<Quantifier>,
    /// Inner expressions (for groups).
    children: Vec<Expr>,
}

#[derive(Debug, Clone)]
struct Quantifier {
    start: usize,
    end: usize,
    text: String,
    /// Normalized: '*', '?', or '+'. Set only when greedy and normalizable.
    normalized: Option<char>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExprKind {
    /// `(?:...)` — non-capturing (passive) group.
    PassiveGroup,
    /// `(...)`, `(?<name>...)`, `(?=...)`, etc. (any non-passive group).
    OtherGroup,
    /// Character class `[...]`.
    CharSet,
    /// Terminal (single char, escape, anchor, dot, etc.).
    Terminal,
    /// Alternation inside a group body (special-cased at parse time to
    /// disqualify a passive-group from being "redundant").
    Alternation,
}

struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
    end: usize,
    extended: bool,
}

impl<'a> Parser<'a> {
    /// Parse body as a list of sub-expressions. Stops at the first unmatched
    /// `)` (returns without consuming it), or at `end`.
    fn parse_sequence(&mut self) -> Vec<Expr> {
        let mut out = Vec::new();
        while self.i < self.end {
            let b = self.bytes[self.i];
            if b == b')' {
                break;
            }
            if b == b'|' {
                // Alternation — record marker and continue.
                out.push(Expr {
                    kind: ExprKind::Alternation,
                    start: self.i,
                    end: self.i + 1,
                    quantifier: None,
                    children: Vec::new(),
                });
                self.i += 1;
                continue;
            }
            // Skip extended-mode whitespace and comments.
            if self.extended {
                if b == b' ' || b == b'\t' || b == b'\n' {
                    self.i += 1;
                    continue;
                }
                if b == b'#' {
                    while self.i < self.end && self.bytes[self.i] != b'\n' {
                        self.i += 1;
                    }
                    continue;
                }
            }
            let expr = match b {
                b'(' => self.parse_group(),
                b'[' => self.parse_char_set(),
                b'\\' => self.parse_escape(),
                _ => self.parse_terminal(),
            };
            out.push(expr);
        }
        out
    }

    fn parse_group(&mut self) -> Expr {
        let start = self.i;
        self.i += 1; // consume `(`
        let mut kind = ExprKind::OtherGroup;
        // Detect `(?:...)`
        if self.i + 1 < self.end && self.bytes[self.i] == b'?' && self.bytes[self.i + 1] == b':' {
            kind = ExprKind::PassiveGroup;
            self.i += 2;
        } else if self.i < self.end && self.bytes[self.i] == b'?' {
            // Other (?...) forms: (?=, (?!, (?<name>, (?#comment), (?imx), etc.
            // We treat all as non-passive; just skip over the special intro until we
            // find the body. For simplicity, parse as OtherGroup. This is OK because
            // we only flag PassiveGroup wrapping.
            self.i += 1;
            // Don't consume specific forms — fall through; parse_sequence handles
            // everything, and `?` chars will be misinterpreted. Better: skip until
            // matching `)` for comments/lookaheads/etc.? For simplicity, skip the
            // whole special group as OtherGroup terminal.
            // Skip until matching `)`.
            let body_start = self.i;
            let mut depth = 1;
            while self.i < self.end && depth > 0 {
                match self.bytes[self.i] {
                    b'\\' if self.i + 1 < self.end => self.i += 2,
                    b'(' => {
                        depth += 1;
                        self.i += 1;
                    }
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        self.i += 1;
                    }
                    b'[' => {
                        // skip char class
                        self.i += 1;
                        while self.i < self.end && self.bytes[self.i] != b']' {
                            if self.bytes[self.i] == b'\\' && self.i + 1 < self.end {
                                self.i += 2;
                            } else {
                                self.i += 1;
                            }
                        }
                        if self.i < self.end {
                            self.i += 1;
                        }
                    }
                    _ => self.i += 1,
                }
            }
            let end = self.i;
            if self.i < self.end && self.bytes[self.i] == b')' {
                self.i += 1;
            }
            let quantifier = self.parse_quantifier_opt();
            let _ = body_start;
            return Expr {
                kind: ExprKind::OtherGroup,
                start,
                end,
                quantifier,
                children: Vec::new(),
            };
        }

        let children = self.parse_sequence();
        let end = self.i;
        // Consume `)`
        if self.i < self.end && self.bytes[self.i] == b')' {
            self.i += 1;
        }
        let quantifier = self.parse_quantifier_opt();
        Expr {
            kind,
            start,
            end,
            quantifier,
            children,
        }
    }

    fn parse_char_set(&mut self) -> Expr {
        let start = self.i;
        self.i += 1;
        // Allow leading `^` and leading `]` as literal.
        if self.i < self.end && self.bytes[self.i] == b'^' {
            self.i += 1;
        }
        if self.i < self.end && self.bytes[self.i] == b']' {
            self.i += 1;
        }
        while self.i < self.end && self.bytes[self.i] != b']' {
            if self.bytes[self.i] == b'\\' && self.i + 1 < self.end {
                self.i += 2;
            } else {
                self.i += 1;
            }
        }
        if self.i < self.end && self.bytes[self.i] == b']' {
            self.i += 1;
        }
        let end = self.i;
        let quantifier = self.parse_quantifier_opt();
        Expr {
            kind: ExprKind::CharSet,
            start,
            end,
            quantifier,
            children: Vec::new(),
        }
    }

    fn parse_escape(&mut self) -> Expr {
        let start = self.i;
        self.i += 1;
        if self.i < self.end {
            self.i += 1;
        }
        let end = self.i;
        let quantifier = self.parse_quantifier_opt();
        Expr {
            kind: ExprKind::Terminal,
            start,
            end,
            quantifier,
            children: Vec::new(),
        }
    }

    fn parse_terminal(&mut self) -> Expr {
        let start = self.i;
        self.i += 1;
        let end = self.i;
        let quantifier = self.parse_quantifier_opt();
        Expr {
            kind: ExprKind::Terminal,
            start,
            end,
            quantifier,
            children: Vec::new(),
        }
    }

    fn parse_quantifier_opt(&mut self) -> Option<Quantifier> {
        // In extended mode, whitespace / comments between an expr and its
        // quantifier are allowed.
        if self.extended {
            while self.i < self.end {
                let b = self.bytes[self.i];
                if b == b' ' || b == b'\t' || b == b'\n' {
                    self.i += 1;
                } else if b == b'#' {
                    while self.i < self.end && self.bytes[self.i] != b'\n' {
                        self.i += 1;
                    }
                } else {
                    break;
                }
            }
        }
        if self.i >= self.end {
            return None;
        }
        let q_start = self.i;
        let b = self.bytes[self.i];
        let base_end: usize;
        match b {
            b'?' | b'*' | b'+' => {
                self.i += 1;
                base_end = self.i;
            }
            b'{' => {
                // Find matching `}`
                let mut j = self.i + 1;
                while j < self.end && self.bytes[j] != b'}' {
                    // must contain only digits and `,`
                    let c = self.bytes[j];
                    if !(c.is_ascii_digit() || c == b',') {
                        return None;
                    }
                    j += 1;
                }
                if j >= self.end {
                    return None;
                }
                self.i = j + 1;
                base_end = self.i;
            }
            _ => return None,
        }
        // Reluctant `?` / possessive `+` suffix.
        let mut greedy = true;
        let mut normalized: Option<char> = None;
        if self.i < self.end && (self.bytes[self.i] == b'?' || self.bytes[self.i] == b'+') {
            greedy = false;
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.bytes[q_start..self.i])
            .unwrap_or("")
            .to_string();
        if greedy {
            // Normalize
            let base_text = std::str::from_utf8(&self.bytes[q_start..base_end]).unwrap_or("");
            normalized = normalize_quantifier(base_text);
        }
        Some(Quantifier {
            start: q_start,
            end: self.i,
            text,
            normalized,
        })
    }
}

fn normalize_quantifier(t: &str) -> Option<char> {
    match t {
        "*" | "{0,}" => Some('*'),
        "?" | "{0,1}" | "{,1}" => Some('?'),
        "+" | "{1,}" => Some('+'),
        _ => None,
    }
}

fn merged(a: char, b: char) -> char {
    if a == b {
        a
    } else {
        '*'
    }
}

/// Is this a "passive group with single child" (modulo free-space/alternation)?
fn redundant_passive_group(expr: &Expr) -> bool {
    if expr.kind != ExprKind::PassiveGroup {
        return false;
    }
    if expr.children.iter().any(|c| c.kind == ExprKind::Alternation) {
        return false;
    }
    if expr.children.len() != 1 {
        return false;
    }
    true
}

fn redundantly_quantifiable(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::PassiveGroup | ExprKind::CharSet | ExprKind::Terminal
    )
}

/// Walk the tree collecting (group, child) pairs matching RuboCop's algorithm:
/// for each redundant passive group with a mergeable outer quantifier, walk
/// its descendant chain-of-single-children and emit when a descendant also
/// has a mergeable quantifier. Seen set prevents double-yield.
fn each_redundantly_quantified_pair<'a>(root: &'a Expr, out: &mut Vec<(&'a Expr, &'a Expr)>, seen: &mut std::collections::HashSet<*const Expr>) {
    // Recurse first (bottom-up not strictly required, but mirrors iteration order).
    for c in &root.children {
        each_redundantly_quantified_pair(c, out, seen);
    }
    if !redundant_passive_group(root) {
        return;
    }
    let outer_q = match &root.quantifier {
        Some(q) if q.normalized.is_some() => q,
        _ => return,
    };
    // Walk descendant chain: each descendant expr is the single child.
    let mut cur = root;
    loop {
        if cur.children.len() != 1 {
            break;
        }
        let child = &cur.children[0];
        let ptr = child as *const Expr;
        if seen.contains(&ptr) {
            break;
        }
        seen.insert(ptr);
        if !redundantly_quantifiable(child) {
            break;
        }
        if let Some(q) = &child.quantifier {
            if q.normalized.is_some() {
                // Emit
                out.push((root, child));
                // Per RuboCop, we continue deeper; `seen` is used to avoid
                // re-entering the same subtree from an outer iteration.
                let _ = outer_q;
            }
        }
        if child.kind != ExprKind::PassiveGroup {
            break;
        }
        cur = child;
    }
}

/// Walk the whole tree to find ALL redundant passive groups as potential
/// outer groups. This is the top-level `node.parsed_tree.each_expression` walk
/// in RuboCop.
fn find_all_pairs(root: &Expr) -> Vec<Pair> {
    let mut pairs: Vec<Pair> = Vec::new();
    let mut seen: std::collections::HashSet<*const Expr> = std::collections::HashSet::new();
    collect_pairs(root, &mut pairs, &mut seen);
    pairs
}

/// Pair of (outer_group, child, intermediate_quantifier_positions).
/// `intermediates` are quantifier ranges to delete between outer and child
/// (used only on the deepest pair of a chain for correction fidelity).
struct Pair {
    group: Expr,
    child: Expr,
    intermediates: Vec<(usize, usize)>,
    /// All quantifier normalized chars in this chain (inner .. outer inclusive).
    /// Used on the deepest pair to compute the iterated merged result.
    chain_quantifiers: Vec<char>,
    /// Whether this pair is the deepest in its chain (i.e., emits correction).
    is_deepest: bool,
}

fn collect_pairs(
    expr: &Expr,
    pairs: &mut Vec<Pair>,
    seen: &mut std::collections::HashSet<*const Expr>,
) {
    if redundant_passive_group(expr) {
        if let Some(outer_q) = &expr.quantifier {
            if outer_q.normalized.is_some() {
                let mut cur = expr;
                let mut intermediates: Vec<(usize, usize)> = Vec::new();
                let mut chain: Vec<Pair> = Vec::new();
                let outer_norm = expr.quantifier.as_ref().unwrap().normalized.unwrap();
                loop {
                    if cur.children.len() != 1 {
                        break;
                    }
                    let child = &cur.children[0];
                    let ptr = child as *const Expr;
                    if seen.contains(&ptr) {
                        break;
                    }
                    seen.insert(ptr);
                    if !redundantly_quantifiable(child) {
                        break;
                    }
                    if let Some(q) = &child.quantifier {
                        if let Some(cn) = q.normalized {
                            // Build chain quantifiers: this child's q, then all intermediates' q, then outer.
                            let mut qs: Vec<char> = vec![cn];
                            // Intermediates recorded so far map to groups already walked past.
                            // They sit between this child and the outer group.
                            // We need their normalized chars — recompute from expr tree is complex;
                            // instead recompute by walking from expr to child.
                            let mut walker = expr;
                            while !std::ptr::eq(walker, cur) {
                                // walker's own quantifier is outer-level (outer group or intermediate).
                                // Skip outer group's quantifier — added at end.
                                if !std::ptr::eq(walker, expr) {
                                    if let Some(wq) = &walker.quantifier {
                                        if let Some(wn) = wq.normalized {
                                            qs.push(wn);
                                        }
                                    }
                                }
                                walker = &walker.children[0];
                            }
                            // Add `cur`'s quantifier if cur != expr (cur is an intermediate group).
                            if !std::ptr::eq(cur, expr) {
                                if let Some(wq) = &cur.quantifier {
                                    if let Some(wn) = wq.normalized {
                                        qs.push(wn);
                                    }
                                }
                            }
                            qs.push(outer_norm);
                            chain.push(Pair {
                                group: expr.clone(),
                                child: child.clone(),
                                intermediates: intermediates.clone(),
                                chain_quantifiers: qs,
                                is_deepest: false,
                            });
                        }
                    }
                    if child.kind != ExprKind::PassiveGroup {
                        break;
                    }
                    // Moving deeper: record this child's own quantifier as intermediate.
                    if let Some(q) = &child.quantifier {
                        intermediates.push((q.start, q.end));
                    }
                    cur = child;
                }
                if let Some(last) = chain.last_mut() {
                    last.is_deepest = true;
                }
                pairs.extend(chain);
            }
        }
    }
    for c in &expr.children {
        collect_pairs(c, pairs, seen);
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let content_loc = node.content_loc();
        let closing_loc = node.closing_loc();
        let closing_src =
            &self.ctx.source[closing_loc.start_offset()..closing_loc.end_offset()];
        let extended = closing_src.contains('x');

        let start = content_loc.start_offset();
        let end = content_loc.end_offset();
        let bytes = self.ctx.source.as_bytes();

        let mut parser = Parser {
            bytes,
            i: start,
            end,
            extended,
        };
        // Wrap in synthetic root so find_all_pairs walks from top.
        let children = parser.parse_sequence();
        let root = Expr {
            kind: ExprKind::OtherGroup,
            start,
            end,
            quantifier: None,
            children,
        };
        let pairs = find_all_pairs(&root);
        for pair in pairs {
            let cq = pair.child.quantifier.as_ref().unwrap();
            let gq = pair.group.quantifier.as_ref().unwrap();
            let inner_norm = cq.normalized.unwrap();
            let outer_norm = gq.normalized.unwrap();
            let merged_char = merged(inner_norm, outer_norm);
            let msg = format!(
                "Replace redundant quantifiers `{}` and `{}` with a single `{}`.",
                cq.text, gq.text, merged_char
            );
            let off_start = cq.start;
            let off_end = gq.end;
            let mut offense = self.ctx.offense_with_range(
                "Lint/RedundantRegexpQuantifiers",
                &msg,
                Severity::Warning,
                off_start,
                off_end,
            );
            if pair.is_deepest {
                // Full chain correction: merge ALL quantifiers in the chain.
                // RuboCop does this iteratively; we fold here in one pass.
                let mut chain_merged = pair.chain_quantifiers[0];
                for &q in &pair.chain_quantifiers[1..] {
                    chain_merged = merged(chain_merged, q);
                }
                let mut edits = Vec::new();
                edits.push(crate::offense::Edit {
                    start_offset: cq.start,
                    end_offset: cq.end,
                    replacement: chain_merged.to_string(),
                });
                edits.push(crate::offense::Edit {
                    start_offset: gq.start,
                    end_offset: gq.end,
                    replacement: String::new(),
                });
                for (s, e) in &pair.intermediates {
                    edits.push(crate::offense::Edit {
                        start_offset: *s,
                        end_offset: *e,
                        replacement: String::new(),
                    });
                }
                offense = offense.with_correction(crate::offense::Correction { edits });
            }
            self.offenses.push(offense);
        }
    }

    // Do NOT check InterpolatedRegularExpressionNode — interpolation disables cop.
}

impl Cop for RedundantRegexpQuantifiers {
    fn name(&self) -> &'static str {
        "Lint/RedundantRegexpQuantifiers"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
        };
        v.visit(&result.node());
        v.offenses
    }
}

// Silence unused warning — function kept for parity with RuboCop's iteration pattern.
#[allow(dead_code)]
fn _unused(expr: &Expr) {
    let mut pairs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    each_redundantly_quantified_pair(expr, &mut pairs, &mut seen);
}

// Dead `Correction` import silencing (we reach into module directly above).
#[allow(dead_code)]
fn _unused2() -> Option<Correction> {
    None
}

crate::register_cop!("Lint/RedundantRegexpQuantifiers", |_cfg| Some(Box::new(RedundantRegexpQuantifiers::new())));
