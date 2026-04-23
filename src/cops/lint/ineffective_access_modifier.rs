//! Lint/IneffectiveAccessModifier cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/ineffective_access_modifier.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct IneffectiveAccessModifier;

impl IneffectiveAccessModifier {
    pub fn new() -> Self { Self }
}

const MSG_PRIVATE: &str = "`private` (on line {}) does not make singleton methods private. Use `private_class_method` or `private` inside a `class << self` block instead.";
const MSG_PROTECTED: &str = "`protected` (on line {}) does not make singleton methods protected. Use `protected` inside a `class << self` block instead.";

impl Cop for IneffectiveAccessModifier {
    fn name(&self) -> &'static str { "Lint/IneffectiveAccessModifier" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_class(&self, node: &ruby_prism::ClassNode, ctx: &CheckContext) -> Vec<Offense> {
        check_body(node.body(), ctx)
    }

    fn check_module(&self, node: &ruby_prism::ModuleNode, ctx: &CheckContext) -> Vec<Offense> {
        check_body(node.body(), ctx)
    }
}

fn check_body(body: Option<Node>, ctx: &CheckContext) -> Vec<Offense> {
    let body = match body {
        Some(b) => b,
        None => return vec![],
    };
    let stmts = match body.as_statements_node() {
        Some(s) => s,
        None => return vec![],
    };

    let mut offenses = Vec::new();
    let mut current_modifier: Option<(String, usize)> = None; // (name, line)

    // Collect private_class_method names in this body
    let private_class_methods: Vec<String> = stmts.body().iter().filter_map(|stmt| {
        if let Some(call) = stmt.as_call_node() {
            let method = node_name!(call);
            if method == "private_class_method" {
                if let Some(args) = call.arguments() {
                    return Some(args.arguments().iter().filter_map(|arg| {
                        if let Some(sym) = arg.as_symbol_node() {
                            let loc = sym.value_loc().unwrap_or_else(|| sym.location());
                            Some(ctx.src(loc.start_offset(), loc.end_offset()).to_string())
                        } else {
                            None
                        }
                    }).collect::<Vec<_>>());
                }
            }
        }
        None
    }).flatten().collect();

    for stmt in stmts.body().iter() {
        match &stmt {
            Node::CallNode { .. } => {
                let call = stmt.as_call_node().unwrap();
                let method = node_name!(call);
                match method.as_ref() {
                    "private" | "protected" if call.arguments().is_none() && call.receiver().is_none() => {
                        let line = ctx.line_of(call.location().start_offset());
                        current_modifier = Some((method.to_string(), line));
                    }
                    _ => {
                        // Non-modifier call — but don't clear modifier per RuboCop
                        // (it persists across intervening code)
                    }
                }
            }
            Node::DefNode { .. } => {
                let def_node = stmt.as_def_node().unwrap();
                // Check if this is a singleton method (def self.method)
                let is_singleton = def_node.receiver().map(|r| {
                    matches!(r, Node::SelfNode { .. })
                }).unwrap_or(false);

                if is_singleton {
                    if let Some((ref modifier, mod_line)) = current_modifier {
                        // Check if this method is exempted via private_class_method
                        let method_name = node_name!(def_node).to_string();
                        let is_exempted = private_class_methods.contains(&method_name);

                        if !is_exempted {
                            let def_kw_loc = def_node.def_keyword_loc();
                            let msg = if modifier == "private" {
                                MSG_PRIVATE.replace("{}", &mod_line.to_string())
                            } else {
                                MSG_PROTECTED.replace("{}", &mod_line.to_string())
                            };
                            offenses.push(ctx.offense_with_range(
                                "Lint/IneffectiveAccessModifier",
                                &msg,
                                Severity::Warning,
                                def_kw_loc.start_offset(),
                                def_kw_loc.end_offset(),
                            ));
                        }
                    }
                }
            }
            Node::SingletonClassNode { .. } => {
                // class << self block — don't apply the modifier to methods inside it
                // Clear modifier context? No — per TOML test, class << self simply doesn't trigger offenses
                // but we don't clear the modifier (it still applies to defs after the block)
            }
            _ => {}
        }
    }
    offenses
}

crate::register_cop!("Lint/IneffectiveAccessModifier", |_cfg| {
    Some(Box::new(IneffectiveAccessModifier::new()))
});
