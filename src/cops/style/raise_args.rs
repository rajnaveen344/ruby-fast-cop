//! Style/RaiseArgs cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Explode,
    Compact,
}

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

    fn is_complex_arg(arg: &ruby_prism::Node) -> bool {
        matches!(
            arg,
            ruby_prism::Node::KeywordHashNode { .. }
                | ruby_prism::Node::HashNode { .. }
                | ruby_prism::Node::SplatNode { .. }
                | ruby_prism::Node::ForwardingArgumentsNode { .. }
        )
    }

    fn get_constant_name(node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = node.as_constant_read_node().unwrap();
                Some(node_name!(const_node).to_string())
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
        let method_name = node_name!(node);
        if method_name != "raise" && method_name != "fail" { return vec![]; }
        if node.receiver().is_some() { return vec![]; }
        let arguments = match node.arguments() { Some(args) => args, None => return vec![] };

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
                            let name = node_name!(call);
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
                                let name = node_name!(call);
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
                        let called_method = node_name!(call_arg);

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

crate::register_cop!("Style/RaiseArgs", |cfg| {
    let cop_config = cfg.get_cop_config("Style/RaiseArgs");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "compact" => EnforcedStyle::Compact,
            _ => EnforcedStyle::Explode,
        })
        .unwrap_or(EnforcedStyle::Explode);
    let allowed_compact_types = cop_config
        .and_then(|c| c.raw.get("AllowedCompactTypes"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(RaiseArgs::with_allowed_compact_types(style, allowed_compact_types)))
});
