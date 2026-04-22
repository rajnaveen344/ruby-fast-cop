//! Lint/UselessMethodDefinition - Detect method definitions that only call super.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Useless method definition detected.";

#[derive(Default)]
pub struct UselessMethodDefinition;

impl UselessMethodDefinition {
    pub fn new() -> Self { Self }
}

impl Cop for UselessMethodDefinition {
    fn name(&self) -> &'static str { "Lint/UselessMethodDefinition" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new(), in_generic_method_arg: false };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

const ACCESS_MODIFIERS: &[&str] = &["public", "protected", "private", "module_function"];

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// True when we're visiting arguments of a non-access-modifier method call
    in_generic_method_arg: bool,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if !self.in_generic_method_arg {
            self.check_def(node);
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        let is_access_modifier = ACCESS_MODIFIERS.contains(&method.as_ref());

        if is_access_modifier {
            // Visit arguments normally — def inside access modifier IS flagged
            ruby_prism::visit_call_node(self, node);
        } else {
            // Mark that any def inside our args is a generic method arg
            let prev = self.in_generic_method_arg;
            // Visit receiver normally
            if let Some(recv) = node.receiver() {
                self.visit(&recv);
            }
            // Visit arguments with flag set
            if let Some(args) = node.arguments() {
                self.in_generic_method_arg = true;
                for arg in args.arguments().iter() {
                    self.visit(&arg);
                }
                self.in_generic_method_arg = prev;
            }
            // Visit block normally
            if let Some(block) = node.block() {
                self.visit(&block);
            }
        }
    }
}

impl<'a> Visitor<'a> {
    fn check_def(&mut self, node: &ruby_prism::DefNode) {
        // Skip initialize (any form)
        let name = String::from_utf8_lossy(node.name().as_slice());
        if name.as_ref() == "initialize" {
            return;
        }

        // Must have a body
        let body = match node.body() {
            Some(b) => b,
            None => return,
        };

        // Body must be a StatementsNode with exactly one statement
        let stmts = match body.as_statements_node() {
            Some(s) => s,
            None => return,
        };
        let stmt_list: Vec<_> = stmts.body().iter().collect();
        if stmt_list.len() != 1 {
            return;
        }

        let stmt = &stmt_list[0];

        // Get method params info
        let params = get_required_params(node);
        let has_complex_params = has_rest_or_optional(node);

        let is_useless = match stmt {
            Node::ForwardingSuperNode { .. } => {
                // Bare `super` — useless unless method has rest/optional args
                !has_complex_params
            }
            Node::SuperNode { .. } => {
                let super_node = stmt.as_super_node().unwrap();
                let super_args = get_super_args(&super_node);

                if has_complex_params {
                    return; // complex params → not useless
                }

                // super() with no method params → useless
                if params.is_empty() && super_args.is_empty() {
                    true
                } else if params.is_empty() && !super_args.is_empty() {
                    // method has no params but super passes args → not useless
                    false
                } else {
                    // super args must match method params exactly
                    super_args == params
                }
            }
            _ => return,
        };

        if !is_useless {
            return;
        }

        // Offense range: from `def` keyword to end of method signature
        let def_loc = node.def_keyword_loc();
        let start = def_loc.start_offset();
        let end = if let Some(rparen) = node.rparen_loc() {
            rparen.end_offset()
        } else {
            node.name_loc().end_offset()
        };

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/UselessMethodDefinition",
            MSG,
            Severity::Warning,
            start,
            end,
        ));
    }
}

/// Returns list of required parameter names in order.
fn get_required_params(node: &ruby_prism::DefNode) -> Vec<String> {
    let params_node = match node.parameters() {
        Some(p) => p,
        None => return vec![],
    };

    let mut result = Vec::new();
    for req in params_node.requireds().iter() {
        if let Some(rp) = req.as_required_parameter_node() {
            let name = String::from_utf8_lossy(rp.name().as_slice()).into_owned();
            result.push(name);
        }
    }
    result
}

/// Returns true if method has rest, optional, or optional keyword params.
fn has_rest_or_optional(node: &ruby_prism::DefNode) -> bool {
    let params_node = match node.parameters() {
        Some(p) => p,
        None => return false,
    };

    // rest (*args)
    if params_node.rest().is_some() {
        return true;
    }
    // optional positional (x = 1)
    let optional_count: usize = params_node.optionals().iter().count();
    if optional_count > 0 {
        return true;
    }
    // optional keyword (x: 1)
    for kw in params_node.keywords().iter() {
        if kw.as_optional_keyword_parameter_node().is_some() {
            return true;
        }
    }
    // keyword_rest (**kwargs)
    if params_node.keyword_rest().is_some() {
        return true;
    }

    false
}

/// Returns list of argument names passed to super.
fn get_super_args(super_node: &ruby_prism::SuperNode) -> Vec<String> {
    let args = match super_node.arguments() {
        Some(a) => a,
        None => return vec![],
    };
    let mut result = Vec::new();
    for arg in args.arguments().iter() {
        if let Some(lvar) = arg.as_local_variable_read_node() {
            let name = String::from_utf8_lossy(lvar.name().as_slice()).into_owned();
            result.push(name);
        } else {
            // Non-variable arg → can't be a simple forwarding
            result.push("__non_local__".to_string());
        }
    }
    result
}

crate::register_cop!("Lint/UselessMethodDefinition", |_cfg| Some(Box::new(UselessMethodDefinition::new())));
