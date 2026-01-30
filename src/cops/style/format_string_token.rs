//! Style/FormatStringToken - Checks format string tokens.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/format_string_token.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;

/// Enforced style for format string tokens
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    /// Requires `%<name>s` style tokens
    Annotated,
    /// Requires `%{name}` style tokens
    Template,
    /// Requires `%s` style tokens (positional)
    Unannotated,
}

/// Checks format string tokens style.
///
/// # Examples
///
/// ## EnforcedStyle: annotated (default)
/// ```ruby
/// # bad
/// format('%{greeting}', greeting: 'Hello')
/// format('%s', 'Hello')
///
/// # good
/// format('%<greeting>s', greeting: 'Hello')
/// ```
///
/// ## EnforcedStyle: template
/// ```ruby
/// # bad
/// format('%<greeting>s', greeting: 'Hello')
/// format('%s', 'Hello')
///
/// # good
/// format('%{greeting}', greeting: 'Hello')
/// ```
///
/// ## EnforcedStyle: unannotated
/// ```ruby
/// # bad
/// format('%<greeting>s', greeting: 'Hello')
/// format('%{greeting}', greeting: 'Hello')
///
/// # good
/// format('%s', 'Hello')
/// ```
pub struct FormatStringToken {
    enforced_style: EnforcedStyle,
    max_unannotated_placeholders: usize,
    annotated_regex: Regex,
    template_regex: Regex,
    unannotated_regex: Regex,
}

impl FormatStringToken {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self {
            enforced_style,
            max_unannotated_placeholders: 1,
            // Matches %<name>s style (annotated)
            annotated_regex: Regex::new(r"%<\w+>[a-zA-Z]").unwrap(),
            // Matches %{name} style (template)
            template_regex: Regex::new(r"%\{\w+\}").unwrap(),
            // Matches %s, %d, %f, etc. (unannotated positional)
            unannotated_regex: Regex::new(r"%[^<{%\s][a-zA-Z]?|%[a-zA-Z]").unwrap(),
        }
    }

    fn contains_format_tokens(&self, s: &str) -> bool {
        s.contains('%')
    }

    fn detect_token_styles(&self, s: &str) -> (bool, bool, bool) {
        let has_annotated = self.annotated_regex.is_match(s);
        let has_template = self.template_regex.is_match(s);
        let has_unannotated = self.unannotated_regex.is_match(s);
        (has_annotated, has_template, has_unannotated)
    }

    fn count_unannotated(&self, s: &str) -> usize {
        self.unannotated_regex.find_iter(s).count()
    }
}

impl Default for FormatStringToken {
    fn default() -> Self {
        Self::new(EnforcedStyle::Annotated)
    }
}

impl Cop for FormatStringToken {
    fn name(&self) -> &'static str {
        "Style/FormatStringToken"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_string(&self, node: &ruby_prism::StringNode, ctx: &CheckContext) -> Vec<Offense> {
        let content = String::from_utf8_lossy(node.unescaped());

        if !self.contains_format_tokens(&content) {
            return vec![];
        }

        let (has_annotated, has_template, has_unannotated) = self.detect_token_styles(&content);

        // If no format tokens detected, skip
        if !has_annotated && !has_template && !has_unannotated {
            return vec![];
        }

        match self.enforced_style {
            EnforcedStyle::Template => {
                // Flag annotated or unannotated styles
                if has_annotated {
                    return vec![ctx.offense(
                        self.name(),
                        "Prefer template tokens (like `%{name}`) over annotated tokens (like `%<name>s`).",
                        self.severity(),
                        &node.location(),
                    )];
                }
                if has_unannotated && self.count_unannotated(&content) > self.max_unannotated_placeholders {
                    return vec![ctx.offense(
                        self.name(),
                        "Prefer template tokens (like `%{name}`) over unannotated tokens (like `%s`).",
                        self.severity(),
                        &node.location(),
                    )];
                }
            }
            EnforcedStyle::Annotated => {
                // Flag template or unannotated styles
                if has_template {
                    return vec![ctx.offense(
                        self.name(),
                        "Prefer annotated tokens (like `%<name>s`) over template tokens (like `%{name}`).",
                        self.severity(),
                        &node.location(),
                    )];
                }
                if has_unannotated && self.count_unannotated(&content) > self.max_unannotated_placeholders {
                    return vec![ctx.offense(
                        self.name(),
                        "Prefer annotated tokens (like `%<name>s`) over unannotated tokens (like `%s`).",
                        self.severity(),
                        &node.location(),
                    )];
                }
            }
            EnforcedStyle::Unannotated => {
                // Flag annotated or template styles
                if has_annotated {
                    return vec![ctx.offense(
                        self.name(),
                        "Prefer unannotated tokens (like `%s`) over annotated tokens (like `%<name>s`).",
                        self.severity(),
                        &node.location(),
                    )];
                }
                if has_template {
                    return vec![ctx.offense(
                        self.name(),
                        "Prefer unannotated tokens (like `%s`) over template tokens (like `%{name}`).",
                        self.severity(),
                        &node.location(),
                    )];
                }
            }
        }

        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_style(source: &str, style: EnforcedStyle) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(FormatStringToken::new(style));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_style(source, EnforcedStyle::Template)
    }

    #[test]
    fn template_allows_template_tokens() {
        let offenses = check("format('%{greeting}', greeting: 'Hello')");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn template_flags_annotated_tokens() {
        let offenses = check("format('%<greeting>s', greeting: 'Hello')");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("template"));
    }

    #[test]
    fn template_allows_single_unannotated() {
        // Single unannotated is allowed by default
        let offenses = check("format('%s', 'Hello')");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn template_flags_multiple_unannotated() {
        let offenses = check("format('%s %s', 'Hello', 'World')");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("template"));
    }

    #[test]
    fn annotated_allows_annotated_tokens() {
        let offenses = check_with_style(
            "format('%<greeting>s', greeting: 'Hello')",
            EnforcedStyle::Annotated,
        );
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn annotated_flags_template_tokens() {
        let offenses = check_with_style(
            "format('%{greeting}', greeting: 'Hello')",
            EnforcedStyle::Annotated,
        );
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("annotated"));
    }

    #[test]
    fn unannotated_allows_unannotated_tokens() {
        let offenses = check_with_style("format('%s', 'Hello')", EnforcedStyle::Unannotated);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn unannotated_flags_template_tokens() {
        let offenses = check_with_style(
            "format('%{greeting}', greeting: 'Hello')",
            EnforcedStyle::Unannotated,
        );
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("unannotated"));
    }

    #[test]
    fn allows_strings_without_format_tokens() {
        let offenses = check("'hello world'");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_percent_in_regular_string() {
        let offenses = check("'50% off'");
        assert_eq!(offenses.len(), 0);
    }
}
