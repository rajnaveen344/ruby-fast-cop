//! Style/RaiseArgs - Checks the args passed to raise/fail.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/raise_args.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

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
    /// Exception types allowed to use compact form even in exploded style
    allowed_compact_types: Vec<String>,
}

impl RaiseArgs {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self {
            enforced_style,
            allowed_compact_types: Vec::new(),
        }
    }

    pub fn with_allowed_compact_types(
        enforced_style: EnforcedStyle,
        allowed_compact_types: Vec<String>,
    ) -> Self {
        Self {
            enforced_style,
            allowed_compact_types,
        }
    }

    fn is_raise_or_fail(&self, name: &str) -> bool {
        name == "raise" || name == "fail"
    }

    /// Check if an argument is "complex" (keyword hash, splat, forwarding, etc.)
    /// These should not be flagged in exploded style
    fn is_complex_arg(arg: &ruby_prism::Node) -> bool {
        matches!(
            arg,
            ruby_prism::Node::KeywordHashNode { .. }
                | ruby_prism::Node::HashNode { .. }
                | ruby_prism::Node::SplatNode { .. }
                | ruby_prism::Node::ForwardingArgumentsNode { .. }
        )
    }

    /// Get the name of a constant from a node
    fn get_constant_name(node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = node.as_constant_read_node().unwrap();
                Some(String::from_utf8_lossy(const_node.name().as_slice()).to_string())
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = node.as_constant_path_node().unwrap();
                path_node
                    .name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()).to_string())
            }
            _ => None,
        }
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
                // Flag: raise X, msg or raise X, msg, backtrace (2+ args)
                // This should be raise X.new(msg) instead
                // BUT don't flag if first arg is already an object (a .new call with keyword args)
                if args.len() >= 2 {
                    // Check if first arg is already an exception object (a .new call)
                    let first_is_new_call = if let Some(arg) = args.first() {
                        if let ruby_prism::Node::CallNode { .. } = arg {
                            let call = arg.as_call_node().unwrap();
                            let name = String::from_utf8_lossy(call.name().as_slice());
                            // If it's a .new call with keyword args, it's already an object
                            if name == "new" {
                                if let Some(new_args) = call.arguments() {
                                    new_args.arguments().iter().any(|a| {
                                        matches!(a, ruby_prism::Node::KeywordHashNode { .. })
                                    })
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if !first_is_new_call {
                        // Correction: raise Ex, msg → raise Ex.new(msg)
                        // Special case: raise Ex.new, msg → raise Ex.new(msg)
                        let correction = if args.len() == 2 {
                            let first_arg = &args[0];
                            let second_arg = &args[1];
                            let exception_src = &ctx.source[first_arg.location().start_offset()..first_arg.location().end_offset()];
                            let msg_src = &ctx.source[second_arg.location().start_offset()..second_arg.location().end_offset()];

                            // Check if the first arg is already a .new call (without args)
                            let first_is_bare_new = if let ruby_prism::Node::CallNode { .. } = first_arg {
                                let call = first_arg.as_call_node().unwrap();
                                let name = String::from_utf8_lossy(call.name().as_slice());
                                name == "new" && call.arguments().is_none()
                            } else {
                                false
                            };

                            let new_src = if first_is_bare_new {
                                // raise Ex.new, msg → raise Ex.new(msg)
                                format!("{}({})", exception_src, msg_src)
                            } else {
                                // raise Ex, msg → raise Ex.new(msg)
                                format!("{}.new({})", exception_src, msg_src)
                            };

                            Some(Correction::replace(
                                first_arg.location().start_offset(),
                                second_arg.location().end_offset(),
                                new_src,
                            ))
                        } else {
                            None
                        };

                        let mut offense = ctx.offense(
                            self.name(),
                            &format!(
                                "Provide an exception object as an argument to `{}`.",
                                method_name
                            ),
                            self.severity(),
                            &node.location(),
                        );
                        if let Some(c) = correction {
                            offense = offense.with_correction(c);
                        }
                        return vec![offense];
                    }
                }
            }
            EnforcedStyle::Explode => {
                // Flag: raise ErrorClass.new or raise ErrorClass.new(single_arg)
                // But NOT raise ErrorClass.new(arg1, arg2) or raise ErrorClass.new(kwarg: val)
                if args.len() == 1 {
                    if let Some(ruby_prism::Node::CallNode { .. }) = args.first() {
                        let call_arg = args.first().unwrap().as_call_node().unwrap();
                        let called_method = String::from_utf8_lossy(call_arg.name().as_slice());

                        if called_method == "new" {
                            // Check if receiver exists (could be constant or variable)
                            if let Some(receiver) = call_arg.receiver() {
                                // Check if this exception type is in allowed_compact_types
                                let exception_name = Self::get_constant_name(&receiver);
                                if let Some(ref name) = exception_name {
                                    if self.allowed_compact_types.contains(name) {
                                        return vec![];
                                    }
                                }

                                // Check arguments to .new
                                let new_args = call_arg.arguments();
                                let should_flag = match &new_args {
                                    None => true, // No args: raise Ex.new
                                    Some(args) => {
                                        let arg_list: Vec<_> = args.arguments().iter().collect();
                                        // Flag only if 0 or 1 simple arguments
                                        // Don't flag if multiple args, keyword args, splats, etc.
                                        if arg_list.is_empty() {
                                            true
                                        } else if arg_list.len() == 1 {
                                            // Check if it's a keyword hash, splat, etc.
                                            let arg = arg_list.first().unwrap();
                                            !Self::is_complex_arg(arg)
                                        } else {
                                            false // Multiple args, don't flag
                                        }
                                    }
                                };

                                if should_flag {
                                    // Correction: raise Ex.new(msg) → raise Ex, msg
                                    //             raise Ex.new → raise Ex
                                    let receiver_src = &ctx.source[receiver.location().start_offset()..receiver.location().end_offset()];
                                    let correction = match &new_args {
                                        None => {
                                            // raise Ex.new → raise Ex
                                            // Replace from receiver start to call_arg end
                                            Some(Correction::replace(
                                                receiver.location().start_offset(),
                                                call_arg.location().end_offset(),
                                                receiver_src.to_string(),
                                            ))
                                        }
                                        Some(new_args_node) => {
                                            let arg_list: Vec<_> = new_args_node.arguments().iter().collect();
                                            if arg_list.len() == 1 {
                                                let msg_arg = &arg_list[0];
                                                let msg_src = &ctx.source[msg_arg.location().start_offset()..msg_arg.location().end_offset()];
                                                // raise Ex.new(msg) → raise Ex, msg
                                                let has_parens = node.opening_loc().is_some();
                                                let new_src = if has_parens {
                                                    format!("{}, {}", receiver_src, msg_src)
                                                } else {
                                                    format!("{}, {}", receiver_src, msg_src)
                                                };
                                                Some(Correction::replace(
                                                    receiver.location().start_offset(),
                                                    call_arg.location().end_offset(),
                                                    new_src,
                                                ))
                                            } else if arg_list.is_empty() {
                                                // raise Ex.new() → raise Ex
                                                Some(Correction::replace(
                                                    receiver.location().start_offset(),
                                                    call_arg.location().end_offset(),
                                                    receiver_src.to_string(),
                                                ))
                                            } else {
                                                None
                                            }
                                        }
                                    };
                                    let mut offense = ctx.offense(
                                        self.name(),
                                        &format!(
                                            "Provide an exception class and message as arguments to `{}`.",
                                            method_name
                                        ),
                                        self.severity(),
                                        &node.location(),
                                    );
                                    if let Some(c) = correction {
                                        offense = offense.with_correction(c);
                                    }
                                    return vec![offense];
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
        let offenses =
            check_with_style("raise StandardError.new('message')", EnforcedStyle::Explode);
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
        let offenses = check_with_style("raise MyError.new('msg', params)", EnforcedStyle::Explode);
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
