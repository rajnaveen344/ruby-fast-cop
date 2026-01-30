//! Style/RaiseArgs - Checks the args passed to raise/fail.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/raise_args.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

/// Enforced style for raise arguments
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    /// Require `raise ErrorClass, message` - separate args
    Explode,
    /// Require `raise ErrorClass.new(message)` - explicit construction
    Compact,
}

/// Checks the args passed to `raise` and `fail`.
///
/// # Examples
///
/// ## EnforcedStyle: explode (default)
/// ```ruby
/// # bad
/// raise StandardError.new("message")
///
/// # good
/// raise StandardError, "message"
/// fail "message"
/// raise MyError.new("message", params)
/// ```
///
/// ## EnforcedStyle: compact
/// ```ruby
/// # bad
/// raise StandardError, "message"
///
/// # good
/// raise StandardError.new("message")
/// fail "message"
/// ```
pub struct RaiseArgs {
    enforced_style: EnforcedStyle,
}

impl RaiseArgs {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self { enforced_style }
    }

    fn is_raise_or_fail(&self, name: &str) -> bool {
        name == "raise" || name == "fail"
    }
}

impl Default for RaiseArgs {
    fn default() -> Self {
        Self::new(EnforcedStyle::Explode)
    }
}

impl Cop for RaiseArgs {
    fn name(&self) -> &'static str {
        "Style/RaiseArgs"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = String::from_utf8_lossy(node.name().as_slice());

        // Only check raise and fail
        if !self.is_raise_or_fail(&method_name) {
            return vec![];
        }

        // Check if there's a receiver (we only want bare raise/fail)
        if node.receiver().is_some() {
            return vec![];
        }

        let arguments = match node.arguments() {
            Some(args) => args,
            None => return vec![], // No arguments, nothing to check
        };

        let args: Vec<_> = arguments.arguments().iter().collect();

        match self.enforced_style {
            EnforcedStyle::Compact => {
                // Flag: raise ErrorClass, "message" (two separate args where first is a constant)
                if args.len() >= 2 {
                    // Check if first arg is a constant (exception class)
                    if let Some(ruby_prism::Node::ConstantReadNode { .. }) = args.first() {
                        return vec![ctx.offense(
                            self.name(),
                            &format!(
                                "Provide an exception object as an argument to `{}`.",
                                method_name
                            ),
                            self.severity(),
                            &node.location(),
                        )];
                    }
                }
            }
            EnforcedStyle::Explode => {
                // Flag: raise ErrorClass.new("message") (single arg that's a .new call)
                if args.len() == 1 {
                    if let Some(ruby_prism::Node::CallNode { .. }) = args.first() {
                        let call_arg = args.first().unwrap().as_call_node().unwrap();
                        let called_method =
                            String::from_utf8_lossy(call_arg.name().as_slice());

                        if called_method == "new" {
                            // Check if receiver is a constant (exception class)
                            if let Some(ruby_prism::Node::ConstantReadNode { .. }) =
                                call_arg.receiver()
                            {
                                // Only flag if there's exactly one string argument to .new
                                if let Some(new_args) = call_arg.arguments() {
                                    let new_args_list: Vec<_> =
                                        new_args.arguments().iter().collect();
                                    if new_args_list.len() == 1 {
                                        if let Some(ruby_prism::Node::StringNode { .. }) =
                                            new_args_list.first()
                                        {
                                            return vec![ctx.offense(
                                                self.name(),
                                                &format!(
                                                    "Provide an exception class and message as arguments to `{}`.",
                                                    method_name
                                                ),
                                                self.severity(),
                                                &node.location(),
                                            )];
                                        }
                                    }
                                }
                            }
                        }
                    }
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
        let cop: Box<dyn Cop> = Box::new(RaiseArgs::new(style));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_style(source, EnforcedStyle::Compact)
    }

    #[test]
    fn compact_flags_separate_args() {
        let offenses = check("raise StandardError, 'message'");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("exception object"));
    }

    #[test]
    fn compact_allows_new_call() {
        let offenses = check("raise StandardError.new('message')");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn compact_allows_just_message() {
        let offenses = check("raise 'message'");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn compact_allows_just_error_class() {
        let offenses = check("raise StandardError");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn explode_flags_new_call() {
        let offenses = check_with_style("raise StandardError.new('message')", EnforcedStyle::Explode);
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("class and message"));
    }

    #[test]
    fn explode_allows_separate_args() {
        let offenses = check_with_style("raise StandardError, 'message'", EnforcedStyle::Explode);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn explode_allows_multiple_args_to_new() {
        // When .new has multiple args, it's allowed
        let offenses =
            check_with_style("raise MyError.new('msg', params)", EnforcedStyle::Explode);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn fail_is_also_checked() {
        let offenses = check("fail StandardError, 'message'");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn allows_raise_with_variable() {
        let offenses = check("raise error");
        assert_eq!(offenses.len(), 0);
    }
}
