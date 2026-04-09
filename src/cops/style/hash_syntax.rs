//! Style/HashSyntax - Checks hash literal syntax.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Ruby19,
    HashRockets,
    NoMixedKeys,
    Ruby19NoMixedKeys,
}

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
        Self { enforced_style, enforced_shorthand_syntax, use_hash_rockets_with_symbol_values, prefer_hash_rockets_for_non_alnum_ending_symbols }
    }

    fn check_pairs(
        &self,
        elements: &[ruby_prism::Node],
        ctx: &CheckContext,
        modifier_call_context: bool,
        paren_positions: Option<(usize, usize)>,
    ) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let assocs: Vec<ruby_prism::AssocNode> = elements.iter()
            .filter_map(|e| if let ruby_prism::Node::AssocNode { .. } = e { Some(e.as_assoc_node().unwrap()) } else { None })
            .collect();
        if assocs.is_empty() { return offenses; }
        let assoc_refs: Vec<&ruby_prism::AssocNode> = assocs.iter().collect();

        self.check_key_style(&assoc_refs, ctx, &mut offenses);
        if !modifier_call_context {
            self.check_shorthand_syntax(&assoc_refs, ctx, &mut offenses, paren_positions);
        }
        offenses
    }

    fn check_key_style(&self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        if self.use_hash_rockets_with_symbol_values
            && matches!(self.enforced_style, EnforcedStyle::Ruby19 | EnforcedStyle::Ruby19NoMixedKeys)
        {
            if assocs.iter().any(|a| matches!(a.value(), ruby_prism::Node::SymbolNode { .. })) {
                for assoc in assocs {
                    if assoc.operator_loc().is_none() && !self.is_shorthand_pair(assoc) {
                        let key = assoc.key();
                        let mut offense = self.key_offense(ctx, "Use hash rockets syntax.", &key);
                        if let Some(c) = self.ruby19_to_rocket_correction(assoc, ctx) { offense = offense.with_correction(c); }
                        offenses.push(offense);
                    }
                }
                return;
            }
        }

        match self.enforced_style {
            EnforcedStyle::Ruby19 => self.check_ruby19_style(assocs, ctx, offenses),
            EnforcedStyle::HashRockets => self.check_hash_rockets_style(assocs, ctx, offenses),
            EnforcedStyle::NoMixedKeys => self.check_no_mixed_keys(assocs, ctx, offenses),
            EnforcedStyle::Ruby19NoMixedKeys => self.check_ruby19_no_mixed_keys(assocs, ctx, offenses),
        }
    }

    fn key_offense(&self, ctx: &CheckContext, msg: &str, key: &ruby_prism::Node) -> Offense {
        ctx.offense_with_range("Style/HashSyntax", msg, Severity::Convention, key.location().start_offset(), key.location().end_offset())
    }

    fn check_ruby19_style(&self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        let has_non_symbol_rocket = assocs.iter().any(|a| {
            a.operator_loc().is_some() && !matches!(a.key(), ruby_prism::Node::SymbolNode { .. })
        });
        if has_non_symbol_rocket { return; }

        for assoc in assocs {
            if assoc.operator_loc().is_some() && self.can_use_ruby19_for_style(&assoc.key(), ctx) {
                let (start, end) = self.key_operator_range(assoc);
                let mut offense = ctx.offense_with_range("Style/HashSyntax", "Use the new Ruby 1.9 hash syntax.", Severity::Convention, start, end);
                if let Some(c) = self.rocket_to_ruby19_correction(assoc, ctx) { offense = offense.with_correction(c); }
                offenses.push(offense);
            }
        }
    }

    fn can_use_ruby19_for_style(&self, key: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        if let ruby_prism::Node::SymbolNode { .. } = key {
            let key_text = self.key_text(key, ctx);
            if key_text.starts_with(":\"") || key_text.starts_with(":'") {
                return ctx.ruby_version_at_least(2, 2);
            }
            if key_text.starts_with(':') {
                return self.is_valid_ruby19_key(&key_text[1..]);
            }
        }
        false
    }

    fn check_hash_rockets_style(&self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        for assoc in assocs {
            if assoc.operator_loc().is_none() && !self.is_shorthand_pair(assoc) {
                let mut offense = self.key_offense(ctx, "Use hash rockets syntax.", &assoc.key());
                if let Some(c) = self.ruby19_to_rocket_correction(assoc, ctx) { offense = offense.with_correction(c); }
                offenses.push(offense);
            }
        }
    }

    fn check_no_mixed_keys(&self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        let mut has_ruby19 = false;
        let mut has_rocket = false;
        let mut has_non_symbol_rocket = false;
        let mut first_style_is_rocket = false;
        let mut first_set = false;

        for assoc in assocs {
            let is_rocket = assoc.operator_loc().is_some();
            if is_rocket {
                has_rocket = true;
                if !matches!(assoc.key(), ruby_prism::Node::SymbolNode { .. }) { has_non_symbol_rocket = true; }
            } else {
                has_ruby19 = true;
            }
            if !first_set { first_style_is_rocket = is_rocket; first_set = true; }
        }

        if !(has_ruby19 && has_rocket) { return; }
        let rocket_wins = has_non_symbol_rocket || first_style_is_rocket;

        for assoc in assocs {
            let is_rocket = assoc.operator_loc().is_some();
            if rocket_wins && !is_rocket {
                let mut offense = self.key_offense(ctx, "Don't mix styles in the same hash.", &assoc.key());
                let correction = if self.is_shorthand_pair(assoc) {
                    self.shorthand_to_rocket_correction(assoc, ctx)
                } else {
                    self.ruby19_to_rocket_correction(assoc, ctx)
                };
                if let Some(c) = correction { offense = offense.with_correction(c); }
                offenses.push(offense);
            } else if !rocket_wins && is_rocket {
                let (start, end) = self.key_operator_range(assoc);
                let mut offense = ctx.offense_with_range("Style/HashSyntax", "Don't mix styles in the same hash.", Severity::Convention, start, end);
                if let Some(c) = self.rocket_to_ruby19_correction(assoc, ctx) { offense = offense.with_correction(c); }
                offenses.push(offense);
            }
        }
    }

    fn check_ruby19_no_mixed_keys(&self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        let mut has_ruby19 = false;
        let mut has_non_symbol_rocket = false;
        let mut has_complex_symbol_key = false;

        for assoc in assocs {
            if self.is_shorthand_pair(assoc) { continue; }
            if assoc.operator_loc().is_some() {
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

        let pair_count = assocs.iter().filter(|a| !self.is_shorthand_pair(a)).count();

        if has_non_symbol_rocket || (has_complex_symbol_key && has_ruby19) {
            // Flag ruby19 pairs as "Don't mix"
            self.flag_ruby19_pairs_as_mixed(assocs, ctx, offenses);
        } else if has_complex_symbol_key && pair_count > 1 && !has_ruby19 {
            // All rockets with complex key — consistent, accept
        } else {
            // All keys are simple symbols — flag rockets
            for assoc in assocs {
                if assoc.operator_loc().is_some() {
                    if let ruby_prism::Node::SymbolNode { .. } = &assoc.key() {
                        let (start, end) = self.key_operator_range(assoc);
                        let mut offense = ctx.offense_with_range("Style/HashSyntax", "Use the new Ruby 1.9 hash syntax.", Severity::Convention, start, end);
                        if let Some(c) = self.rocket_to_ruby19_correction(assoc, ctx) { offense = offense.with_correction(c); }
                        offenses.push(offense);
                    }
                }
            }
        }
    }

    fn flag_ruby19_pairs_as_mixed(&self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        for assoc in assocs {
            if assoc.operator_loc().is_none() && !self.is_shorthand_pair(assoc) {
                let mut offense = self.key_offense(ctx, "Don't mix styles in the same hash.", &assoc.key());
                if let Some(c) = self.ruby19_to_rocket_correction(assoc, ctx) { offense = offense.with_correction(c); }
                offenses.push(offense);
            }
        }
    }

    fn check_shorthand_syntax(
        &self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext,
        offenses: &mut Vec<Offense>, paren_positions: Option<(usize, usize)>,
    ) {
        if !ctx.ruby_version_at_least(3, 1) { return; }
        match self.enforced_shorthand_syntax {
            EnforcedShorthandSyntax::Either => {}
            EnforcedShorthandSyntax::Always => {
                let assocs_to_omit: Vec<&&ruby_prism::AssocNode> = assocs.iter()
                    .filter(|a| self.can_omit_hash_value(a, ctx)).collect();
                if assocs_to_omit.is_empty() { return; }
                self.emit_omit_offenses("Omit the hash value.", &assocs_to_omit, ctx, offenses, paren_positions);
            }
            EnforcedShorthandSyntax::Never => {
                for assoc in assocs {
                    if self.is_shorthand_pair(assoc) {
                        let (start, end) = self.symbol_name_range(&assoc.key(), ctx);
                        let mut offense = ctx.offense_with_range("Style/HashSyntax", "Include the hash value.", Severity::Convention, start, end);
                        if let Some(c) = self.include_value_correction(assoc, ctx) { offense = offense.with_correction(c); }
                        offenses.push(offense);
                    }
                }
            }
            EnforcedShorthandSyntax::Consistent | EnforcedShorthandSyntax::EitherConsistent => {
                self.check_consistent_shorthand(assocs, ctx, offenses, paren_positions);
            }
        }
    }

    fn check_consistent_shorthand(
        &self, assocs: &[&ruby_prism::AssocNode], ctx: &CheckContext,
        offenses: &mut Vec<Offense>, paren_positions: Option<(usize, usize)>,
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
                has_explicit_that_cannot_omit = true;
            }
        }

        let has_shorthand = !shorthand_pairs.is_empty();
        let has_explicit = !explicit_pairs_that_could_omit.is_empty() || has_explicit_that_cannot_omit;

        if !has_shorthand || !has_explicit {
            if !has_shorthand && !explicit_pairs_that_could_omit.is_empty() && !has_explicit_that_cannot_omit {
                if self.enforced_shorthand_syntax == EnforcedShorthandSyntax::Consistent {
                    let refs: Vec<&&ruby_prism::AssocNode> = explicit_pairs_that_could_omit.iter().collect();
                    self.emit_omit_offenses("Omit the hash value.", &refs, ctx, offenses, paren_positions);
                }
            }
            return;
        }

        if !has_explicit_that_cannot_omit {
            let refs: Vec<&&ruby_prism::AssocNode> = explicit_pairs_that_could_omit.iter().collect();
            self.emit_omit_offenses("Do not mix explicit and implicit hash values. Omit the hash value.", &refs, ctx, offenses, paren_positions);
        } else {
            for assoc in &shorthand_pairs {
                let (start, end) = self.symbol_name_range(&assoc.key(), ctx);
                let mut offense = ctx.offense_with_range(
                    "Style/HashSyntax", "Do not mix explicit and implicit hash values. Include the hash value.",
                    Severity::Convention, start, end,
                );
                if let Some(c) = self.include_value_correction(assoc, ctx) { offense = offense.with_correction(c); }
                offenses.push(offense);
            }
        }
    }

    fn can_use_simple_ruby19(&self, key: &ruby_prism::Node, ctx: &CheckContext) -> bool {
        if let ruby_prism::Node::SymbolNode { .. } = key {
            let key_text = self.key_text(key, ctx);
            if key_text.starts_with(":\"") || key_text.starts_with(":'") { return false; }
            if key_text.starts_with(':') { return self.is_valid_ruby19_key(&key_text[1..]); }
        }
        false
    }

    fn is_valid_ruby19_key(&self, s: &str) -> bool {
        if s.is_empty() { return false; }
        let bytes = s.as_bytes();
        if !bytes[0].is_ascii_alphabetic() && bytes[0] != b'_' { return false; }
        let last = *bytes.last().unwrap();
        let check_end = if last == b'?' || last == b'!' {
            if self.prefer_hash_rockets_for_non_alnum_ending_symbols { return false; }
            bytes.len() - 1
        } else if last == b'=' {
            return false;
        } else {
            bytes.len()
        };
        bytes[1..check_end].iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    }

    fn key_text<'b>(&self, key: &ruby_prism::Node, ctx: &'b CheckContext) -> &'b str {
        let loc = key.location();
        ctx.source.get(loc.start_offset()..loc.end_offset()).unwrap_or("")
    }

    fn rocket_to_ruby19_correction(&self, assoc: &ruby_prism::AssocNode, ctx: &CheckContext) -> Option<Correction> {
        let key = assoc.key();
        let key_text = self.key_text(&key, ctx);
        let _op_loc = assoc.operator_loc()?;
        let value_loc = assoc.value().location();

        let new_key = if let Some(inner) = key_text.strip_prefix(":\"").or(key_text.strip_prefix(":'")) {
            format!("{}:", &key_text[1..])
        } else if let Some(ident) = key_text.strip_prefix(':') {
            format!("{}:", ident)
        } else {
            return None;
        };

        let needs_leading_space = key.location().start_offset() > 0 && {
            let prev = ctx.source.as_bytes()[key.location().start_offset() - 1];
            prev.is_ascii_alphanumeric() || prev == b'_'
        };
        let prefix = if needs_leading_space { " " } else { "" };

        Some(Correction::replace(key.location().start_offset(), value_loc.start_offset(), format!("{}{} ", prefix, new_key)))
    }

    fn ruby19_to_rocket_correction(&self, assoc: &ruby_prism::AssocNode, ctx: &CheckContext) -> Option<Correction> {
        let key = assoc.key();
        let key_text = self.key_text(&key, ctx);
        let value_loc = assoc.value().location();

        let inner = key_text.strip_suffix(':')?;
        let new_key = if inner.starts_with('"') || inner.starts_with('\'') {
            format!(":{}", inner)
        } else {
            format!(":{}", inner)
        };
        Some(Correction::replace(key.location().start_offset(), value_loc.start_offset(), format!("{} => ", new_key)))
    }

    fn omit_value_correction(&self, assoc: &ruby_prism::AssocNode, _ctx: &CheckContext) -> Option<Correction> {
        Some(Correction::delete(assoc.key().location().end_offset(), assoc.value().location().end_offset()))
    }

    fn include_value_correction(&self, assoc: &ruby_prism::AssocNode, ctx: &CheckContext) -> Option<Correction> {
        let key_text = self.key_text(&assoc.key(), ctx);
        let name = key_text.strip_suffix(':')?;
        Some(Correction::insert(assoc.key().location().end_offset(), format!(" {}", name)))
    }

    fn shorthand_to_rocket_correction(&self, assoc: &ruby_prism::AssocNode, ctx: &CheckContext) -> Option<Correction> {
        let key_text = self.key_text(&assoc.key(), ctx);
        let name = key_text.strip_suffix(':')?;
        Some(Correction::replace(
            assoc.key().location().start_offset(), assoc.location().end_offset(),
            format!(":{} => {}", name, name),
        ))
    }

    fn key_operator_range(&self, assoc: &ruby_prism::AssocNode) -> (usize, usize) {
        let start = assoc.key().location().start_offset();
        (start, assoc.operator_loc().map_or(assoc.key().location().end_offset(), |op| op.end_offset()))
    }

    fn symbol_name_range(&self, key: &ruby_prism::Node, ctx: &CheckContext) -> (usize, usize) {
        if let ruby_prism::Node::SymbolNode { .. } = key {
            if let Some(val_loc) = key.as_symbol_node().unwrap().value_loc() {
                return (val_loc.start_offset(), val_loc.end_offset());
            }
        }
        let loc = key.location();
        let key_text = self.key_text(key, ctx);
        if key_text.starts_with(':') { (loc.start_offset() + 1, loc.end_offset()) } else { (loc.start_offset(), loc.end_offset()) }
    }

    fn is_shorthand_pair(&self, assoc: &ruby_prism::AssocNode) -> bool {
        matches!(assoc.value(), ruby_prism::Node::ImplicitNode { .. })
    }

    fn can_omit_hash_value(&self, assoc: &ruby_prism::AssocNode, _ctx: &CheckContext) -> bool {
        if self.is_shorthand_pair(assoc) || assoc.operator_loc().is_some() { return false; }
        let key = assoc.key();
        if !matches!(key, ruby_prism::Node::SymbolNode { .. }) { return false; }
        let sym = key.as_symbol_node().unwrap();
        let key_name = String::from_utf8_lossy(sym.unescaped().as_ref()).to_string();
        if key_name.ends_with('?') || key_name.ends_with('!') { return false; }

        let value = assoc.value();
        match &value {
            ruby_prism::Node::LocalVariableReadNode { .. } => {
                key_name == node_name!(value.as_local_variable_read_node().unwrap()).as_ref()
            }
            ruby_prism::Node::CallNode { .. } => {
                let call = value.as_call_node().unwrap();
                if call.receiver().is_some() || call.arguments().is_some() || call.block().is_some() { return false; }
                key_name == node_name!(call).as_ref()
            }
            _ => false,
        }
    }

    fn is_modifier_call_context(&self, node: &ruby_prism::KeywordHashNode, ctx: &CheckContext) -> bool {
        let (node_start, node_end) = (node.location().start_offset(), node.location().end_offset());
        let before = ctx.source.get(..node_start).unwrap_or("");
        let before_on_line = before.rsplit('\n').next().unwrap_or(before);

        if before_on_line.contains('(') { return false; }

        let after = ctx.source.get(node_end..).unwrap_or("");
        let after_on_line = after.split('\n').next().unwrap_or("");
        if Self::starts_with_modifier_keyword(after_on_line.trim()) { return true; }

        let line_trimmed = before_on_line.trim();
        if !line_trimmed.is_empty() {
            for keyword in &["if ", "unless ", "while ", "until "] {
                if let Some(pos) = line_trimmed.find(keyword) {
                    if !line_trimmed[..pos].trim().is_empty() { return true; }
                }
            }
        }
        false
    }

    fn starts_with_modifier_keyword(text: &str) -> bool {
        ["if", "unless", "while", "until"].iter().any(|kw| {
            text.starts_with(kw) && (kw.len() >= text.len() || text.as_bytes()[kw.len()] == b' ')
        })
    }

    fn needs_braces_for_ruby19(&self, hash_node: &ruby_prism::KeywordHashNode, ctx: &CheckContext) -> bool {
        let hash_start = hash_node.location().start_offset();
        let line_start = ctx.source[..hash_start].rfind('\n').map_or(0, |p| p + 1);
        let before_hash = &ctx.source[line_start..hash_start];
        if before_hash.contains('(') || before_hash.contains('{') { return false; }
        let trimmed = before_hash.trim();
        trimmed == "return" || trimmed == "break" || trimmed == "next"
    }

    fn needs_parens_for_shorthand(&self, hash_node: &ruby_prism::KeywordHashNode, ctx: &CheckContext) -> Option<(usize, usize)> {
        let (hash_start, hash_end) = (hash_node.location().start_offset(), hash_node.location().end_offset());
        let line_start = ctx.source[..hash_start].rfind('\n').map_or(0, |p| p + 1);
        let line_end_offset = ctx.source[line_start..].find('\n').map_or(ctx.source.len(), |p| line_start + p);
        let line = &ctx.source[line_start..line_end_offset];
        let hash_pos_in_line = hash_start - line_start;
        let before_hash = &line[..hash_pos_in_line];

        if before_hash.contains('(') { return None; }
        let trimmed = before_hash.trim();
        if trimmed.is_empty() { return None; }

        let line_trimmed = line.trim();
        let in_conditional = ["if ", "unless ", "while ", "until ", "elsif "].iter().any(|kw| line_trimmed.starts_with(kw));
        let is_super_or_yield = trimmed.starts_with("super") || trimmed.starts_with("yield")
            || trimmed.contains(" super ") || trimmed.contains("= super ");
        let after_assignment_super_or_yield = {
            let after_eq = trimmed.rfind("= ").map_or(trimmed, |pos| trimmed[pos + 2..].trim_start());
            after_eq.starts_with("super") || after_eq.starts_with("yield")
        };

        let has_following_expression = {
            let after_line = &ctx.source[line_end_offset..];
            let next_line = after_line.trim_start_matches('\n').split('\n').next().unwrap_or("").trim();
            !next_line.is_empty() && !["end", "else", "elsif", "when", "rescue", "ensure", "def "].iter().any(|s| next_line.starts_with(s))
                && ![b'}', b']', b')'].contains(&next_line.as_bytes().first().copied().unwrap_or(0))
        };

        let has_positional_args = ctx.source[line_start..hash_start].trim_end().ends_with(',');

        if !in_conditional && !is_super_or_yield && !after_assignment_super_or_yield && !has_following_expression && !has_positional_args {
            return None;
        }

        let mut content = before_hash.trim_start();
        for keyword in &["if ", "unless ", "while ", "until ", "elsif ", "raise "] {
            if let Some(rest) = content.strip_prefix(keyword) {
                content = rest.trim_start();
                break;
            }
        }

        loop {
            if let Some(eq_pos) = content.find(" = ") {
                if content[..eq_pos].trim().chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '@') {
                    content = content[eq_pos + 3..].trim_start();
                    continue;
                }
            }
            break;
        }

        let has_commas = content.contains(',');
        let space_pos = if has_commas { content.find(' ') } else { content.rfind(' ') };

        space_pos.map(|sp| {
            let content_start_in_line = before_hash.len() - content.len();
            (line_start + content_start_in_line + sp, hash_end)
        })
    }

    fn emit_omit_offenses(
        &self, message: &str, assocs_to_omit: &[&&ruby_prism::AssocNode],
        ctx: &CheckContext, offenses: &mut Vec<Offense>, paren_positions: Option<(usize, usize)>,
    ) {
        if assocs_to_omit.is_empty() { return; }

        if let Some((open_off, close_off)) = paren_positions {
            let omit_refs: Vec<&ruby_prism::AssocNode> = assocs_to_omit.iter().map(|a| **a).collect();
            let combined = self.shorthand_with_parens_correction(&omit_refs, open_off, close_off);

            for (i, assoc) in assocs_to_omit.iter().enumerate() {
                let value = assoc.value();
                let mut offense = ctx.offense_with_range("Style/HashSyntax", message, Severity::Convention, value.location().start_offset(), value.location().end_offset());
                if i == 0 { if let Some(ref c) = combined { offense = offense.with_correction(c.clone()); } }
                offenses.push(offense);
            }
        } else {
            for assoc in assocs_to_omit {
                let value = assoc.value();
                let mut offense = ctx.offense_with_range("Style/HashSyntax", message, Severity::Convention, value.location().start_offset(), value.location().end_offset());
                if let Some(c) = self.omit_value_correction(assoc, ctx) { offense = offense.with_correction(c); }
                offenses.push(offense);
            }
        }
    }

    fn shorthand_with_parens_correction(&self, assocs_to_omit: &[&ruby_prism::AssocNode], open_paren_offset: usize, close_paren_offset: usize) -> Option<Correction> {
        let mut edits = vec![Edit {
            start_offset: open_paren_offset, end_offset: open_paren_offset + 1, replacement: "(".to_string(),
        }];
        for assoc in assocs_to_omit {
            edits.push(Edit {
                start_offset: assoc.key().location().end_offset(),
                end_offset: assoc.value().location().end_offset(),
                replacement: String::new(),
            });
        }
        edits.push(Edit {
            start_offset: close_paren_offset, end_offset: close_paren_offset, replacement: ")".to_string(),
        });
        Some(Correction { edits })
    }
}

impl Default for HashSyntax {
    fn default() -> Self { Self::new(EnforcedStyle::Ruby19) }
}

impl Cop for HashSyntax {
    fn name(&self) -> &'static str { "Style/HashSyntax" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_pairs(&node.elements().iter().collect::<Vec<_>>(), ctx, false, None)
    }

    fn check_keyword_hash(&self, node: &ruby_prism::KeywordHashNode, ctx: &CheckContext) -> Vec<Offense> {
        let elements: Vec<_> = node.elements().iter().collect();
        let modifier_context = self.is_modifier_call_context(node, ctx);
        let paren_positions = self.needs_parens_for_shorthand(node, ctx);
        let mut offenses = self.check_pairs(&elements, ctx, modifier_context, paren_positions);

        if self.needs_braces_for_ruby19(node, ctx) {
            let (hash_start, hash_end) = (node.location().start_offset(), node.location().end_offset());
            for offense in &mut offenses {
                if let Some(ref correction) = offense.correction {
                    let mut edits = correction.edits.clone();
                    edits.push(Edit { start_offset: hash_start, end_offset: hash_start, replacement: "{".to_string() });
                    edits.push(Edit { start_offset: hash_end, end_offset: hash_end, replacement: "}".to_string() });
                    offense.correction = Some(Correction { edits });
                }
            }
        }
        offenses
    }
}
