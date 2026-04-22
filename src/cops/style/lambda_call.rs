//! Style/LambdaCall cop
//!
//! Checks for use of the lambda.(args) syntax.
//! Default style is `call` (prefer `.call()`), alternate is `braces` (prefer `.()`)

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::CallNode;

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Call,
    Braces,
}

pub struct LambdaCall {
    style: Style,
}

impl Default for LambdaCall {
    fn default() -> Self {
        Self { style: Style::Call }
    }
}

impl LambdaCall {
    pub fn new(style: Style) -> Self {
        Self { style }
    }

    fn is_implicit_call(node: &CallNode) -> bool {
        // Implicit call: lambda.(args) — method name is "call" but call_operator is "()"
        // In Prism, implicit call has no dot, just `.()`
        // Check: the dot location would be right before `(` with no method name chars
        // Actually Prism represents `x.(a)` as CallNode with name "call" and implicit_call flag
        // We detect by checking if there's a `call` selector in the source
        let method = node_name!(node);
        if method != "call" {
            return false;
        }
        // Implicit call has no selector location (the method name "call" isn't in source)
        node.message_loc().is_none()
    }

    fn prefer_str(&self, node: &CallNode, source: &str) -> String {
        let recv_start = node.receiver().unwrap().location().start_offset();
        let recv_end = node.receiver().unwrap().location().end_offset();
        let receiver = &source[recv_start..recv_end];

        let dot = if node.call_operator_loc().is_some() {
            let op_start = node.call_operator_loc().unwrap().start_offset();
            let op_end = node.call_operator_loc().unwrap().end_offset();
            &source[op_start..op_end]
        } else {
            "."
        };

        let args_str = if let Some(args_node) = node.arguments() {
            let parts: Vec<&str> = args_node
                .arguments()
                .iter()
                .map(|a| {
                    let s = a.location().start_offset();
                    let e = a.location().end_offset();
                    &source[s..e]
                })
                .collect();
            parts.join(", ")
        } else {
            String::new()
        };

        match self.style {
            Style::Call => {
                if args_str.is_empty() {
                    format!("{}{}{}", receiver, dot, "call")
                } else {
                    format!("{}{}call({})", receiver, dot, args_str)
                }
            }
            Style::Braces => {
                if args_str.is_empty() {
                    format!("{}{}()", receiver, dot)
                } else {
                    format!("{}{}({})", receiver, dot, args_str)
                }
            }
        }
    }

    fn is_offense(&self, node: &CallNode) -> bool {
        let method = node_name!(node);
        if method != "call" {
            return false;
        }
        let implicit = Self::is_implicit_call(node);
        match self.style {
            Style::Call => implicit,
            Style::Braces => !implicit,
        }
    }

    fn check_node(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node.receiver().is_none() {
            return vec![];
        }
        let method = node_name!(node);
        if method != "call" {
            return vec![];
        }
        if !self.is_offense(node) {
            return vec![];
        }
        let prefer = self.prefer_str(node, ctx.source);
        let current_start = node.location().start_offset();
        let current_end = node.location().end_offset();
        let current = &ctx.source[current_start..current_end];
        let msg = format!(
            "Prefer the use of `{}` over `{}`.",
            prefer, current
        );
        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), current_start, current_end)]
    }
}

impl Cop for LambdaCall {
    fn name(&self) -> &'static str {
        "Style/LambdaCall"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_node(node, ctx)
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/LambdaCall", |cfg| {
    let c: Cfg = cfg.typed("Style/LambdaCall");
    let style = match c.enforced_style.as_deref() {
        Some("braces") => Style::Braces,
        _ => Style::Call,
    };
    Some(Box::new(LambdaCall::new(style)))
});
