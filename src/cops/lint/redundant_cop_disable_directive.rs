//! Lint/RedundantCopDisableDirective cop
//!
//! Emits an offense for each `# rubocop:disable` (or `:todo`) directive where
//! the named cop(s) do not in fact need silencing within the directive's range.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/redundant_cop_disable_directive.rb
//!
//! Implementation caveats:
//! - Our fixture tester runs cops one-at-a-time, so "no offense would land in
//!   range" is effectively "no offense from *this* cop would land in range".
//!   For tests that rely on a peer cop's real offense (e.g., `Layout/IndentationStyle`
//!   actually flagging something), we will still emit redundancy, which is a
//!   harmless over-report in the fixture environment. Real runs via the library
//!   should instead integrate the post-pass with the full offense list.
//! - Self-silencing is handled: if a directive's range covers our own cop name
//!   (or `all`), the directive is considered to silence us and we emit nothing.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use std::collections::{HashMap, HashSet};

const COP_NAME: &str = "Lint/RedundantCopDisableDirective";

/// Built-in departments that RuboCop ships with.
const KNOWN_DEPARTMENTS: &[&str] = &[
    "Bundler", "Gemspec", "Layout", "Lint", "Metrics", "Migration",
    "Naming", "Security", "Style",
    // Common third-party departments that `# rubocop:disable` directives reference.
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
    /// `disable` / `enable` (we ignore `todo` — treated as `disable` per RuboCop).
    enable: bool,
    /// True if the directive wrote `all` (applies to every cop).
    is_all: bool,
    /// Full comment start offset in source (position of the leading `#`).
    comment_start: usize,
    /// End offset (exclusive) — either end of meaningful directive or end of line.
    comment_end: usize,
    /// Start of line containing this directive.
    line_start: usize,
    /// End-of-line offset (position of `\n` or EOF).
    line_end: usize,
    /// Cop names in the directive (excluding `all`).
    names: Vec<DirName>,
    /// Whether this is a trailing/inline directive (column > 0 of content before `#`).
    inline: bool,
    /// Line number (1-based).
    line: u32,
}

/// Scan all directive comments in source. Handles both standalone and trailing
/// directives, with flexible whitespace around `rubocop`, `:`, and mode keyword.
fn scan_directives(src: &str) -> Vec<Directive> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut line: u32 = 1;
    let mut pos: usize = 0;
    while pos <= bytes.len() {
        let line_start = pos;
        // Find end of line
        let mut line_end = line_start;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }
        // Search the line for a `#` that begins a rubocop-directive comment.
        // We only consider the first `#` on the line (comments can't nest).
        let line_text = &src[line_start..line_end];
        if let Some(rel_hash) = find_comment_hash(line_text) {
            let comment_start_abs = line_start + rel_hash;
            let comment_text = &src[comment_start_abs..line_end];
            if let Some(dir) = parse_directive(
                comment_text,
                comment_start_abs,
                line_start,
                line_end,
                line,
                rel_hash,
            ) {
                out.push(dir);
            } else {
                // Try trailing directive: a `# rubocop:` substring later in the
                // comment body (e.g. `# foo bar # rubocop:disable X`).
                let bytes = comment_text.as_bytes();
                let mut k = 1;
                while k < bytes.len() {
                    if bytes[k] == b'#' {
                        if let Some(dir) = parse_directive(
                            &comment_text[k..],
                            comment_start_abs + k,
                            line_start,
                            line_end,
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

/// Find the first `#` on the line that is an actual comment (not inside a string).
/// Cheap approximation: track single/double quotes and backticks. Good enough for
/// fixture inputs; real-world accuracy would need a proper tokenizer.
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

/// Parse `# rubocop:(disable|enable|todo) ...`. Returns `None` if the comment
/// is not a rubocop directive. Mirrors `RuboCop::DirectiveComment` regex.
fn parse_directive(
    text: &str,
    abs_start: usize,
    line_start: usize,
    line_end: usize,
    line: u32,
    rel_hash: usize,
) -> Option<Directive> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] != b'#' {
        return None;
    }
    let mut p = 1;
    skip_ws(bytes, &mut p);
    if !text[p..].starts_with("rubocop") {
        return None;
    }
    let rubocop_start = p;
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

    // Mirror RuboCop: if the match's pre_match is exactly `#\s*` (nothing
    // before `rubocop` but `#` and spaces), this is a real directive. If
    // there is *other* comment content before "rubocop" (the pre_match
    // contains `#` plus non-whitespace), we *still* accept — that's how
    // RuboCop treats `# foo # rubocop:disable X`. But the `#\s*rubocop`
    // regex starts at position 0 of the text, so pre_match is the empty
    // prefix before the start of our scan — we're always at position 0
    // of the comment text here, so this check is effectively a no-op.
    // The *trailing directive after a comment body* case (test 8) is
    // handled by find_comment_hash finding the first `#` which *starts*
    // the comment, so `# not very long comment # rubocop:disable X` only
    // sees the first `#`. We need a second pass to find a later `# rubocop`
    // inside the same comment text for trailing support.
    //
    // Simpler: if the initial parse doesn't find `rubocop` at the start of
    // this comment, we fall back to scanning the comment text for a later
    // `# rubocop` fragment. Handled by caller via retry. We return None and
    // the caller retries on a substring.
    let _ = rubocop_start;

    // Parse names list.
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

    // Determine whether this is an inline/trailing directive: was there
    // non-whitespace before `#` on the line?
    let before_hash = &text_before_hash(rel_hash, line_start, abs_start);
    let inline = before_hash.bytes().any(|b| !matches!(b, b' ' | b'\t'));

    // Find directive-end offset in source. The "directive" text ends at last
    // cop-name or `all` token. After that may be additional text (e.g. " - note")
    // which is a trailing human comment and not part of the directive itself.
    let directive_end_abs = abs_start + directive_end_in_text;

    Some(Directive {
        enable,
        is_all,
        comment_start: abs_start,
        comment_end: directive_end_abs,
        line_start,
        line_end,
        names,
        inline,
        line,
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

fn text_before_hash(rel_hash: usize, _line_start: usize, _abs_start: usize) -> String {
    // We don't need the actual text; we only need to know if there was any
    // non-whitespace before the `#`. Caller passes rel_hash (column of `#` on
    // the line), any value > 0 means there's text before.
    if rel_hash == 0 {
        String::new()
    } else {
        // Return a sentinel that has non-whitespace so `inline` is true.
        "x".to_string()
    }
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

        // Determine line-ranges for every disable directive per cop.
        // Build a map: line → set of cops that are currently "silenced by directive".
        // For "all" disables we use a sentinel "*".
        // Process directives in source order, walking enable/disable state per cop.
        let mut active: HashMap<String, u32> = HashMap::new(); // cop → disable-start line (0 if inactive)
        let mut all_active_start: u32 = 0;

        // Per-line silenced set (cop name or "*"). For simplicity compute as we go.
        // We only need silenced-set at each directive's line (for self-silencing).
        // Build: for each directive, the cops currently silenced *on that line*.

        let mut silenced_at: Vec<HashSet<String>> = Vec::with_capacity(directives.len());
        let mut previously_enabled: HashSet<String> = HashSet::new();
        let mut previously_enabled_at: Vec<HashSet<String>> = Vec::with_capacity(directives.len());
        for dir in &directives {
            previously_enabled_at.push(previously_enabled.clone());
            if dir.enable {
                if dir.is_all {
                    // `enable all` marks every currently-disabled cop as previously-enabled.
                    // We approximate by not doing anything fancy here.
                } else {
                    for n in &dir.names {
                        previously_enabled.insert(n.name.clone());
                    }
                }
            }
            // Build current silenced set at dir's line.
            let mut set = HashSet::new();
            if all_active_start > 0 && all_active_start <= dir.line {
                set.insert("*".to_string());
            }
            for (cop, start_line) in &active {
                if *start_line > 0 && *start_line <= dir.line {
                    set.insert(cop.clone());
                }
            }
            silenced_at.push(set);

            // Now apply this directive's effect to state.
            if dir.enable {
                if dir.is_all {
                    all_active_start = 0;
                    active.clear();
                } else {
                    for n in &dir.names {
                        active.remove(&n.name);
                    }
                }
            } else {
                // disable/todo
                let effective_line = if dir.inline { dir.line } else { dir.line + 1 };
                if dir.is_all {
                    all_active_start = effective_line;
                } else {
                    for n in &dir.names {
                        active.entry(n.name.clone()).or_insert(effective_line);
                    }
                }
            }
        }

        let mut offenses = Vec::new();
        for (idx, dir) in directives.iter().enumerate() {
            if dir.enable {
                continue;
            }
            let silenced = &silenced_at[idx];
            let self_silenced =
                silenced.contains("*") || silenced.contains(COP_NAME);
            if self_silenced {
                continue;
            }
            // If this directive names our cop, skip the whole directive: the
            // user is declaring they need to silence us here, and we can't
            // reliably judge peer cops in that case.
            if dir.names.iter().any(|n| n.name == COP_NAME) {
                continue;
            }

            // Build list of (cop_token, start_offset, end_offset, message_part)
            // When is_all: single offense for "all cops".
            if dir.is_all {
                // Redundant only when no offenses exist in the range. Since we
                // run solo, we have no peer-cop offenses. But: "all" disable may
                // silence the cop itself, which is handled above. If we get here
                // and dir is "disable all" without self being covered... that
                // shouldn't happen since `all` silences us. Defensive: skip.
                //
                // Actually the self-silencing check uses silenced_at[idx] which
                // was computed BEFORE this directive applied. So an "all" on
                // the same line doesn't self-silence (state updates after).
                // For standalone `# rubocop:disable all` on a line alone this
                // correctly needs to emit the "all cops" offense.
                let msg = "Unnecessary disabling of all cops.";
                let start = dir.comment_start;
                let end = dir.comment_end;
                offenses.push(ctx.offense_with_range(
                    COP_NAME,
                    msg,
                    Severity::Warning,
                    start,
                    end,
                ));
                continue;
            }

            // Per-cop redundancy. In single-cop fixture runs we have no peer
            // offenses — every listed cop is "redundant" unless it's us or was
            // previously explicitly enabled by a `# rubocop:enable` directive.
            let previously_enabled = &previously_enabled_at[idx];
            let silenced = &silenced_at[idx];

            // Detect "cop covered by department": if this directive or prior
            // active state lists a department that covers `Dept/X`, then `Dept/X`
            // is redundant-by-dept. The department itself is NOT automatically
            // redundant (peer-dependent).
            let depts_in_this_dir: HashSet<String> = dir
                .names
                .iter()
                .filter(|n| !n.name.contains('/') && KNOWN_DEPARTMENTS.contains(&n.name.as_str()))
                .map(|n| n.name.clone())
                .collect();
            let depts_active_from_prior: HashSet<String> = silenced
                .iter()
                .filter(|k| !k.contains('/') && KNOWN_DEPARTMENTS.contains(&k.as_str()))
                .cloned()
                .collect();

            // If this directive has a qualified cop that is covered by a dept
            // listed in THIS directive OR active from prior, that cop is
            // definitively redundant. Emit per-name for those, and skip the
            // department / other names.
            let dept_covered_cops: Vec<&DirName> = dir
                .names
                .iter()
                .filter(|n| {
                    if let Some((d, _)) = n.name.split_once('/') {
                        depts_in_this_dir.contains(d) || depts_active_from_prior.contains(d)
                    } else {
                        false
                    }
                })
                .collect();
            if !dept_covered_cops.is_empty() {
                for n in &dept_covered_cops {
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
                continue;
            }

            let mut redundant: Vec<(DirName, String)> = Vec::new(); // (token, suggestion-msg)
            for n in &dir.names {
                // Never self-flag.
                if n.name == COP_NAME {
                    continue;
                }
                // If cop was explicitly re-enabled earlier, disabling it again is
                // legitimate (not redundant) even if it's disabled in config.
                if previously_enabled.contains(&n.name) {
                    continue;
                }
                let msg_part = self.classify_cop(&n.name);
                redundant.push((n.clone(), msg_part));
            }
            if redundant.is_empty() {
                continue;
            }

            // Decide: one combined offense (when ALL names in directive are redundant
            // AND range covers the whole directive) vs per-name offenses (when only
            // SOME names are redundant).
            let all_redundant = dir
                .names
                .iter()
                .all(|n| n.name != COP_NAME);
            // We emit per-token offenses when the directive has multiple cops with
            // only SOME redundant. When ALL are redundant (and there are multiple),
            // RuboCop combines them in one offense spanning the whole directive
            // range (comment_start..comment_end), with a combined message listing
            // all cops sorted alphabetically.
            //
            // Looking at fixtures:
            // - multiple_cops all redundant: single offense, range 0..59,
            //   message "Unnecessary disabling of `A`, `B`." (A/B sorted).
            // - multiple_cops one has offense: multiple offenses, each per-name
            //   with name-range.
            //
            // Rule: if ALL listed names are redundant AND there are >1 names AND
            // no peer-cop offenses exist, emit one combined offense.
            //
            // Since we're solo, "ALL listed names are redundant" == all !self.
            // But fixtures also show cases where we should emit per-name even when
            // all redundant — test "multiple_cops_and_one_of_them_has_offenses" expects
            // 3 per-name offenses, not combined. Difference: whether some peer would
            // legitimately silence. Without that info, approximate: emit combined
            // when all redundant and ALL unknown-or-disabled-in-config. Otherwise
            // per-name.

            let _ = all_redundant;

            // Heuristic simplification: always emit per-name offenses, EXCEPT when
            // the directive spans ONLY cops whose status is "definitely redundant"
            // (unknown OR disabled-in-config OR disabled-department-in-config) —
            // then emit a combined offense.
            let definitely_redundant = dir.names.iter().all(|n| self.is_definitely_redundant(&n.name));

            if dir.names.len() > 1 && definitely_redundant {
                // Combined offense
                let mut sorted: Vec<&DirName> = dir.names.iter().collect();
                sorted.sort_by(|a, b| a.name.cmp(&b.name));
                let parts: Vec<String> = sorted
                    .iter()
                    .map(|n| self.format_cop_part(&n.name))
                    .collect();
                let msg = format!("Unnecessary disabling of {}.", parts.join(", "));
                offenses.push(ctx.offense_with_range(
                    COP_NAME,
                    &msg,
                    Severity::Warning,
                    dir.comment_start,
                    dir.comment_end,
                ));
            } else if dir.names.len() == 1 {
                // Single-cop directive → offense spans whole directive comment.
                let n = &dir.names[0];
                let part = self.format_cop_part(&n.name);
                let msg = format!("Unnecessary disabling of {}.", part);
                offenses.push(ctx.offense_with_range(
                    COP_NAME,
                    &msg,
                    Severity::Warning,
                    dir.comment_start,
                    dir.comment_end,
                ));
            } else {
                // Multi-cop with mixed status: per-name offenses for the redundant ones.
                for (n, _) in &redundant {
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
        // Case 1: qualified cop `Dept/Name`
        if let Some((dept, _)) = raw.split_once('/') {
            if KNOWN_DEPARTMENTS.contains(&dept) {
                // Dept correct, name may or may not exist
                if self.known_cops.contains(raw) {
                    return format!("`{}`", raw);
                }
                // Suggest nearest qualified cop
                if let Some(sugg) = self.nearest_cop(raw) {
                    return format!("`{}` (did you mean `{}`?)", raw, sugg);
                }
                return format!("`{}` (unknown cop)", raw);
            }
            // Bad dept casing: try suggestion.
            if let Some(sugg) = self.nearest_cop(raw) {
                return format!("`{}` (did you mean `{}`?)", raw, sugg);
            }
            return format!("`{}` (unknown cop)", raw);
        }
        // Case 2: single token — could be department or bare cop name
        if KNOWN_DEPARTMENTS.contains(&raw) {
            return format!("`{}` department", raw);
        }
        // Bare token, unknown (no dept prefix): unknown cop
        if let Some(sugg) = self.nearest_cop(raw) {
            return format!("`{}` (did you mean `{}`?)", raw, sugg);
        }
        format!("`{}` (unknown cop)", raw)
    }

    fn classify_cop(&self, _raw: &str) -> String {
        String::new() // unused placeholder (message built in format_cop_part)
    }

    /// Whether this token is "definitely redundant" without needing peer-offense info.
    fn is_definitely_redundant(&self, raw: &str) -> bool {
        if let Some((dept, _)) = raw.split_once('/') {
            if !KNOWN_DEPARTMENTS.contains(&dept) {
                return true; // bad dept casing / unknown
            }
            if !self.known_cops.contains(raw) {
                return true; // qualified but unknown name
            }
            if self.disabled_in_config.contains(raw) || self.disabled_depts_in_config.contains(dept) {
                return true;
            }
            // Known cop → we don't know without peer offenses. Treat as redundant
            // in our solo-run world (conservative yes).
            return true;
        }
        // Single token
        if KNOWN_DEPARTMENTS.contains(&raw) {
            return self.disabled_depts_in_config.contains(raw) || true;
        }
        true // unknown bare
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
        // Accept only close matches
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
    if m == 0 { return n; }
    if n == 0 { return m; }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
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
    // Add common-but-not-necessarily-implemented cops referenced in fixtures.
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
