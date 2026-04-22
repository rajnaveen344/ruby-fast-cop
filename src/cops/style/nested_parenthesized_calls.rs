//! Style/NestedParenthesizedCalls cop
//!
//! Checks for unparenthesized method calls in the argument list of a parenthesized method call.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/NestedParenthesizedCalls";

pub struct NestedParenthesizedCalls {
    allowed_methods: Vec<String>,
}

impl Default for NestedParenthesizedCalls {
    fn default() -> Self {
        Self::new()
    }
}

impl NestedParenthesizedCalls {
    pub fn new() -> Self {
        Self {
            allowed_methods: default_allowed(),
        }
    }

    pub fn with_config(allowed_methods: Vec<String>) -> Self {
        Self { allowed_methods }
    }
}

fn default_allowed() -> Vec<String> {
    vec![
        "be".to_string(),
        "be_a".to_string(),
        "be_an".to_string(),
        "be_between".to_string(),
        "be_falsey".to_string(),
        "be_kind_of".to_string(),
        "be_instance_of".to_string(),
        "be_truthy".to_string(),
        "be_within".to_string(),
        "eq".to_string(),
        "eql".to_string(),
        "end_with".to_string(),
        "include".to_string(),
        "match".to_string(),
        "raise_error".to_string(),
        "respond_to".to_string(),
        "start_with".to_string(),
    ]
}

impl Cop for NestedParenthesizedCalls {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = NestedParenVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct NestedParenVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a NestedParenthesizedCalls,
    offenses: Vec<Offense>,
}

impl<'a> NestedParenVisitor<'a> {
    /// Check if the call is parenthesized (has opening paren)
    fn is_parenthesized(call: &ruby_prism::CallNode) -> bool {
        call.opening_loc().is_some()
    }

    /// Check if the nested call is "allowed omission"
    fn allowed_omission(&self, nested: &ruby_prism::CallNode, parent_args_count: usize) -> bool {
        // No arguments: allowed
        if nested.arguments().is_none() {
            return true;
        }
        // Already parenthesized: allowed
        if Self::is_parenthesized(nested) {
            return true;
        }
        // Setter method: allowed
        let name = node_name!(nested);
        if name.ends_with('=') && name != "==" && name != "!=" {
            return true;
        }
        // Operator method: allowed
        let is_op = matches!(
            name.as_ref(),
            "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>" | "+" | "-" | "*" | "/" | "%"
            | "**" | "<<" | ">>" | "&" | "|" | "^" | "[]" | "[]="
        );
        if is_op {
            return true;
        }
        // Allowed method: parent has single arg AND nested has single arg
        if parent_args_count == 1 {
            if let Some(args) = nested.arguments() {
                let arg_count = args.arguments().iter().count();
                if arg_count == 1 {
                    let method_name = node_name!(nested);
                    if self.cop.allowed_methods.contains(&method_name.to_string()) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        if !Self::is_parenthesized(node) {
            return;
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };

        let all_args: Vec<Node> = args.arguments().iter().collect();
        let parent_args_count = all_args.len();

        for arg in &all_args {
            let nested = match arg.as_call_node() {
                Some(c) => c,
                None => continue,
            };
            if self.allowed_omission(&nested, parent_args_count) {
                continue;
            }
            let nested_src = self.ctx.src(
                nested.location().start_offset(),
                nested.location().end_offset(),
            );
            let msg = format!("Add parentheses to nested method call `{nested_src}`.");
            let start = nested.location().start_offset();
            let end = nested.location().end_offset();
            self.offenses.push(self.ctx.offense_with_range(
                COP_NAME, &msg, Severity::Convention, start, end,
            ));
        }
    }
}

impl Visit<'_> for NestedParenVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/NestedParenthesizedCalls", |cfg| {
    let cop_config = cfg.get_cop_config("Style/NestedParenthesizedCalls");
    let allowed_methods = cop_config
        .and_then(|c| c.raw.get("AllowedMethods"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(default_allowed);
    Some(Box::new(NestedParenthesizedCalls::with_config(allowed_methods)))
});
