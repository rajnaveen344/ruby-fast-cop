//! Lint/RedundantCopDisableDirective cop
//!
//! Emits an offense for each `# rubocop:disable` (or `:todo`) directive where
//! the named cop(s) do not in fact need silencing within the directive's range.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/redundant_cop_disable_directive.rb
//!
//! Unlike every other cop, this one needs *peer offense data* to decide whether
//! a directive actually silences something. The runtime runs every other cop
//! first, then feeds the resulting offenses via `CheckContext::peer_offenses`.
//! When running in isolation (e.g. a fixture test invoking only this cop), any
//! offenses that RuboCop's own specs mock via `FakeLocation` must be supplied
//! through the fixture's `peer_offenses` field.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use std::collections::{HashMap, HashSet};

const COP_NAME: &str = "Lint/RedundantCopDisableDirective";

/// Built-in departments that RuboCop ships with.
const KNOWN_DEPARTMENTS: &[&str] = &[
    "Bundler", "Gemspec", "Layout", "Lint", "Metrics", "Migration",
    "Naming", "Security", "Style",
    // Common third-party departments referenced in fixtures.
    "Rails", "Rake", "Minitest", "RSpec", "Performance", "Thread",
    "Capybara", "FactoryBot",
];

pub struct RedundantCopDisableDirective {
    /// Cops the user explicitly disabled in config (`Enabled: false`).
    disabled_in_config: HashSet<String>,
    /// Departments the user explicitly disabled in config.
    disabled_depts_in_config: HashSet<String>,
    /// Known cop names (from the registry snapshot + common extras).
    known_cops: HashSet<String>,
}

impl RedundantCopDisableDirective {
    pub fn new(
        disabled_in_config: HashSet<String>,
        disabled_depts_in_config: HashSet<String>,
        known_cops: HashSet<String>,
    ) -> Self {
        Self {
            disabled_in_config,
            disabled_depts_in_config,
            known_cops,
        }
    }
}

// ── Directive parsing ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct DirName {
    name: String,
    /// Byte offset in the source where the name starts.
    start: usize,
}

#[derive(Debug)]
struct Directive {
    /// `disable` / `enable` (we treat `todo` as `disable` per RuboCop).
    enable: bool,
    /// True if the directive wrote `all` (applies to every cop).
    is_all: bool,
    /// Full comment start offset in source (position of the leading `#`).
    comment_start: usize,
    /// End offset (exclusive) of the directive proper — excludes trailing free-text.
    comment_end: usize,
    /// Cop names in the directive (excluding `all`).
    names: Vec<DirName>,
    /// Line number (1-based) of the directive.
    line: u32,
    /// Whether this directive is on an inline comment (code preceded `#` on the same line).
    inline: bool,
}

/// Scan all directive comments in source.
fn scan_directives(src: &str) -> Vec<Directive> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut line: u32 = 1;
    let mut pos: usize = 0;
    while pos <= bytes.len() {
        let line_start = pos;
        let mut line_end = line_start;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }
        let line_text = &src[line_start..line_end];
        if let Some(rel_hash) = find_comment_hash(line_text) {
            let comment_start_abs = line_start + rel_hash;
            let comment_text = &src[comment_start_abs..line_end];
            if let Some(dir) = parse_directive(comment_text, comment_start_abs, line, rel_hash) {
                out.push(dir);
            } else {
                // Retry: look for a later `# rubocop` fragment inside the comment body
                // to handle `# not a directive # rubocop:disable X`.
                let cb = comment_text.as_bytes();
                let mut k = 1;
                while k < cb.len() {
                    if cb[k] == b'#' {
                        if let Some(dir) = parse_directive(
                            &comment_text[k..],
                            comment_start_abs + k,
                            line,
                            rel_hash + k,
                        ) {
                            out.push(dir);
                            break;
                        }
                    }
                    k += 1;
                }
            }
        }
        if line_end >= bytes.len() {
            break;
        }
        pos = line_end + 1;
        line += 1;
    }
    out
}

/// Find the first `#` on the line that starts an actual comment (not inside a string).
fn find_comment_hash(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_s: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_s {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == q {
                in_s = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => in_s = Some(c),
            b'#' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_directive(text: &str, abs_start: usize, line: u32, rel_hash: usize) -> Option<Directive> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] != b'#' {
        return None;
    }
    let mut p = 1;
    skip_ws(bytes, &mut p);
    if !text[p..].starts_with("rubocop") {
        return None;
    }
    p += "rubocop".len();
    skip_ws(bytes, &mut p);
    if p >= bytes.len() || bytes[p] != b':' {
        return None;
    }
    p += 1;
    skip_ws(bytes, &mut p);
    let mode_start = p;
    while p < bytes.len() && bytes[p].is_ascii_alphabetic() {
        p += 1;
    }
    let mode = &text[mode_start..p];
    let enable = match mode {
        "enable" => true,
        "disable" | "todo" => false,
        _ => return None,
    };
    skip_ws(bytes, &mut p);

    let mut names = Vec::new();
    let mut is_all = false;
    let mut directive_end_in_text = p;
    loop {
        skip_ws(bytes, &mut p);
        if p >= bytes.len() {
            break;
        }
        let s = p;
        while p < bytes.len() && is_name_char(bytes[p]) {
            p += 1;
        }
        if s == p {
            break;
        }
        let name = &text[s..p];
        if name == "all" {
            is_all = true;
        } else {
            names.push(DirName {
                name: name.to_string(),
                start: abs_start + s,
            });
        }
        directive_end_in_text = p;
        skip_ws(bytes, &mut p);
        if p < bytes.len() && bytes[p] == b',' {
            p += 1;
            continue;
        }
        break;
    }

    let directive_end_abs = abs_start + directive_end_in_text;
    let inline = rel_hash > 0;

    Some(Directive {
        enable,
        is_all,
        comment_start: abs_start,
        comment_end: directive_end_abs,
        names,
        line,
        inline,
    })
}

fn skip_ws(bytes: &[u8], p: &mut usize) {
    while *p < bytes.len() && matches!(bytes[*p], b' ' | b'\t') {
        *p += 1;
    }
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'/' || b == b'_'
}

/// Canonicalize a cop token — strip trailing `,`, normalize abbreviated bare names
/// (like `MethodLength` → `Metrics/MethodLength`) against the known-cop set.
fn canonical_cop(raw: &str, known: &HashSet<String>) -> Option<String> {
    if raw.contains('/') {
        if known.contains(raw) {
            return Some(raw.to_string());
        }
        // case-insensitive match
        let lower = raw.to_lowercase();
        for k in known {
            if k.to_lowercase() == lower {
                return Some(k.clone());
            }
        }
        return None;
    }
    // bare name: find the unique qualified form in known
    for k in known {
        if let Some((_, name)) = k.split_once('/') {
            if name == raw {
                return Some(k.clone());
            }
        }
    }
    None
}

// ── Cop impl ───────────────────────────────────────────────────────────────────

impl Cop for RedundantCopDisableDirective {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        _node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let src = ctx.source;
        if !src.contains("rubocop") {
            return vec![];
        }
        let directives = scan_directives(src);
        if directives.is_empty() {
            return vec![];
        }

        // ── Build per-cop disable ranges ────────────────────────────────────
        // For each disable directive, compute `[start_line..end_line]` (inclusive of end).
        // end_line is set by a subsequent `enable` of the same cop or `enable all`,
        // else u32::MAX (open to EOF).
        //
        // We maintain a per-cop "currently-active directive index" so we can close it.

        let mut per_cop_ranges: HashMap<String, Vec<(usize, u32, u32)>> = HashMap::new(); // cop -> (directive_idx, start, end)
        let mut all_ranges: Vec<(usize, u32, u32)> = Vec::new(); // (directive_idx, start, end)
        let mut active_cop: HashMap<String, usize> = HashMap::new(); // cop -> index into per_cop_ranges[cop]
        let mut active_all: Option<usize> = None;
        // Track cops explicitly enabled at some point — used to suppress "already disabled" for re-disables.
        let mut previously_enabled_cops: HashSet<String> = HashSet::new();
        // Per-directive snapshot of `previously_enabled_cops` as it stood JUST BEFORE
        // that directive was processed. Used so redundancy checks at directive i
        // only see enables that came before i, not later in the file.
        let mut enabled_before: Vec<HashSet<String>> = Vec::with_capacity(directives.len());

        for (idx, dir) in directives.iter().enumerate() {
            enabled_before.push(previously_enabled_cops.clone());
            let effective = dir.line;
            if dir.enable {
                if dir.is_all {
                    // Close every open range at this line.
                    if let Some(ai) = active_all.take() {
                        all_ranges[ai].2 = dir.line;
                    }
                    for (cop, ri) in active_cop.drain().collect::<Vec<_>>() {
                        per_cop_ranges.get_mut(&cop).unwrap()[ri].2 = dir.line;
                        previously_enabled_cops.insert(cop);
                    }
                } else {
                    for n in &dir.names {
                        if let Some(ri) = active_cop.remove(&n.name) {
                            per_cop_ranges.get_mut(&n.name).unwrap()[ri].2 = dir.line;
                        }
                        previously_enabled_cops.insert(n.name.clone());
                    }
                }
            } else {
                if dir.is_all {
                    // `disable all` splits every open per-cop range at this line.
                    // Close each at dir.line, reopen at dir.line.
                    let open: Vec<(String, usize)> =
                        active_cop.iter().map(|(k, v)| (k.clone(), *v)).collect();
                    for (cop, ri) in open {
                        per_cop_ranges.get_mut(&cop).unwrap()[ri].2 = dir.line;
                        let v = per_cop_ranges.get_mut(&cop).unwrap();
                        let new_idx = v.len();
                        v.push((idx, dir.line, u32::MAX));
                        active_cop.insert(cop, new_idx);
                    }
                    let new_idx = all_ranges.len();
                    all_ranges.push((idx, effective, u32::MAX));
                    active_all = Some(new_idx);
                } else {
                    for n in &dir.names {
                        let v = per_cop_ranges.entry(n.name.clone()).or_default();
                        let new_idx = v.len();
                        v.push((idx, effective, u32::MAX));
                        active_cop.entry(n.name.clone()).or_insert(new_idx);
                    }
                }
            }
        }

        // ── Classify each directive's names ─────────────────────────────────
        // Step 1: Determine, per (directive_idx, cop_name), whether the cop in this
        // directive is "redundant" (no peer offense covered in this directive's range
        // AND the cop isn't already-disabled-by-prior-directive).

        // Build a lookup from directive_idx+cop -> (start, end) for redundancy checks.
        // Also compute "already disabled" for back-to-back / dept-umbrella cases.

        let mut per_dir_redundant: HashMap<usize, Vec<DirName>> = HashMap::new();
        let mut per_dir_all_redundant: HashMap<usize, bool> = HashMap::new();

        // Helper: does any peer offense with cop_name matching `pred` fall in [start..=end]?
        let peer_hit_for =
            |pred: &dyn Fn(&str) -> bool, start: u32, end: u32| -> bool {
                ctx.peer_offenses.iter().any(|o| {
                    let line = o.location.line;
                    line >= start && line <= end && pred(&o.cop_name)
                })
            };

        // Pre-compute "active-at-or-before-directive" covering set per directive.
        // For each directive D at index i, an "umbrella" cop C is actively disabled from
        // some earlier directive at this line iff C or its dept is in any range with
        // start <= D.line and end >= D.line AND the earlier directive's idx < i.
        // We use this for "already disabled" detection.

        for (idx, dir) in directives.iter().enumerate() {
            if dir.enable {
                continue;
            }
            // Self-silencing by active prior directive covering THIS cop.
            // If any prior-active directive silences `COP_NAME` at `dir.line`,
            // we emit nothing.
            let self_silenced_by_prior = {
                let covers_us = per_cop_ranges
                    .get(COP_NAME)
                    .map(|v| v.iter().any(|(di, s, e)| *di < idx && *s <= dir.line && *e >= dir.line))
                    .unwrap_or(false)
                    || all_ranges
                        .iter()
                        .any(|(di, s, e)| *di < idx && *s <= dir.line && *e >= dir.line);
                covers_us
            };
            // If the directive itself lists our cop, skip (user declares intent to silence us).
            let self_listed = dir.names.iter().any(|n| n.name == COP_NAME);
            if self_silenced_by_prior || self_listed {
                continue;
            }

            if dir.is_all {
                // `all` is redundant if no peer offense falls in its range at all.
                let range = all_ranges
                    .iter()
                    .find(|(di, _, _)| *di == idx)
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((dir.line, u32::MAX));
                let has_any = ctx
                    .peer_offenses
                    .iter()
                    .any(|o| o.location.line >= range.0 && o.location.line <= range.1);
                // RuboCop also suppresses `all` when a later specific disable follows,
                // because the specific one will catch redundancy if applicable.
                // We approximate: look for any later directive starting at range.end + 1
                // for some specific cop — but simpler: just use the has_any check.
                if !has_any {
                    per_dir_all_redundant.insert(idx, true);
                }
                continue;
            }

            // Per-cop redundancy in this directive.
            let mut redundant_names: Vec<DirName> = Vec::new();
            // Departments mentioned in THIS directive (those directly disable a whole dept).
            let depts_in_dir: HashSet<String> = dir
                .names
                .iter()
                .filter(|n| !n.name.contains('/') && KNOWN_DEPARTMENTS.contains(&n.name.as_str()))
                .map(|n| n.name.clone())
                .collect();

            // Depts covered by a prior active directive at dir.line (umbrella).
            let mut umbrella_depts: HashSet<String> = HashSet::new();
            for d in KNOWN_DEPARTMENTS.iter() {
                if let Some(v) = per_cop_ranges.get(*d) {
                    if v.iter().any(|(di, s, e)| *di < idx && *s <= dir.line && *e >= dir.line) {
                        umbrella_depts.insert((*d).to_string());
                    }
                }
            }

            for n in &dir.names {
                // Unknown/misspelled — always redundant.
                let known = self.known_cops.contains(&n.name)
                    || KNOWN_DEPARTMENTS.contains(&n.name.as_str())
                    || canonical_cop(&n.name, &self.known_cops).is_some();
                if !known {
                    redundant_names.push(n.clone());
                    continue;
                }

                // Re-disabling a cop after an explicit `enable` is always legitimate —
                // this mirrors RuboCop's `expected_final_disable?` exception which treats
                // a final disable of a config-disabled cop as intentional.
                if enabled_before[idx].contains(&n.name) {
                    continue;
                }

                // Determine this (idx,cop) range.
                let range = if let Some(v) = per_cop_ranges.get(&n.name) {
                    v.iter().find(|(di, _, _)| *di == idx).map(|(_, s, e)| (*s, *e))
                } else { None }.unwrap_or((dir.line, u32::MAX));

                let is_dept = !n.name.contains('/') && KNOWN_DEPARTMENTS.contains(&n.name.as_str());

                // "Already disabled by previous directive / dept umbrella":
                // - For a qualified cop `Dept/Name`: redundant if `Dept/Name` is actively
                //   disabled by a prior directive, OR `Dept` is actively disabled by
                //   a prior directive OR is in THIS directive (dept-covers-cop-in-same-line).
                //   Exception: if we previously `enable`d this cop, the re-disable is legitimate.
                // - For a department `Dept`: redundant if `Dept` is actively disabled by a
                //   prior directive. Being covered by `all` is OK: explicit dept disable
                //   may be legitimate after an all-disable.
                let already_covered = if is_dept {
                    umbrella_depts.contains(&n.name)
                } else if let Some((dept, _)) = n.name.split_once('/') {
                    let prior_same_cop = per_cop_ranges.get(&n.name).map_or(false, |v| {
                        v.iter().any(|(di, s, e)| *di < idx && *s <= dir.line && *e >= dir.line)
                    });
                    let prior_dept = umbrella_depts.contains(dept);
                    let same_line_dept = depts_in_dir.contains(dept);
                    let prev_enabled = enabled_before[idx].contains(&n.name);
                    (prior_same_cop || prior_dept || same_line_dept) && !prev_enabled
                } else {
                    false
                };

                if already_covered {
                    redundant_names.push(n.clone());
                    continue;
                }

                // Peer-offense check.
                let pred: Box<dyn Fn(&str) -> bool> = if is_dept {
                    let d = n.name.clone();
                    Box::new(move |c: &str| c.starts_with(&format!("{}/", d)))
                } else {
                    // Canonicalize abbreviated bare name to `Dept/Name` if possible.
                    let nm = canonical_cop(&n.name, &self.known_cops)
                        .unwrap_or_else(|| n.name.clone());
                    Box::new(move |c: &str| c == nm)
                };
                if !peer_hit_for(&pred, range.0, range.1) {
                    redundant_names.push(n.clone());
                }
            }

            if !redundant_names.is_empty() {
                per_dir_redundant.insert(idx, redundant_names);
            }
        }

        // ── Emit offenses ────────────────────────────────────────────────────
        let mut offenses = Vec::new();
        for (idx, dir) in directives.iter().enumerate() {
            if dir.enable {
                continue;
            }
            if dir.is_all {
                if per_dir_all_redundant.get(&idx).copied().unwrap_or(false) {
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        "Unnecessary disabling of all cops.",
                        Severity::Warning,
                        dir.comment_start,
                        dir.comment_end,
                    ));
                }
                continue;
            }

            let redundant = match per_dir_redundant.remove(&idx) {
                Some(r) => r,
                None => continue,
            };

            // Combined offense when ALL listed names are redundant.
            // Range spans whole directive; message lists all cop parts sorted alphabetically.
            if redundant.len() == dir.names.len() {
                let mut sorted: Vec<&DirName> = redundant.iter().collect();
                sorted.sort_by(|a, b| a.name.cmp(&b.name));
                let parts: Vec<String> =
                    sorted.iter().map(|n| self.format_cop_part(&n.name)).collect();
                let msg = format!("Unnecessary disabling of {}.", parts.join(", "));
                offenses.push(ctx.offense_with_range(
                    COP_NAME,
                    &msg,
                    Severity::Warning,
                    dir.comment_start,
                    dir.comment_end,
                ));
            } else {
                // Per-name offenses for just the redundant ones.
                for n in &redundant {
                    let part = self.format_cop_part(&n.name);
                    let msg = format!("Unnecessary disabling of {}.", part);
                    let end = n.start + n.name.len();
                    offenses.push(ctx.offense_with_range(
                        COP_NAME,
                        &msg,
                        Severity::Warning,
                        n.start,
                        end,
                    ));
                }
            }
        }

        offenses
    }
}

impl RedundantCopDisableDirective {
    /// Produce the `` `Cop/Name` `` / `` `Dept` department `` / `` `X` (unknown cop) `` /
    /// `` `X` (did you mean `Y`?) `` fragment used in messages.
    fn format_cop_part(&self, raw: &str) -> String {
        if let Some((dept, _)) = raw.split_once('/') {
            if KNOWN_DEPARTMENTS.contains(&dept) {
                if self.known_cops.contains(raw) {
                    return format!("`{}`", raw);
                }
                if let Some(sugg) = self.nearest_cop(raw) {
                    return format!("`{}` (did you mean `{}`?)", raw, sugg);
                }
                return format!("`{}` (unknown cop)", raw);
            }
            if let Some(sugg) = self.nearest_cop(raw) {
                return format!("`{}` (did you mean `{}`?)", raw, sugg);
            }
            return format!("`{}` (unknown cop)", raw);
        }
        if KNOWN_DEPARTMENTS.contains(&raw) {
            return format!("`{}` department", raw);
        }
        // Bare, not a department: if we can canonicalize to a known cop, emit that.
        if let Some(canon) = canonical_cop(raw, &self.known_cops) {
            return format!("`{}`", canon);
        }
        if let Some(sugg) = self.nearest_cop(raw) {
            return format!("`{}` (did you mean `{}`?)", raw, sugg);
        }
        format!("`{}` (unknown cop)", raw)
    }

    fn nearest_cop(&self, raw: &str) -> Option<String> {
        if self.known_cops.is_empty() {
            return None;
        }
        let mut best: Option<(usize, &String)> = None;
        let lower = raw.to_lowercase();
        for k in &self.known_cops {
            let d = levenshtein(&lower, &k.to_lowercase());
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, k));
            }
        }
        let (d, name) = best?;
        if d <= (raw.len() / 2).max(3) {
            Some(name.clone())
        } else {
            None
        }
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

// ── Registration ─────────────────────────────────────────────────────────────

fn collect_known_cops() -> HashSet<String> {
    let mut s: HashSet<String> = inventory::iter::<crate::cops::registry::Registration>
        .into_iter()
        .map(|r| r.name.to_string())
        .collect();
    for extra in &[
        "Lint/Syntax",
        "Lint/Debugger",
        "Lint/AmbiguousOperator",
        "Lint/SelfAssignment",
        "Lint/RedundantCopDisableDirective",
        "Lint/RedundantCopEnableDirective",
        "Metrics/MethodLength",
        "Metrics/ClassLength",
        "Metrics/BlockLength",
        "Metrics/AbcSize",
        "Layout/LineLength",
        "Layout/IndentationStyle",
        "Style/ClassVars",
    ] {
        s.insert((*extra).to_string());
    }
    s
}

crate::register_cop!("Lint/RedundantCopDisableDirective", |cfg| {
    let mut disabled_cops = HashSet::new();
    let mut disabled_depts = HashSet::new();
    for (name, cc) in &cfg.cops {
        if cc.enabled == Some(false) {
            if name.contains('/') {
                disabled_cops.insert(name.clone());
            } else {
                disabled_depts.insert(name.clone());
            }
        }
    }
    let known = collect_known_cops();
    Some(Box::new(RedundantCopDisableDirective::new(
        disabled_cops,
        disabled_depts,
        known,
    )))
});
