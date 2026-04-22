//! Style/IfUnlessModifierOfIfUnless cop
//!
//! Flags modifier if/unless applied to another conditional (if/unless/ternary).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{IfNode, UnlessNode, Node, Visit};

#[derive(Default)]
pub struct IfUnlessModifierOfIfUnless;

impl IfUnlessModifierOfIfUnless {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for IfUnlessModifierOfIfUnless {
    fn name(&self) -> &'static str {
        "Style/IfUnlessModifierOfIfUnless"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ModifierOfIfUnlessVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct ModifierOfIfUnlessVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

fn is_modifier_if(node: &IfNode) -> bool {
    // Modifier `if`: the if keyword comes AFTER the statements (body)
    if let Some(kw) = node.if_keyword_loc() {
        if kw.as_slice() != b"if" {
            return false;
        }
        // For modifier form, body/statements appear before the keyword
        if let Some(stmts) = node.statements() {
            let parts: Vec<_> = stmts.body().iter().collect();
            if let Some(first) = parts.first() {
                return first.location().start_offset() < kw.start_offset();
            }
        }
        // No statements before keyword = not modifier
        false
    } else {
        false
    }
}

fn is_modifier_unless(node: &UnlessNode) -> bool {
    // Modifier `unless`: body statements appear before the keyword
    let kw = node.keyword_loc();
    if let Some(stmts) = node.statements() {
        let parts: Vec<_> = stmts.body().iter().collect();
        if let Some(first) = parts.first() {
            return first.location().start_offset() < kw.start_offset();
        }
    }
    false
}

fn is_body_conditional(node: &Node) -> bool {
    matches!(node, Node::IfNode { .. } | Node::UnlessNode { .. })
}

impl<'a> ModifierOfIfUnlessVisitor<'a> {
    fn check_if_modifier(&mut self, node: &IfNode, keyword: &str) {
        let stmts = match node.statements() {
            Some(s) => s,
            None => return,
        };
        let parts: Vec<_> = stmts.body().iter().collect();
        if parts.len() != 1 {
            return;
        }
        let body = &parts[0];

        if !is_body_conditional(body) {
            return;
        }

        let msg = format!("Avoid modifier `{}` after another conditional.", keyword);
        let kw_loc = node.if_keyword_loc().unwrap();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/IfUnlessModifierOfIfUnless",
            &msg,
            Severity::Convention,
            kw_loc.start_offset(),
            kw_loc.end_offset(),
        ));
    }

    fn check_unless_modifier(&mut self, node: &UnlessNode) {
        let stmts = match node.statements() {
            Some(s) => s,
            None => return,
        };
        let parts: Vec<_> = stmts.body().iter().collect();
        if parts.len() != 1 {
            return;
        }
        let body = &parts[0];

        if !is_body_conditional(body) {
            return;
        }

        let msg = "Avoid modifier `unless` after another conditional.";
        let kw_loc = node.keyword_loc();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/IfUnlessModifierOfIfUnless",
            msg,
            Severity::Convention,
            kw_loc.start_offset(),
            kw_loc.end_offset(),
        ));
    }
}

impl Visit<'_> for ModifierOfIfUnlessVisitor<'_> {
    fn visit_if_node(&mut self, node: &IfNode) {
        if is_modifier_if(node) {
            self.check_if_modifier(node, "if");
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &UnlessNode) {
        if is_modifier_unless(node) {
            self.check_unless_modifier(node);
        }
        ruby_prism::visit_unless_node(self, node);
    }
}

crate::register_cop!("Style/IfUnlessModifierOfIfUnless", |_cfg| {
    Some(Box::new(IfUnlessModifierOfIfUnless::new()))
});
