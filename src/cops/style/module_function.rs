//! Style/ModuleFunction cop
//!
//! Enforces consistent use of module_function vs extend self.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{ModuleNode, Node};

#[derive(Clone, Copy, PartialEq)]
enum Style {
    ModuleFunction,
    ExtendSelf,
    Forbidden,
}

pub struct ModuleFunction {
    style: Style,
}

impl Default for ModuleFunction {
    fn default() -> Self {
        Self { style: Style::ModuleFunction }
    }
}

impl ModuleFunction {
    pub fn new(style: Style) -> Self {
        Self { style }
    }

    /// Check if this CallNode is `extend self`
    /// In Prism: `extend self` is a CallNode with method "extend", no receiver,
    /// and one argument that is a SelfNode.
    fn is_extend_self(call: &ruby_prism::CallNode) -> bool {
        let name = node_name!(call);
        if name != "extend" { return false; }
        if call.receiver().is_some() { return false; }
        if let Some(args) = call.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() == 1 && arg_list[0].as_self_node().is_some() {
                return true;
            }
        }
        false
    }

    /// Check if this CallNode is bare `module_function` (no arguments)
    fn is_bare_module_function(call: &ruby_prism::CallNode) -> bool {
        let name = node_name!(call);
        if name != "module_function" { return false; }
        if call.receiver().is_some() { return false; }
        // No arguments = bare
        call.arguments().map(|a| a.arguments().len()).unwrap_or(0) == 0
    }

    /// Check if the body has `private` called (bare or with args)
    fn has_private_methods(stmts: &[Node]) -> bool {
        for node in stmts {
            if let Some(call) = node.as_call_node() {
                let name = node_name!(call);
                if name == "private" {
                    return true;
                }
            }
        }
        false
    }

    fn check_module_node(&self, node: &ModuleNode, ctx: &CheckContext) -> Vec<Offense> {
        let body = match node.body() {
            Some(b) => b,
            None => return vec![],
        };
        let stmts_node = match body.as_statements_node() {
            Some(s) => s,
            None => return vec![],
        };
        let stmts: Vec<_> = stmts_node.body().iter().collect();

        let mut offenses = Vec::new();

        for stmt in &stmts {
            if let Some(call) = stmt.as_call_node() {
                match self.style {
                    Style::ModuleFunction => {
                        if Self::is_extend_self(&call) {
                            // Skip if module has private methods
                            if !Self::has_private_methods(&stmts) {
                                offenses.push(ctx.offense(
                                    self.name(),
                                    "Use `module_function` instead of `extend self`.",
                                    self.severity(),
                                    &call.location(),
                                ));
                            }
                        }
                    }
                    Style::ExtendSelf => {
                        if Self::is_bare_module_function(&call) {
                            offenses.push(ctx.offense(
                                self.name(),
                                "Use `extend self` instead of `module_function`.",
                                self.severity(),
                                &call.location(),
                            ));
                        }
                    }
                    Style::Forbidden => {
                        if Self::is_bare_module_function(&call) {
                            offenses.push(ctx.offense(
                                self.name(),
                                "Do not use `module_function` or `extend self`.",
                                self.severity(),
                                &call.location(),
                            ));
                        }
                        if Self::is_extend_self(&call) {
                            offenses.push(ctx.offense(
                                self.name(),
                                "Do not use `module_function` or `extend self`.",
                                self.severity(),
                                &call.location(),
                            ));
                        }
                    }
                }
            }
        }

        offenses
    }
}

impl Cop for ModuleFunction {
    fn name(&self) -> &'static str {
        "Style/ModuleFunction"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_module(&self, node: &ModuleNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_module_node(node, ctx)
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

crate::register_cop!("Style/ModuleFunction", |cfg| {
    let c: Cfg = cfg.typed("Style/ModuleFunction");
    let style = match c.enforced_style.as_str() {
        "extend_self" => Style::ExtendSelf,
        "forbidden" => Style::Forbidden,
        _ => Style::ModuleFunction,
    };
    Some(Box::new(ModuleFunction::new(style)))
});
