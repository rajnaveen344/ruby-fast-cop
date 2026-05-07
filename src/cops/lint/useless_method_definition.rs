//! Lint/UselessMethodDefinition - Detect method definitions that only call super.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

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
        // Pre-pass: build map def_start → parent call node range (for access modifier calls)
        let mut parent_call_map: HashMap<usize, (usize, usize)> = HashMap::new();
        {
            let mut pre = PreVisitor { parent_call_map: &mut parent_call_map };
            pre.visit_program_node(node);
        }
        let mut visitor = Visitor { ctx, offenses: Vec::new(), in_generic_method_arg: false, parent_call_map: &parent_call_map };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Pre-pass: for each access-modifier call (e.g. `private def foo; super; end`),
/// map def node start → call node (start, end).
struct PreVisitor<'a> {
    parent_call_map: &'a mut HashMap<usize, (usize, usize)>,
}

impl Visit<'_> for PreVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if ACCESS_MODIFIERS.contains(&method.as_ref()) && node.receiver().is_none() {
            if let Some(args) = node.arguments() {
                for arg in args.arguments().iter() {
                    if let Some(def_node) = arg.as_def_node() {
                        let nloc = node.location();
                        self.parent_call_map.insert(
                            def_node.location().start_offset(),
                            (nloc.start_offset(), nloc.end_offset()),
                        );
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

const ACCESS_MODIFIERS: &[&str] = &["public", "protected", "private", "module_function"];

struct Visitor<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    offenses: Vec<Offense>,
    /// True when we're visiting arguments of a non-access-modifier method call
    in_generic_method_arg: bool,
    parent_call_map: &'a HashMap<usize, (usize, usize)>,
}

impl Visit<'_> for Visitor<'_, '_> {
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


impl<'a, 'b> Visitor<'a, 'b> {
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

        let def_node_start = node.location().start_offset();
        let def_node_end = node.location().end_offset();

        // Determine deletion range: if inside access modifier call, use call range; else use def node range
        let (del_start, del_end) = self.parent_call_map
            .get(&def_node_start)
            .copied()
            .unwrap_or((def_node_start, def_node_end));

        let mut offense = self.ctx.offense_with_range(
            "Lint/UselessMethodDefinition",
            MSG,
            Severity::Warning,
            start,
            end,
        );
        // RuboCop uses corrector.remove(range) — delete exactly the node range (no whole-line)
        offense = offense.with_correction(Correction::delete(del_start, del_end));
        self.offenses.push(offense);
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
