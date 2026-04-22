//! Style/IfInsideElse cop
//!
//! If the else branch of a conditional consists solely of an if node,
//! it can be combined with the else to become an elsif.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Convert `if` nested inside `else` to `elsif`.";

pub struct IfInsideElse {
    allow_if_modifier: bool,
}

impl IfInsideElse {
    pub fn new(allow_if_modifier: bool) -> Self {
        Self { allow_if_modifier }
    }
}

impl Default for IfInsideElse {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Cop for IfInsideElse {
    fn name(&self) -> &'static str {
        "Style/IfInsideElse"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = IfInsideElseVisitor { ctx, cop: self, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct IfInsideElseVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a IfInsideElse,
    offenses: Vec<Offense>,
}

impl<'a> IfInsideElseVisitor<'a> {
    fn kw_src(&self, loc: &ruby_prism::Location) -> &str {
        &self.ctx.source[loc.start_offset()..loc.end_offset()]
    }

    fn is_ternary(&self, node: &ruby_prism::IfNode) -> bool {
        // Ternary: then_keyword_loc is `?`
        if let Some(then_loc) = node.then_keyword_loc() {
            self.kw_src(&then_loc) == "?"
        } else {
            false
        }
    }

    fn is_modifier_if(&self, node: &ruby_prism::IfNode) -> bool {
        // Modifier form: `foo if condition` — if_keyword appears AFTER statements
        if let Some(kw_loc) = node.if_keyword_loc() {
            if let Some(stmts) = node.statements() {
                let kw_start = kw_loc.start_offset();
                let body_start = stmts.location().start_offset();
                // Modifier: keyword comes after body
                return kw_start > body_start;
            }
        }
        false
    }
}

impl<'a> Visit<'_> for IfInsideElseVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Skip ternary
        if self.is_ternary(node) {
            ruby_prism::visit_if_node(self, node);
            return;
        }

        // Skip unless
        if let Some(kw_loc) = node.if_keyword_loc() {
            if self.kw_src(&kw_loc) == "unless" {
                ruby_prism::visit_if_node(self, node);
                return;
            }
        }

        // Check subsequent() — is it an ElseNode?
        if let Some(subsequent) = node.subsequent() {
            if let Some(else_node) = subsequent.as_else_node() {
                // Else body must be a single if node
                if let Some(stmts) = else_node.statements() {
                    let children: Vec<_> = stmts.body().iter().collect();
                    if children.len() == 1 {
                        if let Some(inner_if) = children[0].as_if_node() {
                            // Inner must be `if` not `unless`
                            let inner_kw_src = if let Some(kw) = inner_if.if_keyword_loc() {
                                self.kw_src(&kw).to_string()
                            } else {
                                String::new()
                            };

                            if inner_kw_src == "if" {
                                let is_modifier = self.is_modifier_if(&inner_if);
                                if !(self.cop.allow_if_modifier && is_modifier) {
                                    if let Some(kw_loc) = inner_if.if_keyword_loc() {
                                        let start = kw_loc.start_offset();
                                        let end = kw_loc.end_offset();
                                        self.offenses.push(self.ctx.offense_with_range(
                                            "Style/IfInsideElse",
                                            MSG,
                                            Severity::Convention,
                                            start,
                                            end,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        ruby_prism::visit_if_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_if_modifier: bool,
}

crate::register_cop!("Style/IfInsideElse", |cfg| {
    let c: Cfg = cfg.typed("Style/IfInsideElse");
    Some(Box::new(IfInsideElse::new(c.allow_if_modifier)))
});
