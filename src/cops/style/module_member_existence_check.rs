//! Style/ModuleMemberExistenceCheck - flag `x.constants.include?(:y)` →
//! `x.const_defined?(:y)` and friends.
//!
//! Ported from `lib/rubocop/cop/style/module_member_existence_check.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;
use std::collections::HashSet;

#[derive(Default)]
pub struct ModuleMemberExistenceCheck {
    allowed_methods: HashSet<String>,
}

impl ModuleMemberExistenceCheck {
    pub fn with_allowed(allowed: HashSet<String>) -> Self {
        Self { allowed_methods: allowed }
    }
}

fn replacement_for(method: &str) -> Option<&'static str> {
    Some(match method {
        "class_variables" => "class_variable_defined?",
        "constants" => "const_defined?",
        "instance_methods" => "method_defined?",
        "private_instance_methods" => "private_method_defined?",
        "protected_instance_methods" => "protected_method_defined?",
        "public_instance_methods" => "public_method_defined?",
        "included_modules" => "include?",
        _ => return None,
    })
}

fn is_without_inherit(method: &str) -> bool {
    matches!(method, "class_variables" | "included_modules")
}

fn is_simple_arg(call: &ruby_prism::CallNode) -> bool {
    if let Some(args) = call.arguments() {
        for a in args.arguments().iter() {
            match a {
                Node::SplatNode { .. }
                | Node::BlockArgumentNode { .. }
                | Node::ForwardingArgumentsNode { .. } => return false,
                _ => {}
            }
        }
        // first arg cannot be a hash literal
        if let Some(first) = args.arguments().iter().next() {
            if matches!(first, Node::HashNode { .. } | Node::KeywordHashNode { .. }) {
                return false;
            }
        }
    }
    true
}

impl Cop for ModuleMemberExistenceCheck {
    fn name(&self) -> &'static str { "Style/ModuleMemberExistenceCheck" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, parent: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // parent must be `.include?` or `.member?`
        let parent_method = node_name!(parent);
        if parent_method.as_ref() != "include?" && parent_method.as_ref() != "member?" {
            return vec![];
        }

        // parent must have exactly 1 argument
        let parent_args_node = match parent.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let parent_args: Vec<_> = parent_args_node.arguments().iter().collect();
        if parent_args.len() != 1 {
            return vec![];
        }

        // parent receiver must be a CallNode with a member-replacement method name
        let recv = match parent.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let inner = match recv {
            Node::CallNode { .. } => recv.as_call_node().unwrap(),
            _ => return vec![],
        };
        let inner_method = node_name!(inner);
        let replacement = match replacement_for(&inner_method) {
            Some(r) => r,
            None => return vec![],
        };

        if self.allowed_methods.contains(inner_method.as_ref()) {
            return vec![];
        }

        // Check inner args: WITHOUT_INHERIT methods must have 0 args,
        // WITH_INHERIT methods must have 0 or 1 arg.
        let inner_args: Vec<_> = inner
            .arguments()
            .map(|a| a.arguments().iter().collect::<Vec<_>>())
            .unwrap_or_default();
        if is_without_inherit(&inner_method) {
            if !inner_args.is_empty() { return vec![]; }
        } else if inner_args.len() > 1 {
            return vec![];
        }

        // simple_method_argument? on both
        if !is_simple_arg(&inner) || !is_simple_arg(parent) {
            return vec![];
        }

        // Build offense: from inner's selector (message_loc) to end of parent.
        let inner_sel_loc = match inner.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let start = inner_sel_loc.start_offset();
        let end = parent.location().end_offset();

        // Build replacement: METHOD(arg) or METHOD(arg, inherit)
        let parent_arg_loc = parent_args[0].location();
        let parent_arg_src = &ctx.source[parent_arg_loc.start_offset()..parent_arg_loc.end_offset()];

        let new_src = if is_without_inherit(&inner_method)
            || inner_args.is_empty()
            || matches!(&inner_args[0], Node::TrueNode { .. })
        {
            format!("{}({})", replacement, parent_arg_src)
        } else {
            let inner_arg_loc = inner_args[0].location();
            let inner_arg_src = &ctx.source[inner_arg_loc.start_offset()..inner_arg_loc.end_offset()];
            format!("{}({}, {})", replacement, parent_arg_src, inner_arg_src)
        };

        let msg = format!("Use `{}` instead.", new_src);
        let offense = ctx
            .offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, new_src));
        vec![offense]
    }
}

crate::register_cop!("Style/ModuleMemberExistenceCheck", |cfg| {
    let allowed: HashSet<String> = cfg
        .get_cop_config("Style/ModuleMemberExistenceCheck")
        .and_then(|c| c.raw.get("AllowedMethods"))
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    Some(Box::new(ModuleMemberExistenceCheck::with_allowed(allowed)))
});
