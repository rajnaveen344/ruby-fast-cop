//! Style/HashSyntax - Checks hash literal syntax.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/hash_syntax.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

/// Enforced style for hash syntax
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Ruby19,
    HashRockets,
    NoMixedKeys,
    Ruby19NoMixedKeys,
}

/// Enforced shorthand syntax style (Ruby 3.1+ hash value omission)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedShorthandSyntax {
    Always,
    Never,
    Either,
    Consistent,
    EitherConsistent,
}

pub struct HashSyntax {
    enforced_style: EnforcedStyle,
    enforced_shorthand_syntax: EnforcedShorthandSyntax,
    use_hash_rockets_with_symbol_values: bool,
    prefer_hash_rockets_for_non_alnum_ending_symbols: bool,
}

impl HashSyntax {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self {
            enforced_style,
            enforced_shorthand_syntax: EnforcedShorthandSyntax::Either,
            use_hash_rockets_with_symbol_values: false,
            prefer_hash_rockets_for_non_alnum_ending_symbols: true,
        }
    }

    pub fn with_config(
        enforced_style: EnforcedStyle,
        enforced_shorthand_syntax: EnforcedShorthandSyntax,
        use_hash_rockets_with_symbol_values: bool,
        prefer_hash_rockets_for_non_alnum_ending_symbols: bool,
    ) -> Self {
        Self {
            enforced_style,
            enforced_shorthand_syntax,
            use_hash_rockets_with_symbol_values,
            prefer_hash_rockets_for_non_alnum_ending_symbols,
        }
    }

    /// Core logic: check a list of AssocNode elements for key style and shorthand offenses.
    fn check_pairs(
        &self,
        elements: &[ruby_prism::Node],
        ctx: &CheckContext,
        modifier_call_context: bool,
    ) -> Vec<Offense> {
        let mut offenses = Vec::new();

        // Collect assoc nodes
        let assocs: Vec<ruby_prism::AssocNode> = elements
            .iter()
            .filter_map(|e| {
                if let ruby_prism::Node::AssocNode { .. } = e {
                    Some(e.as_assoc_node().unwrap())
                } else {
                    None
                }
            })
            .collect();

        if assocs.is_empty() {
            return offenses;
        }

        let assoc_refs: Vec<&ruby_prism::AssocNode> = assocs.iter().collect();

        // Check key style (ruby19 vs hash_rockets vs no_mixed)
        self.check_key_style(&assoc_refs, ctx, &mut offenses);

        // Check shorthand syntax (skip if in modifier call context where shorthand is unsafe)
        if !modifier_call_context {
            self.check_shorthand_syntax(&assoc_refs, ctx, &mut offenses);
        }

        offenses
    }

    fn check_key_style(
        &self,
        assocs: &[&ruby_prism::AssocNode],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        // If UseHashRocketsWithSymbolValues is true, check if any value is a symbol
        if self.use_hash_rockets_with_symbol_values
            && matches!(self.enforced_style, EnforcedStyle::Ruby19 | EnforcedStyle::Ruby19NoMixedKeys)
        {
            let has_symbol_value = assocs.iter().any(|a| {
                matches!(a.value(), ruby_prism::Node::SymbolNode { .. })
            });
            if has_symbol_value {
                // Flag all ruby19-style pairs to use hash rockets
                for assoc in assocs {
                    if assoc.operator_loc().is_none() && !self.is_shorthand_pair(assoc) {
                        let key = assoc.key();
                        offenses.push(ctx.offense_with_range(
                            "Style/HashSyntax",
                            "Use hash rockets syntax.",
                            Severity::Convention,
                            key.location().start_offset(),
                            key.location().end_offset(),
                        ));
                    }
                }
                return;
            }
        }

        match self.enforced_style {
            EnforcedStyle::Ruby19 => {
                self.check_ruby19_style(assocs, ctx, offenses);
            }
            EnforcedStyle::HashRockets => {
                self.check_hash_rockets_style(assocs, ctx, offenses);
            }
            EnforcedStyle::NoMixedKeys => {
                self.check_no_mixed_keys(assocs, ctx, offenses);
            }
            EnforcedStyle::Ruby19NoMixedKeys => {
                self.check_ruby19_no_mixed_keys(assocs, ctx, offenses);
            }
        }
    }

    /// EnforcedStyle: ruby19 — flag symbol keys using hash rockets that could use ruby19
    fn check_ruby19_style(
        &self,
        assocs: &[&ruby_prism::AssocNode],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        // If any key is a non-symbol using rockets, don't flag symbol keys
        // (mixed key types justify rocket syntax for consistency)
        let has_non_symbol_rocket = assocs.iter().any(|a| {
            a.operator_loc().is_some()
                && !matches!(a.key(), ruby_prism::Node::SymbolNode { .. })
        });
        if has_non_symbol_rocket {
            return;
        }

        for assoc in assocs {
            if assoc.operator_loc().is_some() {
                // Has hash rocket — check if this symbol key can use ruby19
                if self.can_use_ruby19_for_style(&assoc.key(), ctx) {
                    let (start, end) = self.key_operator_range(assoc, ctx);
                    offenses.push(ctx.offense_with_range(
                        "Style/HashSyntax",
                        "Use the new Ruby 1.9 hash syntax.",
                        Severity::Convention,
                        start,
                        end,
                    ));
                }
            }
        }
    }

    /// Check if a symbol key can be converted for the current EnforcedStyle.
    /// For ruby19: accepts simple identifiers AND quoted symbols (:"foo" → "foo":)
    /// For ruby19_no_mixed_keys: same but handled separately
    fn can_use_ruby19_for_style(&self, key: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        if let ruby_prism::Node::SymbolNode { .. } = key {
            let key_loc = key.location();
            let key_text = ctx
                .source
                .get(key_loc.start_offset()..key_loc.end_offset())
                .unwrap_or("");

            if key_text.starts_with(":\"") || key_text.starts_with(":'") {
                // Quoted symbol — can use "key": syntax (requires Ruby >= 2.2)
                return ctx.ruby_version_at_least(2, 2);
            }
            if key_text.starts_with(':') {
                let identifier = &key_text[1..];
                return self.is_valid_ruby19_key(identifier);
            }
        }
        false
    }

    /// EnforcedStyle: hash_rockets — flag ruby19 style pairs
    fn check_hash_rockets_style(
        &self,
        assocs: &[&ruby_prism::AssocNode],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        for assoc in assocs {
            if assoc.operator_loc().is_none() && !self.is_shorthand_pair(assoc) {
                let key = assoc.key();
                offenses.push(ctx.offense_with_range(
                    "Style/HashSyntax",
                    "Use hash rockets syntax.",
                    Severity::Convention,
                    key.location().start_offset(),
                    key.location().end_offset(),
                ));
            }
        }
    }

    /// EnforcedStyle: no_mixed_keys — flag when styles are mixed within a hash
    fn check_no_mixed_keys(
        &self,
        assocs: &[&ruby_prism::AssocNode],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        let mut has_ruby19 = false;
        let mut has_rocket = false;
        let mut has_non_symbol_rocket = false;
        let mut first_style_is_rocket = false;
        let mut first_set = false;

        for assoc in assocs {
            let is_rocket = assoc.operator_loc().is_some();
            if is_rocket {
                has_rocket = true;
                if !matches!(assoc.key(), ruby_prism::Node::SymbolNode { .. }) {
                    has_non_symbol_rocket = true;
                }
                if !first_set {
                    first_style_is_rocket = true;
                    first_set = true;
                }
            } else {
                // Both ruby19 and shorthand count as "non-rocket" style
                has_ruby19 = true;
                if !first_set {
                    first_style_is_rocket = false;
                    first_set = true;
                }
            }
        }

        if has_ruby19 && has_rocket {
            // If non-symbol keys force rockets, rocket style wins
            let rocket_wins = has_non_symbol_rocket || first_style_is_rocket;

            for assoc in assocs {
                let is_rocket = assoc.operator_loc().is_some();
                if rocket_wins && !is_rocket {
                    // Ruby19 pair should use rockets
                    let key = assoc.key();
                    offenses.push(ctx.offense_with_range(
                        "Style/HashSyntax",
                        "Don't mix styles in the same hash.",
                        Severity::Convention,
                        key.location().start_offset(),
                        key.location().end_offset(),
                    ));
                } else if !rocket_wins && is_rocket {
                    // Rocket pair should use ruby19
                    let (start, end) = self.key_operator_range(assoc, ctx);
                    offenses.push(ctx.offense_with_range(
                        "Style/HashSyntax",
                        "Don't mix styles in the same hash.",
                        Severity::Convention,
                        start,
                        end,
                    ));
                }
            }
        }
    }

    /// EnforcedStyle: ruby19_no_mixed_keys — prefer ruby19, but if non-symbol keys
    /// force hash rockets, don't mix styles
    fn check_ruby19_no_mixed_keys(
        &self,
        assocs: &[&ruby_prism::AssocNode],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        let mut has_ruby19 = false;
        let mut has_rocket = false;
        let mut has_non_symbol_rocket = false;
        let mut has_complex_symbol_key = false;

        for assoc in assocs {
            if self.is_shorthand_pair(assoc) {
                continue;
            }
            if assoc.operator_loc().is_some() {
                has_rocket = true;
                let key = assoc.key();
                if !matches!(key, ruby_prism::Node::SymbolNode { .. }) {
                    has_non_symbol_rocket = true;
                } else if !self.can_use_simple_ruby19(&key, ctx) {
                    has_complex_symbol_key = true;
                }
            } else {
                has_ruby19 = true;
            }
        }

        // Count non-shorthand pairs
        let pair_count = assocs
            .iter()
            .filter(|a| !self.is_shorthand_pair(a))
            .count();

        if has_non_symbol_rocket {
            // Non-symbol keys force hash rockets for consistency
            // Flag any ruby19-style pairs as "Don't mix"
            for assoc in assocs {
                if assoc.operator_loc().is_none() && !self.is_shorthand_pair(assoc) {
                    let key = assoc.key();
                    offenses.push(ctx.offense_with_range(
                        "Style/HashSyntax",
                        "Don't mix styles in the same hash.",
                        Severity::Convention,
                        key.location().start_offset(),
                        key.location().end_offset(),
                    ));
                }
            }
        } else if has_complex_symbol_key && pair_count > 1 && !has_ruby19 {
            // Multiple keys, all rockets, one is complex → accept (consistent rockets)
            // Do nothing — rockets are justified for consistency
        } else if has_complex_symbol_key && has_ruby19 {
            // Complex symbol key with rockets + simple ruby19 pairs = mixed
            // Flag the ruby19 pairs as "Don't mix"
            for assoc in assocs {
                if assoc.operator_loc().is_none() && !self.is_shorthand_pair(assoc) {
                    let key = assoc.key();
                    offenses.push(ctx.offense_with_range(
                        "Style/HashSyntax",
                        "Don't mix styles in the same hash.",
                        Severity::Convention,
                        key.location().start_offset(),
                        key.location().end_offset(),
                    ));
                }
            }
        } else {
            // All keys are simple symbols — flag rockets for ruby19 conversion
            for assoc in assocs {
                if assoc.operator_loc().is_some() {
                    if let ruby_prism::Node::SymbolNode { .. } = &assoc.key() {
                        let (start, end) = self.key_operator_range(assoc, ctx);
                        offenses.push(ctx.offense_with_range(
                            "Style/HashSyntax",
                            "Use the new Ruby 1.9 hash syntax.",
                            Severity::Convention,
                            start,
                            end,
                        ));
                    }
                }
            }
        }
    }

    /// Check shorthand syntax (Ruby 3.1+ hash value omission)
    fn check_shorthand_syntax(
        &self,
        assocs: &[&ruby_prism::AssocNode],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        // Shorthand syntax requires Ruby 3.1+
        if !ctx.ruby_version_at_least(3, 1) {
            return;
        }
        match self.enforced_shorthand_syntax {
            EnforcedShorthandSyntax::Either => {}
            EnforcedShorthandSyntax::Always => {
                for assoc in assocs {
                    if self.can_omit_hash_value(assoc, ctx) {
                        // Offense on the value
                        let value = assoc.value();
                        offenses.push(ctx.offense_with_range(
                            "Style/HashSyntax",
                            "Omit the hash value.",
                            Severity::Convention,
                            value.location().start_offset(),
                            value.location().end_offset(),
                        ));
                    }
                }
            }
            EnforcedShorthandSyntax::Never => {
                for assoc in assocs {
                    if self.is_shorthand_pair(assoc) {
                        // Offense on the key's value_loc (symbol name without colon)
                        let (start, end) = self.symbol_name_range(&assoc.key(), ctx);
                        offenses.push(ctx.offense_with_range(
                            "Style/HashSyntax",
                            "Include the hash value.",
                            Severity::Convention,
                            start,
                            end,
                        ));
                    }
                }
            }
            EnforcedShorthandSyntax::Consistent
            | EnforcedShorthandSyntax::EitherConsistent => {
                self.check_consistent_shorthand(assocs, ctx, offenses);
            }
        }
    }

    /// Check consistent shorthand: don't mix implicit and explicit hash values
    fn check_consistent_shorthand(
        &self,
        assocs: &[&ruby_prism::AssocNode],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        let mut shorthand_pairs = Vec::new();
        let mut explicit_pairs_that_could_omit = Vec::new();
        let mut has_explicit_that_cannot_omit = false;

        for assoc in assocs {
            if self.is_shorthand_pair(assoc) {
                shorthand_pairs.push(*assoc);
            } else if self.can_omit_hash_value(assoc, ctx) {
                explicit_pairs_that_could_omit.push(*assoc);
            } else if assoc.operator_loc().is_none() {
                // Explicit pair where key != value (can't omit)
                has_explicit_that_cannot_omit = true;
            }
        }

        let has_shorthand = !shorthand_pairs.is_empty();
        let has_explicit = !explicit_pairs_that_could_omit.is_empty() || has_explicit_that_cannot_omit;

        if !has_shorthand || !has_explicit {
            // Not mixing — but check if all explicit could be omitted
            if !has_shorthand
                && !explicit_pairs_that_could_omit.is_empty()
                && !has_explicit_that_cannot_omit
            {
                // All values present, all could be omitted → "Omit the hash value."
                // Only for "consistent" mode (prefers shorthand). "either_consistent" allows
                // all-explicit as a valid consistent choice.
                if self.enforced_shorthand_syntax == EnforcedShorthandSyntax::Consistent {
                    for assoc in &explicit_pairs_that_could_omit {
                        let value = assoc.value();
                        offenses.push(ctx.offense_with_range(
                            "Style/HashSyntax",
                            "Omit the hash value.",
                            Severity::Convention,
                            value.location().start_offset(),
                            value.location().end_offset(),
                        ));
                    }
                }
            }
            return;
        }

        // Mixing shorthand and explicit — determine which direction to go
        let all_can_omit = !has_explicit_that_cannot_omit;

        if all_can_omit {
            // All pairs CAN be shorthand → flag the explicit ones to omit
            for assoc in &explicit_pairs_that_could_omit {
                let value = assoc.value();
                offenses.push(ctx.offense_with_range(
                    "Style/HashSyntax",
                    "Do not mix explicit and implicit hash values. Omit the hash value.",
                    Severity::Convention,
                    value.location().start_offset(),
                    value.location().end_offset(),
                ));
            }
        } else {
            // Some pairs can't be shorthand → flag the shorthand ones to include
            for assoc in &shorthand_pairs {
                let (start, end) = self.symbol_name_range(&assoc.key(), ctx);
                offenses.push(ctx.offense_with_range(
                    "Style/HashSyntax",
                    "Do not mix explicit and implicit hash values. Include the hash value.",
                    Severity::Convention,
                    start,
                    end,
                ));
            }
        }
    }

    /// Check if a symbol key can use simple ruby19 syntax (key: value)
    /// This is for EnforcedStyle::Ruby19 only — simple identifiers only
    fn can_use_simple_ruby19(&self, key: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        if let ruby_prism::Node::SymbolNode { .. } = key {
            let key_loc = key.location();
            let key_text = ctx
                .source
                .get(key_loc.start_offset()..key_loc.end_offset())
                .unwrap_or("");

            // Quoted symbols like :"foo-bar" can't use simple ruby19
            if key_text.starts_with(":\"") || key_text.starts_with(":'") {
                return false;
            }

            if key_text.starts_with(':') {
                let identifier = &key_text[1..];
                return self.is_valid_ruby19_key(identifier);
            }
        }
        false
    }

    /// Check if a string is a valid identifier for ruby19 syntax
    /// Matches: [a-zA-Z_]\w*[?!]?
    fn is_valid_ruby19_key(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let bytes = s.as_bytes();
        let first = bytes[0];
        if !first.is_ascii_alphabetic() && first != b'_' {
            return false;
        }
        let last = *bytes.last().unwrap();
        let check_end = if last == b'?' || last == b'!' {
            if self.prefer_hash_rockets_for_non_alnum_ending_symbols {
                return false;
            }
            bytes.len() - 1
        } else if last == b'=' {
            // = ending symbols always use hash rockets
            return false;
        } else {
            bytes.len()
        };
        for &b in &bytes[1..check_end] {
            if !b.is_ascii_alphanumeric() && b != b'_' {
                return false;
            }
        }
        true
    }

    /// Get the byte range covering key through operator (`:a =>`) for ruby19-style offenses
    fn key_operator_range(&self, assoc: &ruby_prism::AssocNode, _ctx: &CheckContext) -> (usize, usize) {
        let start = assoc.key().location().start_offset();
        if let Some(op) = assoc.operator_loc() {
            (start, op.end_offset())
        } else {
            (start, assoc.key().location().end_offset())
        }
    }

    /// Get the symbol name range (without leading `:` or trailing `:` in ruby19 syntax)
    fn symbol_name_range(&self, key: &ruby_prism::Node, ctx: &CheckContext) -> (usize, usize) {
        if let ruby_prism::Node::SymbolNode { .. } = key {
            let sym = key.as_symbol_node().unwrap();
            if let Some(val_loc) = sym.value_loc() {
                return (val_loc.start_offset(), val_loc.end_offset());
            }
        }
        let loc = key.location();
        let key_text = ctx.source.get(loc.start_offset()..loc.end_offset()).unwrap_or("");
        if key_text.starts_with(':') {
            (loc.start_offset() + 1, loc.end_offset())
        } else {
            (loc.start_offset(), loc.end_offset())
        }
    }

    /// Check if this pair uses shorthand (value is ImplicitNode)
    fn is_shorthand_pair(&self, assoc: &ruby_prism::AssocNode) -> bool {
        matches!(assoc.value(), ruby_prism::Node::ImplicitNode { .. })
    }

    /// Check if this pair can be converted to shorthand (key name == value name)
    fn can_omit_hash_value(&self, assoc: &ruby_prism::AssocNode, _ctx: &CheckContext) -> bool {
        // Must not already be shorthand
        if self.is_shorthand_pair(assoc) {
            return false;
        }
        // Must be ruby19 style (no hash rocket)
        if assoc.operator_loc().is_some() {
            return false;
        }
        // Key must be a symbol
        let key = assoc.key();
        if !matches!(key, ruby_prism::Node::SymbolNode { .. }) {
            return false;
        }
        let sym = key.as_symbol_node().unwrap();
        let key_name = String::from_utf8_lossy(sym.unescaped().as_ref()).to_string();

        // Key name ending with ? or ! can't use shorthand (foo?: is invalid syntax)
        if key_name.ends_with('?') || key_name.ends_with('!') {
            return false;
        }

        // Value must be a simple reference with the same name
        let value = assoc.value();
        match &value {
            ruby_prism::Node::LocalVariableReadNode { .. } => {
                let lvar = value.as_local_variable_read_node().unwrap();
                let val_name = String::from_utf8_lossy(lvar.name().as_slice());
                key_name == val_name.as_ref()
            }
            ruby_prism::Node::CallNode { .. } => {
                let call = value.as_call_node().unwrap();
                // Must be a bare method call (no receiver, no args, no block)
                if call.receiver().is_some()
                    || call.arguments().is_some()
                    || call.block().is_some()
                {
                    return false;
                }
                let val_name = String::from_utf8_lossy(call.name().as_slice());
                key_name == val_name.as_ref()
            }
            _ => false,
        }
    }

    /// Check if a KeywordHashNode is inside an unparenthesized call with a modifier condition.
    /// Examples where shorthand would be unsafe:
    ///   `foo value: value if bar`   — shorthand `foo value: if bar` is ambiguous
    ///   `baz if foo bar: bar`       — shorthand `baz if foo bar:` is ambiguous
    ///   `return foo value: value if bar`
    /// Check if a KeywordHashNode is inside an unparenthesized call with a modifier condition.
    /// In modifier form (`foo value: value if bar`), shorthand would be ambiguous.
    /// In block form (`if foo bar: bar\n  baz\nend`), shorthand is safe (autocorrect adds parens).
    fn is_modifier_call_context(
        &self,
        node: &ruby_prism::KeywordHashNode,
        ctx: &CheckContext,
    ) -> bool {
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        // Get the text before the hash on the same line
        let before = ctx.source.get(..node_start).unwrap_or("");
        let before_on_line = before.rsplit('\n').next().unwrap_or(before);

        // If there's an opening parenthesis before the hash on this line,
        // shorthand is safe: `foo(value: value) if bar` → `foo(value:) if bar`
        if before_on_line.contains('(') {
            return false;
        }

        // Check for modifier keyword AFTER the hash on the same line
        let after = ctx.source.get(node_end..).unwrap_or("");
        let after_on_line = after.split('\n').next().unwrap_or("");
        let after_trimmed = after_on_line.trim();

        let has_modifier_after = Self::starts_with_modifier_keyword(after_trimmed);
        if has_modifier_after {
            return true;
        }

        // Check for modifier keyword BEFORE the hash on the same line
        // (e.g., `baz if foo bar: bar` — the `if` is before `foo bar: bar`)
        // A modifier keyword mid-line (not at the start of the line) indicates modifier form
        let line_trimmed = before_on_line.trim();
        if !line_trimmed.is_empty() {
            // Check if the line starts with a non-keyword word followed by a modifier keyword
            // This catches `baz if foo bar: bar` pattern
            for keyword in &["if ", "unless ", "while ", "until "] {
                if let Some(pos) = line_trimmed.find(keyword) {
                    // Only count as modifier if there's non-whitespace content before the keyword
                    let before_keyword = line_trimmed[..pos].trim();
                    if !before_keyword.is_empty() {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check if text starts with a modifier keyword (if/unless/while/until).
    fn starts_with_modifier_keyword(text: &str) -> bool {
        for keyword in &["if", "unless", "while", "until"] {
            if text.starts_with(keyword) {
                let after_pos = keyword.len();
                if after_pos >= text.len() || text.as_bytes()[after_pos] == b' ' {
                    return true;
                }
            }
        }
        false
    }
}

impl Default for HashSyntax {
    fn default() -> Self {
        Self::new(EnforcedStyle::Ruby19)
    }
}

impl Cop for HashSyntax {
    fn name(&self) -> &'static str {
        "Style/HashSyntax"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        let elements: Vec<_> = node.elements().iter().collect();
        self.check_pairs(&elements, ctx, false)
    }

    fn check_keyword_hash(
        &self,
        node: &ruby_prism::KeywordHashNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let elements: Vec<_> = node.elements().iter().collect();
        // Check if this keyword hash is in an unparenthesized call with a modifier condition.
        // In that context, shorthand `value:` instead of `value: value` would be ambiguous
        // because Ruby could interpret the modifier keyword as the hash value.
        // e.g., `foo value: value if bar` → `foo value: if bar` is ambiguous.
        let modifier_context = self.is_modifier_call_context(node, ctx);
        self.check_pairs(&elements, ctx, modifier_context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_style(source: &str, style: EnforcedStyle) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(HashSyntax::new(style));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_style(source, EnforcedStyle::Ruby19NoMixedKeys)
    }

    #[test]
    fn allows_ruby19_syntax() {
        let offenses = check("{a: 1, b: 2}");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_hash_rocket_for_symbol_keys() {
        let offenses = check("{:a => 1}");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("Ruby 1.9"));
    }

    #[test]
    fn allows_hash_rocket_for_string_keys() {
        let offenses = check("{'a' => 1}");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_mixed_styles() {
        let offenses = check("{a: 1, 'b' => 2}");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("mix"));
    }

    #[test]
    fn hash_rockets_style_flags_ruby19() {
        let offenses = check_with_style("{a: 1}", EnforcedStyle::HashRockets);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("hash rockets"));
    }

    #[test]
    fn no_mixed_keys_allows_consistent_ruby19() {
        let offenses = check_with_style("{a: 1, b: 2}", EnforcedStyle::NoMixedKeys);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn no_mixed_keys_allows_consistent_hash_rockets() {
        let offenses = check_with_style("{:a => 1, :b => 2}", EnforcedStyle::NoMixedKeys);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn no_mixed_keys_flags_mixed() {
        let offenses = check_with_style("{a: 1, :b => 2}", EnforcedStyle::NoMixedKeys);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("mix"));
    }

    #[test]
    fn allows_quoted_symbol_with_hash_rocket() {
        // :"foo-bar" cannot use simple ruby19 syntax under ruby19_no_mixed_keys
        // But since it's the only pair, no mixing → just check if can convert
        // Under ruby19_no_mixed_keys, ALL symbol keys get flagged
        let offenses = check("{:\"foo-bar\" => 1}");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("Ruby 1.9"));
    }
}
