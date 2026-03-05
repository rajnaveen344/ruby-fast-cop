//! Style/RescueStandardError cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Explicit,
    Implicit,
}

pub struct RescueStandardError {
    enforced_style: EnforcedStyle,
}

impl RescueStandardError {
    pub fn new(enforced_style: EnforcedStyle) -> Self { Self { enforced_style } }
}

impl Default for RescueStandardError {
    fn default() -> Self { Self::new(EnforcedStyle::Explicit) }
}

struct RescueVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    enforced_style: EnforcedStyle,
    cop_name: &'static str,
    offenses: Vec<Offense>,
}

impl RescueVisitor<'_> {
    fn is_standard_error(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } =>
                String::from_utf8_lossy(node.as_constant_read_node().unwrap().name().as_slice()) == "StandardError",
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path = node.as_constant_path_node().unwrap();
                path.parent().is_none() && path.name()
                    .map_or(false, |n| String::from_utf8_lossy(n.as_slice()) == "StandardError")
            }
            _ => false,
        }
    }
}

impl Visit<'_> for RescueVisitor<'_> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        let exceptions = node.exceptions();
        let keyword_loc = node.keyword_loc();

        match self.enforced_style {
            EnforcedStyle::Implicit => {
                if exceptions.len() == 1 {
                    if let Some(exc) = exceptions.iter().next() {
                        if self.is_standard_error(&exc) {
                            self.offenses.push(self.ctx.offense_with_range(
                                self.cop_name,
                                "Omit the error class when rescuing `StandardError` by itself.",
                                Severity::Convention, keyword_loc.start_offset(), exc.location().end_offset(),
                            ).with_correction(Correction::delete(keyword_loc.end_offset(), exc.location().end_offset())));
                        }
                    }
                }
            }
            EnforcedStyle::Explicit => {
                if exceptions.is_empty() {
                    self.offenses.push(self.ctx.offense(
                        self.cop_name, "Avoid rescuing without specifying an error class.",
                        Severity::Convention, &keyword_loc,
                    ).with_correction(Correction::insert(keyword_loc.end_offset(), " StandardError")));
                }
            }
        }
        ruby_prism::visit_rescue_node(self, node);
    }
}

impl Cop for RescueStandardError {
    fn name(&self) -> &'static str { "Style/RescueStandardError" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RescueVisitor { ctx, enforced_style: self.enforced_style, cop_name: self.name(), offenses: Vec::new() };
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
