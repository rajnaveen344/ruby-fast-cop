//! Naming/InclusiveLanguage - flags configured non-inclusive terms.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/naming/inclusive_language.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use regex::{Regex, RegexBuilder};
use ruby_prism::Visit;
use serde_yaml::Value as YValue;

const COP_NAME: &str = "Naming/InclusiveLanguage";

/// One configured flagged term (e.g. `whitelist`).
struct Term {
    /// Display name (config key).
    term: String,
    /// Compiled per-term regex (case-insensitive).
    regex: Regex,
    /// Optional list of suggestions.
    suggestions: Vec<String>,
}

pub struct InclusiveLanguage {
    check_identifiers: bool,
    check_constants: bool,
    check_variables: bool,
    check_symbols: bool,
    check_strings: bool,
    check_comments: bool,
    check_filepaths: bool,
    terms: Vec<Term>,
    /// Combined regex of all flagged terms (alternation).
    flagged_regex: Option<Regex>,
    /// Combined AllowedRegex (mask) - case-insensitive.
    allowed_regex: Option<Regex>,
}

impl Default for InclusiveLanguage {
    fn default() -> Self {
        Self {
            check_identifiers: true,
            check_constants: true,
            check_variables: true,
            check_symbols: true,
            check_strings: false,
            check_comments: true,
            check_filepaths: true,
            terms: Vec::new(),
            flagged_regex: None,
            allowed_regex: None,
        }
    }
}

impl InclusiveLanguage {
    pub fn new() -> Self { Self::default() }

    fn build(
        check_identifiers: bool,
        check_constants: bool,
        check_variables: bool,
        check_symbols: bool,
        check_strings: bool,
        check_comments: bool,
        check_filepaths: bool,
        flagged_terms: &serde_yaml::Mapping,
    ) -> Self {
        let mut terms: Vec<Term> = Vec::new();
        let mut flagged_strings: Vec<String> = Vec::new();
        let mut allowed_strings: Vec<String> = Vec::new();

        for (k, v) in flagged_terms {
            let Some(term_name) = k.as_str() else { continue; };

            // term_definition.nil? -> skip
            // In TOML/YAML this can be a Mapping (with options) or null/string => skip.
            let map = match v {
                YValue::Mapping(m) => m,
                _ => continue,
            };

            // Extract AllowedRegex (string or array of strings)
            if let Some(allowed) = map.get(YValue::String("AllowedRegex".into())) {
                process_allowed_regex(allowed, &mut allowed_strings);
            }

            // Extract Regex (explicit) or build from term + WholeWord
            let regex_string = if let Some(r) = map.get(YValue::String("Regex".into())) {
                ensure_regex_string(r).unwrap_or_else(|| regex::escape(term_name))
            } else if map
                .get(YValue::String("WholeWord".into()))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                // Mirror RuboCop:  /(?:\b|(?<=[\W_]))#{term}(?:\b|(?=[\W_]))/
                // Rust regex has no look-around, so emulate equivalent for ASCII identifiers:
                // disallow letter/digit/underscore directly adjacent.
                format!(
                    r"(?:^|[^A-Za-z0-9])({})(?:$|[^A-Za-z0-9])",
                    regex::escape(term_name)
                )
            } else {
                regex::escape(term_name)
            };

            flagged_strings.push(regex_string.clone());

            let regex = match RegexBuilder::new(&regex_string).case_insensitive(true).build() {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Suggestions: nil | "" | [] | "single" | ["a"] | ["a","b"] ...
            let suggestions: Vec<String> = map
                .get(YValue::String("Suggestions".into()))
                .map(coerce_suggestions)
                .unwrap_or_default();

            terms.push(Term { term: term_name.to_string(), regex, suggestions });
        }

        let flagged_regex = if flagged_strings.is_empty() {
            None
        } else {
            RegexBuilder::new(&flagged_strings.join("|"))
                .case_insensitive(true)
                .build()
                .ok()
        };

        let allowed_regex = if allowed_strings.is_empty() {
            None
        } else {
            RegexBuilder::new(&allowed_strings.join("|"))
                .case_insensitive(true)
                .build()
                .ok()
        };

        Self {
            check_identifiers, check_constants, check_variables,
            check_symbols, check_strings, check_comments, check_filepaths,
            terms, flagged_regex, allowed_regex,
        }
    }

    /// Mask AllowedRegex matches (replace with `*` to preserve byte offsets) then scan.
    /// Returns Vec<(absolute_byte_offset, matched_text)>.
    fn scan_for_words(&self, text: &str, base_offset: usize) -> Vec<(usize, String)> {
        let Some(re) = &self.flagged_regex else { return Vec::new(); };

        // Mask
        let masked: String = if let Some(allow) = &self.allowed_regex {
            mask_allowed(text, allow)
        } else {
            text.to_string()
        };

        let mut out = Vec::new();
        for caps in re.captures_iter(&masked) {
            // Prefer first non-empty capture group (used by WholeWord emulation),
            // else fall back to whole-match group 0.
            let m = (1..caps.len())
                .find_map(|i| caps.get(i))
                .unwrap_or_else(|| caps.get(0).unwrap());
            let matched_text = &text[m.start()..m.end()];
            out.push((base_offset + m.start(), matched_text.to_string()));
        }
        out
    }

    fn find_term_for(&self, word: &str) -> Option<&Term> {
        // Mirror RuboCop's `find_flagged_term`: iterate insertion order and pick first regex match.
        for t in &self.terms {
            if t.regex.is_match(word) {
                return Some(t);
            }
        }
        None
    }

    fn create_message(&self, word: &str) -> String {
        let suffix = self
            .find_term_for(word)
            .map(|t| format_suggestions(&t.suggestions))
            .unwrap_or_else(|| " with another term".into());
        format!("Consider replacing '{}'{}.", word, suffix)
    }

    fn create_filepath_message_single(&self, word: &str) -> String {
        let suffix = self
            .find_term_for(word)
            .map(|t| format_suggestions(&t.suggestions))
            .unwrap_or_else(|| " with another term".into());
        format!("Consider replacing '{}' in file path{}.", word, suffix)
    }

    fn create_filepath_message_multi(&self, words: &[String]) -> String {
        let joined = words
            .iter()
            .map(|w| format!("'{}'", w))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Consider replacing {} in file path with other terms.", joined)
    }
}

fn coerce_suggestions(v: &YValue) -> Vec<String> {
    match v {
        YValue::String(s) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s.clone()]
            }
        }
        YValue::Sequence(seq) => seq
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn process_allowed_regex(v: &YValue, out: &mut Vec<String>) {
    match v {
        YValue::String(s) => {
            if !s.trim().is_empty() {
                out.push(s.clone());
            }
        }
        YValue::Sequence(seq) => {
            for x in seq {
                if let Some(s) = ensure_regex_string(x) {
                    if !s.trim().is_empty() {
                        out.push(s);
                    }
                }
            }
        }
        _ => {}
    }
}

fn ensure_regex_string(v: &YValue) -> Option<String> {
    match v {
        YValue::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn mask_allowed(text: &str, allowed: &Regex) -> String {
    // Replace each match with '*' * byte_length to preserve offsets.
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in allowed.find_iter(text) {
        out.push_str(&text[last..m.start()]);
        // byte length of match
        for _ in 0..(m.end() - m.start()) {
            out.push('*');
        }
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

/// Apply case transform from `original` to `suggestion`.
/// lower→lower, Title→Title, UPPER→UPPER, else return suggestion as-is.
fn apply_case_transform(original: &str, suggestion: &str) -> String {
    if original.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) && original.chars().any(|c| c.is_alphabetic()) {
        suggestion.to_uppercase()
    } else if original.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        let mut s = suggestion.to_string();
        if let Some(first) = s.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        s
    } else {
        suggestion.to_string()
    }
}

fn format_suggestions(suggestions: &[String]) -> String {
    if suggestions.is_empty() {
        return " with another term".to_string();
    }
    let quoted: Vec<String> = suggestions.iter().map(|w| format!("'{}'", w)).collect();
    let s = match quoted.len() {
        1 => quoted[0].clone(),
        2 => format!("{} or {}", quoted[0], quoted[1]),
        _ => {
            let last = quoted.last().unwrap();
            let rest = &quoted[..quoted.len() - 1];
            let mut joined = rest.join(", ");
            joined.push_str(", or ");
            joined.push_str(last);
            joined
        }
    };
    format!(" with {}", s)
}

impl Cop for InclusiveLanguage {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses: Vec<Offense> = Vec::new();
        if self.flagged_regex.is_none() {
            return offenses;
        }

        // 1. AST scan
        let mut v = Visitor { cop: self, ctx, offenses: &mut offenses };
        v.visit_program_node(node);

        // 2. Comments
        if self.check_comments {
            let parsed = ruby_prism::parse(ctx.source.as_bytes());
            for c in parsed.comments() {
                let loc = c.location();
                let text = &ctx.source[loc.start_offset()..loc.end_offset()];
                self.emit_for_text(text, loc.start_offset(), &mut offenses, ctx);
            }
        }

        // 3. Filepath
        if self.check_filepaths {
            self.emit_for_filepath(ctx, &mut offenses);
        }

        offenses
    }
}

impl InclusiveLanguage {
    fn emit_for_text(
        &self,
        text: &str,
        base: usize,
        offenses: &mut Vec<Offense>,
        ctx: &CheckContext,
    ) {
        for (off, word) in self.scan_for_words(text, base) {
            let end = off + word.len();
            let msg = self.create_message(&word);
            let offense = ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, off, end);
            // Add correction if exactly one suggestion
            let offense = if let Some(term) = self.find_term_for(&word) {
                if term.suggestions.len() == 1 {
                    let replacement = apply_case_transform(&word, &term.suggestions[0]);
                    offense.with_correction(Correction::replace(off, end, replacement))
                } else {
                    offense
                }
            } else {
                offense
            };
            offenses.push(offense);
        }
    }

    fn emit_for_filepath(&self, ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        // Use filename
        let filename = ctx.filename;
        // Scan filepath text.
        if self.flagged_regex.is_none() {
            return;
        }
        // Mask + scan
        let masked: String = if let Some(allow) = &self.allowed_regex {
            mask_allowed(filename, allow)
        } else {
            filename.to_string()
        };
        let re = self.flagged_regex.as_ref().unwrap();
        let words: Vec<String> = re
            .captures_iter(&masked)
            .map(|caps| {
                let m = (1..caps.len())
                    .find_map(|i| caps.get(i))
                    .unwrap_or_else(|| caps.get(0).unwrap());
                filename[m.start()..m.end()].to_string()
            })
            .collect();

        if words.is_empty() {
            return;
        }

        let msg = if words.len() == 1 {
            self.create_filepath_message_single(&words[0])
        } else {
            self.create_filepath_message_multi(&words)
        };

        // Global offense - line 1, col 0..1 (zero-width-widened)
        let prefixed = format!("{{}} {}", msg);
        offenses.push(ctx.offense_with_range(
            COP_NAME, &prefixed, Severity::Convention, 0, 0,
        ));
    }
}

struct Visitor<'a, 'src> {
    cop: &'a InclusiveLanguage,
    ctx: &'a CheckContext<'src>,
    offenses: &'a mut Vec<Offense>,
}

impl<'a, 'src> Visitor<'a, 'src> {
    fn scan_loc(&mut self, loc: ruby_prism::Location, kind: TokenKind) {
        let enabled = match kind {
            TokenKind::Identifier => self.cop.check_identifiers,
            TokenKind::Constant => self.cop.check_constants,
            TokenKind::Variable => self.cop.check_variables,
            TokenKind::Symbol => self.cop.check_symbols,
            TokenKind::String => self.cop.check_strings,
        };
        if !enabled { return; }
        let start = loc.start_offset();
        let end = loc.end_offset();
        if end > self.ctx.source.len() || start > end { return; }
        // Need to obtain &str slice safely (UTF-8 boundaries should be valid for AST locs).
        let text = match self.ctx.source.get(start..end) {
            Some(t) => t,
            None => return,
        };
        self.cop.emit_for_text(text, start, self.offenses, self.ctx);
    }
}

#[derive(Clone, Copy)]
enum TokenKind { Identifier, Constant, Variable, Symbol, String }

fn is_identifier_shaped(s: &str) -> bool {
    let mut chars = s.chars();
    let first = match chars.next() { Some(c) => c, None => return false };
    if !(first.is_ascii_alphabetic() || first == '_') { return false; }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '?' || c == '!' || c == '=') {
            return false;
        }
    }
    true
}

impl<'a, 'src> Visit<'src> for Visitor<'a, 'src> {
    // ── Constants ──
    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode<'src>) {
        self.scan_loc(node.location(), TokenKind::Constant);
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Constant);
        ruby_prism::visit_constant_write_node(self, node);
    }
    fn visit_constant_target_node(&mut self, node: &ruby_prism::ConstantTargetNode<'src>) {
        self.scan_loc(node.location(), TokenKind::Constant);
    }
    fn visit_constant_and_write_node(&mut self, node: &ruby_prism::ConstantAndWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Constant);
        ruby_prism::visit_constant_and_write_node(self, node);
    }
    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Constant);
        ruby_prism::visit_constant_or_write_node(self, node);
    }
    fn visit_constant_operator_write_node(&mut self, node: &ruby_prism::ConstantOperatorWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Constant);
        ruby_prism::visit_constant_operator_write_node(self, node);
    }
    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode<'src>) {
        // Only scan the rightmost name segment; recurse parent.
        self.scan_loc(node.name_loc(), TokenKind::Constant);
        ruby_prism::visit_constant_path_node(self, node);
    }

    // ── Local variables ──
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'src>) {
        self.scan_loc(node.location(), TokenKind::Identifier);
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'src>) {
        self.scan_loc(node.location(), TokenKind::Identifier);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    // ── Instance / class / global vars ──
    fn visit_instance_variable_read_node(&mut self, node: &ruby_prism::InstanceVariableReadNode<'src>) {
        // Skip leading `@` so col_start matches.
        let loc = node.location();
        let start = loc.start_offset() + 1;
        let end = loc.end_offset();
        if start <= end {
            let len = end - start;
            // Synthesize a slice scan
            if let Some(text) = self.ctx.source.get(start..end) {
                if self.cop.check_variables {
                    self.cop.emit_for_text(text, start, self.offenses, self.ctx);
                }
            }
            let _ = len;
        }
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'src>) {
        let loc = node.name_loc();
        let start = loc.start_offset() + 1; // skip `@`
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
        ruby_prism::visit_instance_variable_write_node(self, node);
    }
    fn visit_instance_variable_target_node(&mut self, node: &ruby_prism::InstanceVariableTargetNode<'src>) {
        let loc = node.location();
        let start = loc.start_offset() + 1;
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
    }
    fn visit_class_variable_read_node(&mut self, node: &ruby_prism::ClassVariableReadNode<'src>) {
        let loc = node.location();
        let start = loc.start_offset() + 2; // skip `@@`
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'src>) {
        let loc = node.name_loc();
        let start = loc.start_offset() + 2;
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
        ruby_prism::visit_class_variable_write_node(self, node);
    }
    fn visit_class_variable_target_node(&mut self, node: &ruby_prism::ClassVariableTargetNode<'src>) {
        let loc = node.location();
        let start = loc.start_offset() + 2;
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
    }
    fn visit_global_variable_read_node(&mut self, node: &ruby_prism::GlobalVariableReadNode<'src>) {
        let loc = node.location();
        let start = loc.start_offset() + 1; // skip `$`
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'src>) {
        let loc = node.name_loc();
        let start = loc.start_offset() + 1;
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
        ruby_prism::visit_global_variable_write_node(self, node);
    }
    fn visit_global_variable_target_node(&mut self, node: &ruby_prism::GlobalVariableTargetNode<'src>) {
        let loc = node.location();
        let start = loc.start_offset() + 1;
        let end = loc.end_offset();
        if let Some(text) = self.ctx.source.get(start..end) {
            if self.cop.check_variables {
                self.cop.emit_for_text(text, start, self.offenses, self.ctx);
            }
        }
    }

    // ── Method calls, defs, params (identifiers) ──
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'src>) {
        if let Some(msg) = node.message_loc() {
            // Only scan when the message_loc text is an identifier-shaped name
            // (skip operator/index calls like `[]`, `[]=`, `+`, `==`, ...).
            let s = msg.start_offset();
            let e = msg.end_offset();
            if let Some(text) = self.ctx.source.get(s..e) {
                if is_identifier_shaped(text) {
                    self.scan_loc(msg, TokenKind::Identifier);
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
        ruby_prism::visit_def_node(self, node);
    }
    fn visit_required_parameter_node(&mut self, node: &ruby_prism::RequiredParameterNode<'src>) {
        self.scan_loc(node.location(), TokenKind::Identifier);
    }
    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
        ruby_prism::visit_optional_parameter_node(self, node);
    }
    fn visit_required_keyword_parameter_node(&mut self, node: &ruby_prism::RequiredKeywordParameterNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
    }
    fn visit_optional_keyword_parameter_node(&mut self, node: &ruby_prism::OptionalKeywordParameterNode<'src>) {
        self.scan_loc(node.name_loc(), TokenKind::Identifier);
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
    }

    // ── Symbols ──
    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode<'src>) {
        if let Some(value_loc) = node.value_loc() {
            self.scan_loc(value_loc, TokenKind::Symbol);
        }
    }

    // ── Strings ──
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'src>) {
        let loc = node.content_loc();
        self.scan_loc(loc, TokenKind::String);
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'src>) {
        // Parts are visited by default; visit_string_node handles inner StringNode parts.
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

// ── Registration ──

crate::register_cop!("Naming/InclusiveLanguage", |cfg| {
    use serde_yaml::Value;

    let cop_config = cfg.get_cop_config(COP_NAME);
    let raw = cop_config.map(|c| &c.raw);

    let bool_or = |key: &str, default: bool| -> bool {
        raw.and_then(|m| m.get(key))
           .and_then(|v| v.as_bool())
           .unwrap_or(default)
    };

    let check_identifiers = bool_or("CheckIdentifiers", true);
    let check_constants   = bool_or("CheckConstants",   true);
    let check_variables   = bool_or("CheckVariables",   true);
    let check_symbols     = bool_or("CheckSymbols",     true);
    let check_strings     = bool_or("CheckStrings",     false);
    let check_comments    = bool_or("CheckComments",    true);
    let check_filepaths   = bool_or("CheckFilepaths",   true);

    let empty = serde_yaml::Mapping::new();
    let flagged: serde_yaml::Mapping = raw
        .and_then(|m| m.get("FlaggedTerms"))
        .and_then(|v| v.as_mapping().cloned())
        .unwrap_or(empty);

    let _ = Value::Null; // silence unused

    Some(Box::new(InclusiveLanguage::build(
        check_identifiers, check_constants, check_variables,
        check_symbols, check_strings, check_comments, check_filepaths,
        &flagged,
    )))
});
