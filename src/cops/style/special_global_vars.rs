//! Style/SpecialGlobalVars - prefer English lib names over Perl-style globals ($&, $`, etc).
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/special_global_vars.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    UseEnglishNames,
    UsePerlNames,
    UseBuiltinEnglishNames,
}

pub struct SpecialGlobalVars {
    style: EnforcedStyle,
    require_english: bool,
}

impl SpecialGlobalVars {
    pub fn new(style: EnforcedStyle, require_english: bool) -> Self {
        Self {
            style,
            require_english,
        }
    }
}

impl Default for SpecialGlobalVars {
    fn default() -> Self {
        Self::new(EnforcedStyle::UseEnglishNames, true)
    }
}

/// Perl-style var -> [english names, starting with library English, then regular (e.g., ARGV)].
/// First entry is the library English name (e.g., $PROCESS_ID); later are regular names like $PID / ARGV.
/// For "regular" we specifically mean NON_ENGLISH_VARS: $LOAD_PATH, $LOADED_FEATURES, $PROGRAM_NAME, ARGV.
const ENGLISH_MAP: &[(&str, &[&str])] = &[
    ("$:", &["$LOAD_PATH"]),
    ("$\"", &["$LOADED_FEATURES"]),
    ("$0", &["$PROGRAM_NAME"]),
    ("$!", &["$ERROR_INFO"]),
    ("$@", &["$ERROR_POSITION"]),
    ("$;", &["$FIELD_SEPARATOR", "$FS"]),
    ("$,", &["$OUTPUT_FIELD_SEPARATOR", "$OFS"]),
    ("$/", &["$INPUT_RECORD_SEPARATOR", "$RS"]),
    ("$\\", &["$OUTPUT_RECORD_SEPARATOR", "$ORS"]),
    ("$.", &["$INPUT_LINE_NUMBER", "$NR"]),
    ("$_", &["$LAST_READ_LINE"]),
    ("$>", &["$DEFAULT_OUTPUT"]),
    ("$<", &["$DEFAULT_INPUT"]),
    ("$$", &["$PROCESS_ID", "$PID"]),
    ("$?", &["$CHILD_STATUS"]),
    ("$~", &["$LAST_MATCH_INFO"]),
    ("$=", &["$IGNORECASE"]),
    ("$*", &["$ARGV", "ARGV"]),
];

/// Names that are NOT provided by the English library (they are builtin).
const NON_ENGLISH_VARS: &[&str] = &["$LOAD_PATH", "$LOADED_FEATURES", "$PROGRAM_NAME", "ARGV"];

fn is_non_english(v: &str) -> bool {
    NON_ENGLISH_VARS.contains(&v)
}

/// Given any global var name, return preferred names list for `style`.
/// If None returned: not a recognized special var (or already preferred per style).
fn preferred_names(style: EnforcedStyle, global: &str) -> Option<Vec<&'static str>> {
    match style {
        EnforcedStyle::UseEnglishNames => {
            // Perl name → English; English name → itself.
            if let Some((_, english)) = ENGLISH_MAP.iter().find(|(k, _)| *k == global) {
                return Some(english.to_vec());
            }
            // English name (e.g., $PROGRAM_NAME, $PROCESS_ID, $LOAD_PATH, ARGV, etc): itself.
            if is_known_english(global) {
                return Some(vec![static_name(global)]);
            }
            None
        }
        EnforcedStyle::UsePerlNames => {
            // English name → Perl.
            for (perl, englishes) in ENGLISH_MAP.iter() {
                if englishes.contains(&global) {
                    return Some(vec![*perl]);
                }
            }
            if is_perl_var(global) {
                return Some(vec![static_name(global)]);
            }
            None
        }
        EnforcedStyle::UseBuiltinEnglishNames => {
            // Builtin-English mode: only `$`-prefixed builtins are preferred over
            // Perl forms. Non-`$` constants (like `ARGV`) don't count, so `$*` is NOT
            // flagged (it has no `$`-prefix builtin counterpart, just `ARGV`).
            if let Some((_, englishes)) = ENGLISH_MAP.iter().find(|(k, _)| *k == global) {
                if let Some(builtin) = englishes
                    .iter()
                    .find(|e| is_non_english(e) && e.starts_with('$'))
                {
                    return Some(vec![*builtin]);
                }
                return None;
            }
            // English library name → flag, prefer builtin ($-prefixed) form.
            if is_known_english(global) {
                for (_, englishes) in ENGLISH_MAP.iter() {
                    if englishes.contains(&global) {
                        if let Some(builtin) = englishes
                            .iter()
                            .find(|e| is_non_english(e) && e.starts_with('$'))
                        {
                            return Some(vec![*builtin]);
                        }
                        // Already the builtin itself → preferred.
                        return Some(vec![static_name(global)]);
                    }
                }
            }
            None
        }
    }
}

fn is_perl_var(v: &str) -> bool {
    ENGLISH_MAP.iter().any(|(k, _)| *k == v)
}

fn is_known_english(v: &str) -> bool {
    ENGLISH_MAP.iter().any(|(_, vs)| vs.contains(&v))
}

/// Return the `&'static str` matching the given name (lookup in our tables).
fn static_name(v: &str) -> &'static str {
    for (k, vs) in ENGLISH_MAP {
        if *k == v {
            return k;
        }
        for n in vs.iter() {
            if *n == v {
                return n;
            }
        }
    }
    // Fallback — shouldn't happen for recognized vars.
    ""
}

fn format_list(items: &[&str]) -> String {
    items.join("` or `")
}

fn format_message(style: EnforcedStyle, global: &str) -> String {
    match style {
        EnforcedStyle::UseEnglishNames => {
            let (_, all) = match ENGLISH_MAP.iter().find(|(k, _)| *k == global) {
                Some(e) => e,
                None => {
                    // Already-English: shouldn't produce a message.
                    return String::new();
                }
            };
            let (regular, english): (Vec<&str>, Vec<&str>) =
                all.iter().partition(|v| is_non_english(v));
            if regular.is_empty() {
                format!(
                    "Prefer `{}` from the stdlib 'English' module (don't forget to require it) over `{}`.",
                    format_list(&english),
                    global
                )
            } else if english.is_empty() {
                format!("Prefer `{}` over `{}`.", format_list(&regular), global)
            } else {
                format!(
                    "Prefer `{}` from the stdlib 'English' module (don't forget to require it) or `{}` over `{}`.",
                    format_list(&english),
                    format_list(&regular),
                    global
                )
            }
        }
        _ => {
            let preferred = preferred_names(style, global).map(|v| v[0]).unwrap_or("");
            format!("Prefer `{}` over `{}`.", preferred, global)
        }
    }
}

// ── Visitor ──

struct Visitor<'a> {
    cop: &'a SpecialGlobalVars,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    // Track insertion of `require 'English'`: track whether we've emitted.
    // RuboCop relies on `RequireLibrary`; we approximate by inserting a single `require 'English'`
    // before the first statement that needs it, for each file, for the FIRST offense only.
    require_english_inserted: bool,
}

impl<'a> Visitor<'a> {
    /// Handles a gvar inside string/regexp interpolation. `outer_range` is the
    /// byte span of the whole `#...` or `#{...}` — used when we need to replace
    /// the whole interpolation (e.g. `#{$LOAD_PATH}` → `#$:`).
    /// `is_shorthand`: true if `#$var`, false if `#{$var}`.
    fn check_in_interp(
        &mut self,
        name: &[u8],
        loc: &ruby_prism::Location,
        outer_range: (usize, usize),
        is_shorthand: bool,
    ) {
        let name_str = String::from_utf8_lossy(name).to_string();
        if name_str.len() >= 2
            && name_str.as_bytes()[1].is_ascii_digit()
            && name_str.as_bytes()[1] != b'0'
        {
            return;
        }

        let preferred = match preferred_names(self.cop.style, &name_str) {
            Some(p) => p,
            None => return,
        };
        if preferred.contains(&name_str.as_str()) {
            return;
        }

        let message = format_message(self.cop.style, &name_str);
        if message.is_empty() {
            return;
        }

        let mut offense = self
            .ctx
            .offense(self.cop.name(), &message, Severity::Convention, loc);

        let preferred_first = preferred[0];
        let replacement = match self.cop.style {
            EnforcedStyle::UseEnglishNames => {
                // Always use `#{$ENGLISH_NAME}` form
                format!("#{{{}}}", preferred_first)
            }
            EnforcedStyle::UsePerlNames | EnforcedStyle::UseBuiltinEnglishNames => {
                // Preferred perl-style; use shorthand `#$:` form.
                format!("#{}", preferred_first)
            }
        };
        let _ = is_shorthand;
        let edit = Edit {
            start_offset: outer_range.0,
            end_offset: outer_range.1,
            replacement,
        };

        offense = offense.with_correction(Correction { edits: vec![edit] });
        self.offenses.push(offense);
    }

    fn check(&mut self, name: &[u8], loc: &ruby_prism::Location) {
        let name_str = String::from_utf8_lossy(name).to_string();
        // Skip backrefs like $1..$9 — not perl-style special vars.
        // `$0` IS special (program name) and must be flagged.
        if name_str.len() >= 2
            && name_str.as_bytes()[1].is_ascii_digit()
            && name_str.as_bytes()[1] != b'0'
        {
            return;
        }

        let preferred = match preferred_names(self.cop.style, &name_str) {
            Some(p) => p,
            None => return,
        };
        if preferred.contains(&name_str.as_str()) {
            return;
        }

        let message = format_message(self.cop.style, &name_str);
        if message.is_empty() {
            return;
        }

        let mut offense = self.ctx.offense(
            self.cop.name(),
            &message,
            Severity::Convention,
            loc,
        );

        // Build correction — simple identifier swap.
        let preferred_first = preferred[0];
        let start = loc.start_offset();
        let end = loc.end_offset();
        let mut edits: Vec<Edit> = vec![Edit {
            start_offset: start,
            end_offset: end,
            replacement: preferred_first.to_string(),
        }];

        // If style is use_english_names with require_english, add `require 'English'` once.
        if self.cop.style == EnforcedStyle::UseEnglishNames
            && self.cop.require_english
            && !self.require_english_inserted
            && !is_non_english(preferred_first)
            && !self.file_already_requires_english()
        {
            let insert_off = self.insertion_offset_for_require();
            edits.insert(
                0,
                Edit {
                    start_offset: insert_off,
                    end_offset: insert_off,
                    replacement: "require 'English'\n".to_string(),
                },
            );
            self.require_english_inserted = true;
        }
        // If file already requires English (or has `require 'English'` anywhere),
        // but AT OR BELOW our use site, we move it above. For simplicity, detect presence
        // after use site and remove original, then insert above.
        else if self.cop.style == EnforcedStyle::UseEnglishNames
            && self.cop.require_english
            && !is_non_english(preferred_first)
            && !self.require_english_inserted
        {
            // Check if require is below; if yes, move it up.
            if let Some((r_start, r_end)) = self.find_require_english_after(start) {
                let insert_off = self.insertion_offset_for_require();
                edits.insert(
                    0,
                    Edit {
                        start_offset: insert_off,
                        end_offset: insert_off,
                        replacement: "require 'English'\n".to_string(),
                    },
                );
                // Remove existing require (and trailing newline).
                let mut rm_end = r_end;
                if rm_end < self.ctx.source.len()
                    && self.ctx.source.as_bytes()[rm_end] == b'\n'
                {
                    rm_end += 1;
                }
                edits.push(Edit {
                    start_offset: r_start,
                    end_offset: rm_end,
                    replacement: String::new(),
                });
                self.require_english_inserted = true;
            }
        }

        offense = offense.with_correction(Correction { edits });
        self.offenses.push(offense);
    }

    fn file_already_requires_english(&self) -> bool {
        // Scan source for `require 'English'` or `require "English"`.
        let src = self.ctx.source;
        src.contains("require 'English'") || src.contains("require \"English\"")
    }

    fn find_require_english_after(&self, after_off: usize) -> Option<(usize, usize)> {
        let src = self.ctx.source;
        let needles = ["require 'English'", "require \"English\""];
        for needle in &needles {
            if let Some(p) = src.find(needle) {
                if p > after_off {
                    return Some((p, p + needle.len()));
                }
            }
        }
        None
    }

    fn insertion_offset_for_require(&self) -> usize {
        // Skip leading magic comments / frozen_string_literal / encoding / shebang / blank lines.
        let src = self.ctx.source;
        let mut off = 0;
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("#!")
                || trimmed.starts_with("# frozen_string_literal")
                || trimmed.starts_with("# encoding")
                || trimmed.is_empty()
            {
                off += line.len() + 1; // +1 for \n
                continue;
            }
            break;
        }
        off
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_global_variable_read_node(&mut self, node: &ruby_prism::GlobalVariableReadNode) {
        self.check(node.name().as_slice(), &node.location());
        ruby_prism::visit_global_variable_read_node(self, node);
    }

    fn visit_numbered_reference_read_node(
        &mut self,
        node: &ruby_prism::NumberedReferenceReadNode,
    ) {
        // $0 is special (program name); $1..$9 are backrefs. Prism may parse `$0` as
        // NumberedReferenceReadNode. Handle the `$0` case here.
        let loc = node.location();
        let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        if src == "$0" {
            self.check(b"$0", &loc);
        }
        ruby_prism::visit_numbered_reference_read_node(self, node);
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.check(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_write_node(self, node);
    }
    fn visit_global_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOperatorWriteNode,
    ) {
        self.check(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_operator_write_node(self, node);
    }
    fn visit_global_variable_and_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableAndWriteNode,
    ) {
        self.check(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_and_write_node(self, node);
    }
    fn visit_global_variable_or_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOrWriteNode,
    ) {
        self.check(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_or_write_node(self, node);
    }

    // For interpolated strings/regexps: `#$:` is an embedded global var reference
    // without explicit `#{...}`. The source has `#$:` and Prism models this as
    // an EmbeddedVariableNode inside InterpolatedStringNode. We handle the
    // enclosed gvar here and DO NOT recurse, to avoid the default visitor
    // double-reporting via visit_global_variable_read_node.
    fn visit_embedded_variable_node(&mut self, node: &ruby_prism::EmbeddedVariableNode) {
        let var = node.variable();
        // The `#` byte is at node.location().start_offset(). Replacement for
        // use_english_names should turn `#$:` into `#{$LOAD_PATH}`;
        // for use_perl_names it stays `#$:` (already). Our `check` detects and
        // replaces node-local.
        let outer_loc = node.location();
        let outer_range = (outer_loc.start_offset(), outer_loc.end_offset());
        if let Node::GlobalVariableReadNode { .. } = var {
            let gv = var.as_global_variable_read_node().unwrap();
            self.check_in_interp(
                gv.name().as_slice(),
                &gv.location(),
                outer_range,
                true, // shorthand form
            );
        } else if let Node::NumberedReferenceReadNode { .. } = var {
            let nr = var.as_numbered_reference_read_node().unwrap();
            let loc = nr.location();
            let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
            if src == "$0" {
                self.check_in_interp(b"$0", &loc, outer_range, true);
            }
        }
        // Intentionally do NOT call ruby_prism::visit_embedded_variable_node here.
    }

    // `"#{$var}"` in an interpolated string: if the embedded statements are just a
    // single gvar, treat specially so we can emit `"#$var"` shorthand for perl style.
    fn visit_embedded_statements_node(
        &mut self,
        node: &ruby_prism::EmbeddedStatementsNode,
    ) {
        // Default visit first; but we need special correction. Check body.
        if let Some(body) = node.statements() {
            let stmts: Vec<_> = body.body().iter().collect();
            if stmts.len() == 1 {
                let outer_loc = node.location();
                let outer_range = (outer_loc.start_offset(), outer_loc.end_offset());
                if let Node::GlobalVariableReadNode { .. } = &stmts[0] {
                    let gv = stmts[0].as_global_variable_read_node().unwrap();
                    self.check_in_interp(
                        gv.name().as_slice(),
                        &gv.location(),
                        outer_range,
                        false, // full #{...} form
                    );
                    return; // don't recurse
                }
                if let Node::NumberedReferenceReadNode { .. } = &stmts[0] {
                    let nr = stmts[0].as_numbered_reference_read_node().unwrap();
                    let loc = nr.location();
                    let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
                    if src == "$0" {
                        self.check_in_interp(b"$0", &loc, outer_range, false);
                        return;
                    }
                }
            }
        }
        ruby_prism::visit_embedded_statements_node(self, node);
    }
}

impl Cop for SpecialGlobalVars {
    fn name(&self) -> &'static str {
        "Style/SpecialGlobalVars"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            require_english_inserted: false,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/SpecialGlobalVars", |cfg| {
    let cop_config = cfg.get_cop_config("Style/SpecialGlobalVars");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "use_perl_names" => EnforcedStyle::UsePerlNames,
            "use_builtin_english_names" => EnforcedStyle::UseBuiltinEnglishNames,
            _ => EnforcedStyle::UseEnglishNames,
        })
        .unwrap_or(EnforcedStyle::UseEnglishNames);
    let require_english = cop_config
        .and_then(|c| c.raw.get("RequireEnglish"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(SpecialGlobalVars::new(style, require_english)))
});
