//! Style/RescueStandardError - Checks for rescues with or without StandardError.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/rescue_standard_error.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

/// Enforced style for StandardError rescuing
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    /// Require `rescue StandardError` explicitly
    Explicit,
    /// Prefer bare `rescue` instead of `rescue StandardError`
    Implicit,
}

/// Checks for rescuing StandardError.
///
/// # Examples
///
/// ## EnforcedStyle: explicit (default)
/// ```ruby
/// # bad
/// begin
///   foo
/// rescue
///   bar
/// end
///
/// # good
/// begin
///   foo
/// rescue StandardError
///   bar
/// end
/// ```
///
/// ## EnforcedStyle: implicit
/// ```ruby
/// # bad
/// begin
///   foo
/// rescue StandardError
///   bar
/// end
///
/// # good
/// begin
///   foo
/// rescue
///   bar
/// end
/// ```
pub struct RescueStandardError {
    enforced_style: EnforcedStyle,
}

impl RescueStandardError {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self { enforced_style }
    }
}

impl Default for RescueStandardError {
    fn default() -> Self {
        Self::new(EnforcedStyle::Explicit)
    }
}

struct RescueVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    enforced_style: EnforcedStyle,
    cop_name: &'static str,
    offenses: Vec<Offense>,
}

impl<'a> RescueVisitor<'a> {
    fn new(
        ctx: &'a CheckContext<'a>,
        enforced_style: EnforcedStyle,
        cop_name: &'static str,
    ) -> Self {
        Self {
            ctx,
            enforced_style,
            cop_name,
            offenses: Vec::new(),
        }
    }

    fn is_standard_error(&self, node: &ruby_prism::Node) -> bool {
        match node {
            // Direct reference: StandardError
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = node.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(const_node.name().as_slice());
                name == "StandardError"
            }
            // Top-level reference: ::StandardError
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = node.as_constant_path_node().unwrap();
                // Check if parent is nil (top-level) and name is StandardError
                if path_node.parent().is_none() {
                    if let Some(name) = path_node.name() {
                        let const_name = String::from_utf8_lossy(name.as_slice());
                        return const_name == "StandardError";
                    }
                }
                false
            }
            _ => false,
        }
    }
}

impl Visit<'_> for RescueVisitor<'_> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        // Get the exceptions being rescued
        let exceptions = node.exceptions();
        let keyword_loc = node.keyword_loc();

        match self.enforced_style {
            EnforcedStyle::Implicit => {
                // Flag if only StandardError is explicitly specified
                if exceptions.len() == 1 {
                    let first = exceptions.iter().next();
                    if let Some(exc) = first {
                        if self.is_standard_error(&exc) {
                            // Highlight from 'rescue' to end of 'StandardError'
                            let start = keyword_loc.start_offset();
                            let end = exc.location().end_offset();
                            // Correction: delete from after 'rescue' to end of exception class
                            let correction = Correction::delete(
                                keyword_loc.end_offset(),
                                exc.location().end_offset(),
                            );
                            self.offenses.push(self.ctx.offense_with_range(
                                self.cop_name,
                                "Omit the error class when rescuing `StandardError` by itself.",
                                Severity::Convention,
                                start,
                                end,
                            ).with_correction(correction));
                        }
                    }
                }
            }
            EnforcedStyle::Explicit => {
                // Flag if no exception class is specified (bare rescue)
                if exceptions.is_empty() {
                    // Correction: insert " StandardError" after 'rescue'
                    let correction = Correction::insert(
                        keyword_loc.end_offset(),
                        " StandardError",
                    );
                    // Highlight just the 'rescue' keyword
                    self.offenses.push(self.ctx.offense(
                        self.cop_name,
                        "Avoid rescuing without specifying an error class.",
                        Severity::Convention,
                        &keyword_loc,
                    ).with_correction(correction));
                }
            }
        }

        // Continue visiting nested nodes
        ruby_prism::visit_rescue_node(self, node);
    }
}

impl Cop for RescueStandardError {
    fn name(&self) -> &'static str {
        "Style/RescueStandardError"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = RescueVisitor::new(ctx, self.enforced_style, self.name());
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_style(source: &str, style: EnforcedStyle) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(RescueStandardError::new(style));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_style(source, EnforcedStyle::Implicit)
    }

    #[test]
    fn implicit_flags_standard_error() {
        let source = r#"
begin
  foo
rescue StandardError
  bar
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("Omit"));
    }

    #[test]
    fn implicit_allows_bare_rescue() {
        let source = r#"
begin
  foo
rescue
  bar
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn implicit_allows_other_exceptions() {
        let source = r#"
begin
  foo
rescue RuntimeError
  bar
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn implicit_allows_multiple_exceptions() {
        let source = r#"
begin
  foo
rescue StandardError, RuntimeError
  bar
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn explicit_flags_bare_rescue() {
        let source = r#"
begin
  foo
rescue
  bar
end
"#;
        let offenses = check_with_style(source, EnforcedStyle::Explicit);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("without specifying"));
    }

    #[test]
    fn explicit_allows_standard_error() {
        let source = r#"
begin
  foo
rescue StandardError
  bar
end
"#;
        let offenses = check_with_style(source, EnforcedStyle::Explicit);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_rescue_with_variable() {
        let source = r#"
begin
  foo
rescue => e
  bar
end
"#;
        let offenses = check(source);
        assert_eq!(offenses.len(), 0);
    }
}
