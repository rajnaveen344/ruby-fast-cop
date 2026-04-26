//! Style/InvertibleUnlessCondition cop
//!
//! Suggests inverting `unless` conditions to use `if`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;
use std::collections::HashMap;

pub struct InvertibleUnlessCondition {
    inverse_methods: HashMap<String, String>,
}

impl Default for InvertibleUnlessCondition {
    fn default() -> Self {
        Self { inverse_methods: HashMap::new() }
    }
}

impl InvertibleUnlessCondition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(inverse_methods: HashMap<String, String>) -> Self {
        Self { inverse_methods }
    }

    /// Strip outer ParenthesesNode (with its single-statement body) once.
    fn strip_one_paren<'a>(node: &Node<'a>) -> Option<Node<'a>> {
        let p = node.as_parentheses_node()?;
        let body = p.body()?;
        if let Some(stmts) = body.as_statements_node() {
            let v: Vec<_> = stmts.body().iter().collect();
            if v.len() == 1 {
                return Some(v.into_iter().next().unwrap());
            }
            return None;
        }
        Some(body)
    }

    fn invertible(&self, node: &Node, source: &str) -> bool {
        match node {
            Node::ParenthesesNode { .. } => match Self::strip_one_paren(node) {
                Some(inner) => self.invertible(&inner, source),
                None => false,
            },
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if Self::inheritance_check(&call, source) {
                    return false;
                }
                let m = node_name!(call).to_string();
                if m == "!" {
                    return true;
                }
                self.inverse_methods.contains_key(&m)
            }
            Node::OrNode { .. } => {
                let o = node.as_or_node().unwrap();
                self.invertible(&o.left(), source) && self.invertible(&o.right(), source)
            }
            Node::AndNode { .. } => {
                let a = node.as_and_node().unwrap();
                self.invertible(&a.left(), source) && self.invertible(&a.right(), source)
            }
            _ => false,
        }
    }

    /// Whether `x < Foo` style inheritance check (skip).
    fn inheritance_check(call: &ruby_prism::CallNode, _source: &str) -> bool {
        let m = node_name!(call);
        if m != "<" {
            return false;
        }
        let args = match call.arguments() {
            Some(a) => a,
            None => return false,
        };
        let arg = match args.arguments().iter().next() {
            Some(a) => a,
            None => return false,
        };
        // Argument must be a constant whose short name (last segment) is all uppercase.
        let short_name = match &arg {
            Node::ConstantReadNode { .. } => {
                let c = arg.as_constant_read_node().unwrap();
                Some(String::from_utf8_lossy(c.name().as_slice()).into_owned())
            }
            Node::ConstantPathNode { .. } => {
                let c = arg.as_constant_path_node().unwrap();
                c.name().map(|n| String::from_utf8_lossy(n.as_slice()).into_owned())
            }
            _ => None,
        };
        let short = match short_name {
            Some(s) => s,
            None => return false,
        };
        // Skip when name has lowercase chars (i.e. constant of class type, like `Foo`).
        // RuboCop: `argument.short_name.to_s.upcase != argument.short_name.to_s` → flag (i.e. invertible).
        // We want inverse: skip-when-inheritance, so return true when name has lowercase chars.
        short.to_uppercase() != short
    }

    fn preferred_condition(&self, node: &Node, source: &str) -> String {
        match node {
            Node::ParenthesesNode { .. } => {
                if let Some(inner) = Self::strip_one_paren(node) {
                    return format!("({})", self.preferred_condition(&inner, source));
                }
                node_source(node, source).to_string()
            }
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                self.preferred_send(&c, source)
            }
            Node::OrNode { .. } => {
                let o = node.as_or_node().unwrap();
                let lhs = self.preferred_condition(&o.left(), source);
                let rhs = self.preferred_condition(&o.right(), source);
                format!("{} && {}", lhs, rhs)
            }
            Node::AndNode { .. } => {
                let a = node.as_and_node().unwrap();
                let lhs = self.preferred_condition(&a.left(), source);
                let rhs = self.preferred_condition(&a.right(), source);
                format!("{} || {}", lhs, rhs)
            }
            _ => node_source(node, source).to_string(),
        }
    }

    fn preferred_send(&self, call: &ruby_prism::CallNode, source: &str) -> String {
        let method = node_name!(call).to_string();
        let receiver_src = call
            .receiver()
            .map(|r| node_source(&r, source).to_string());

        if method == "!" {
            // `!x` → `x` (just the receiver source)
            return receiver_src.unwrap_or_default();
        }

        let inverse = match self.inverse_methods.get(&method) {
            Some(s) => s.clone(),
            None => method.clone(),
        };

        let dotted_receiver = receiver_src
            .as_ref()
            .map(|r| format!("{}.", r))
            .unwrap_or_default();

        // No-arg call
        let args = match call.arguments() {
            Some(a) => a,
            None => {
                return format!("{}{}", dotted_receiver, inverse);
            }
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return format!("{}{}", dotted_receiver, inverse);
        }
        let arg_src: Vec<String> = arg_list.iter().map(|a| node_source(a, source).to_string()).collect();
        let argument_list = arg_src.join(", ");

        // Operator method (binary op like `!=`, `<`, `>=`)
        if Self::is_operator_method(&method) {
            let recv = receiver_src.clone().unwrap_or_default();
            return format!("{} {} {}", recv, inverse, argument_list);
        }

        // Parenthesized?
        let is_paren = call.opening_loc().is_some();
        if is_paren {
            return format!("{}{}({})", dotted_receiver, inverse, argument_list);
        }
        format!("{}{} {}", dotted_receiver, inverse, argument_list)
    }

    fn is_operator_method(name: &str) -> bool {
        matches!(
            name,
            "+" | "-" | "*" | "/" | "%" | "**" | "==" | "!=" | "<" | "<=" | ">" | ">="
                | "<<" | ">>" | "&" | "|" | "^" | "<=>" | "===" | "=~"
        )
    }
}

fn node_source<'a>(node: &Node<'a>, source: &'a str) -> &'a str {
    let loc = node.location();
    &source[loc.start_offset()..loc.end_offset()]
}

impl Cop for InvertibleUnlessCondition {
    fn name(&self) -> &'static str {
        "Style/InvertibleUnlessCondition"
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        let cond = node.predicate();
        if !self.invertible(&cond, ctx.source) {
            return vec![];
        }
        let preferred = self.preferred_condition(&cond, ctx.source);
        let cond_src = node_source(&cond, ctx.source);
        let message = format!(
            "Prefer `if {}` over `unless {}`.",
            preferred, cond_src
        );
        let loc = node.location();
        vec![ctx.offense_with_range(
            self.name(),
            &message,
            Severity::Convention,
            loc.start_offset(),
            loc.end_offset(),
        )]
    }
}

crate::register_cop!("Style/InvertibleUnlessCondition", |cfg| {
    let raw = cfg
        .get_cop_config("Style/InvertibleUnlessCondition")
        .and_then(|c| c.raw.get("InverseMethods"))
        .and_then(|v| v.as_mapping());
    let mut map: HashMap<String, String> = HashMap::new();
    if let Some(m) = raw {
        for (k, v) in m {
            // YAML keys can be plain or symbols — try as_str then strip leading `:`.
            let key = match k.as_str() {
                Some(s) => s.trim_start_matches(':').to_string(),
                None => continue,
            };
            let val = match v.as_str() {
                Some(s) => s.trim_start_matches(':').to_string(),
                None => continue,
            };
            map.insert(key, val);
        }
    }
    Some(Box::new(InvertibleUnlessCondition::with_config(map)))
});
