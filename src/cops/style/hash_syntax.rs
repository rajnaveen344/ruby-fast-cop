//! Style/HashSyntax - Checks hash literal syntax.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/hash_syntax.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

/// Enforced style for hash syntax
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    /// Use `{a: 1}` syntax for symbol keys
    Ruby19,
    /// Use `{:a => 1}` syntax for all keys
    HashRockets,
    /// Don't mix styles in the same hash
    NoMixedKeys,
    /// Use ruby19 style but also don't mix styles
    Ruby19NoMixedKeys,
}

/// Checks hash literal syntax.
///
/// # Examples
///
/// ## EnforcedStyle: ruby19 (default)
/// ```ruby
/// # bad
/// {:a => 2}
/// {b: 1, :c => 2}
///
/// # good
/// {a: 2, b: 1}
/// {:c => 2, 'd' => 2} # acceptable since 'd' isn't a symbol
/// {d: 1, 'e' => 2} # technically ok but triggers if UseHashRocketsWithSymbolValues
/// ```
///
/// ## EnforcedStyle: ruby19_no_mixed_keys
/// ```ruby
/// # bad
/// {:a => 1, b: 2}
/// {c: 1, 'd' => 2}
///
/// # good
/// {a: 1, b: 2}
/// {:c => 1, 'd' => 2}
/// ```
pub struct HashSyntax {
    enforced_style: EnforcedStyle,
}

impl HashSyntax {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self { enforced_style }
    }

    /// Check if a symbol can use ruby19 syntax (key: value)
    /// Symbols like `:"foo-bar"` or `:"foo bar"` cannot use ruby19 syntax
    fn can_use_ruby19_syntax(&self, key_text: &str) -> bool {
        // If the symbol is quoted (:"..."), check if the inner content is a valid identifier
        if key_text.starts_with(":\"") || key_text.starts_with(":'") {
            return false;
        }
        // Simple symbol like :foo or :foo_bar can use ruby19 syntax
        if key_text.starts_with(':') {
            let identifier = &key_text[1..];
            return self.is_valid_ruby19_key(identifier);
        }
        false
    }

    /// Check if a string is a valid identifier for ruby19 syntax
    fn is_valid_ruby19_key(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let mut chars = s.chars();
        // First character must be letter or underscore
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
            _ => return false,
        }
        // Rest must be alphanumeric or underscore
        for c in chars {
            if !c.is_ascii_alphanumeric() && c != '_' {
                return false;
            }
        }
        true
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
        let mut offenses = Vec::new();
        let elements = node.elements();

        // Collect styles used in this hash
        let mut has_ruby19 = false;
        let mut has_hash_rocket = false;
        let mut ruby19_symbol_with_hash_rocket: Vec<ruby_prism::Location> = Vec::new();

        for element in elements.iter() {
            if let ruby_prism::Node::AssocNode { .. } = element {
                let assoc = element.as_assoc_node().unwrap();
                let key = assoc.key();
                let operator = assoc.operator_loc();

                // Check if this pair uses hash rocket (has => operator)
                let uses_hash_rocket = operator.is_some();

                if uses_hash_rocket {
                    has_hash_rocket = true;

                    // Check if the key is a symbol that could use ruby19 syntax
                    if let ruby_prism::Node::SymbolNode { .. } = &key {
                        let key_loc = key.location();
                        let key_start = key_loc.start_offset();
                        let key_end = key_loc.end_offset();
                        if let Some(key_text) = ctx.source.get(key_start..key_end) {
                            if self.can_use_ruby19_syntax(key_text) {
                                ruby19_symbol_with_hash_rocket.push(assoc.location());
                            }
                        }
                    }
                } else {
                    has_ruby19 = true;
                }
            }
        }

        match self.enforced_style {
            EnforcedStyle::Ruby19 | EnforcedStyle::Ruby19NoMixedKeys => {
                // Flag symbol keys using hash rockets that could use ruby19 syntax
                for loc in &ruby19_symbol_with_hash_rocket {
                    offenses.push(ctx.offense(
                        self.name(),
                        "Use the new Ruby 1.9 hash syntax.",
                        self.severity(),
                        loc,
                    ));
                }
            }
            EnforcedStyle::HashRockets => {
                // Flag any pairs using ruby19 syntax
                for element in elements.iter() {
                    if let ruby_prism::Node::AssocNode { .. } = element {
                        let assoc = element.as_assoc_node().unwrap();
                        if assoc.operator_loc().is_none() {
                            offenses.push(ctx.offense(
                                self.name(),
                                "Use hash rockets syntax.",
                                self.severity(),
                                &assoc.location(),
                            ));
                        }
                    }
                }
            }
            EnforcedStyle::NoMixedKeys => {
                // Only flag if we have mixed styles
                if has_ruby19 && has_hash_rocket {
                    offenses.push(ctx.offense(
                        self.name(),
                        "Don't mix styles in the same hash.",
                        self.severity(),
                        &node.location(),
                    ));
                }
            }
        }

        // For Ruby19NoMixedKeys, also check for mixed styles
        if self.enforced_style == EnforcedStyle::Ruby19NoMixedKeys && has_ruby19 && has_hash_rocket
        {
            // Only report mixed styles if we haven't already flagged the hash rockets
            // that could be converted to ruby19
            let remaining_hash_rockets = has_hash_rocket
                && elements.iter().any(|e| {
                    if let ruby_prism::Node::AssocNode { .. } = e {
                        let assoc = e.as_assoc_node().unwrap();
                        if assoc.operator_loc().is_some() {
                            // Check if this is a non-symbol key
                            !matches!(assoc.key(), ruby_prism::Node::SymbolNode { .. })
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                });

            if remaining_hash_rockets {
                offenses.push(ctx.offense(
                    self.name(),
                    "Don't mix styles in the same hash.",
                    self.severity(),
                    &node.location(),
                ));
            }
        }

        offenses
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
        // Symbol key with hash rocket that could be ruby19, and string key with hash rocket
        let offenses = check("{a: 1, 'b' => 2}");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("mix"));
    }

    #[test]
    fn allows_consistent_hash_rockets_with_string_keys() {
        let offenses = check("{:a => 1, 'b' => 2}");
        // This should flag :a => 1 as it could use ruby19 syntax
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("Ruby 1.9"));
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
        // :"foo-bar" cannot use ruby19 syntax
        let offenses = check("{:\"foo-bar\" => 1}");
        assert_eq!(offenses.len(), 0);
    }
}
