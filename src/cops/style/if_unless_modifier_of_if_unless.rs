//! Style/IfUnlessModifierOfIfUnless cop
//!
//! Flags modifier if/unless applied to another conditional (if/unless/ternary).

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{IfNode, Node, UnlessNode, Visit};

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
            correction_covered: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct ModifierOfIfUnlessVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Ranges already covered by an outer recursive correction.
    /// Inner nodes in these ranges should not emit their own correction.
    correction_covered: Vec<(usize, usize)>,
}

fn is_modifier_if(node: &IfNode) -> bool {
    if let Some(kw) = node.if_keyword_loc() {
        if kw.as_slice() != b"if" {
            return false;
        }
        if let Some(stmts) = node.statements() {
            let parts: Vec<_> = stmts.body().iter().collect();
            if let Some(first) = parts.first() {
                return first.location().start_offset() < kw.start_offset();
            }
        }
        false
    } else {
        false
    }
}

fn is_modifier_unless(node: &UnlessNode) -> bool {
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

fn source_slice(source: &str, start: usize, end: usize) -> String {
    source.get(start..end).unwrap_or("").to_string()
}

/// Recursively normalize a body node: if it's a modifier if/unless, expand to block form.
fn normalize_node_source(body: &Node, source: &str) -> String {
    match body {
        Node::IfNode { .. } => {
            let node = body.as_if_node().unwrap();
            if is_modifier_if(&node) {
                if let Some(stmts) = node.statements() {
                    let parts: Vec<_> = stmts.body().iter().collect();
                    if parts.len() == 1 {
                        let inner_body = &parts[0];
                        let cond = node.predicate();
                        let cond_src = source_slice(
                            source,
                            cond.location().start_offset(),
                            cond.location().end_offset(),
                        );
                        let body_norm = normalize_node_source(inner_body, source);
                        return format!("if {}\n{}\nend", cond_src, body_norm);
                    }
                }
            }
            source_slice(
                source,
                body.location().start_offset(),
                body.location().end_offset(),
            )
        }
        Node::UnlessNode { .. } => {
            let node = body.as_unless_node().unwrap();
            if is_modifier_unless(&node) {
                if let Some(stmts) = node.statements() {
                    let parts: Vec<_> = stmts.body().iter().collect();
                    if parts.len() == 1 {
                        let inner_body = &parts[0];
                        let cond = node.predicate();
                        let cond_src = source_slice(
                            source,
                            cond.location().start_offset(),
                            cond.location().end_offset(),
                        );
                        let body_norm = normalize_node_source(inner_body, source);
                        return format!("unless {}\n{}\nend", cond_src, body_norm);
                    }
                }
            }
            source_slice(
                source,
                body.location().start_offset(),
                body.location().end_offset(),
            )
        }
        _ => source_slice(
            source,
            body.location().start_offset(),
            body.location().end_offset(),
        ),
    }
}

impl<'a> ModifierOfIfUnlessVisitor<'a> {
    fn is_covered(&self, start: usize, end: usize) -> bool {
        self.correction_covered
            .iter()
            .any(|&(cs, ce)| start >= cs && end <= ce)
    }

    fn build_correction_for_if(
        &mut self,
        node: &IfNode,
        keyword: &str,
        body: &Node,
    ) -> Option<Correction> {
        let source = self.ctx.source;
        let cond = node.predicate();
        let cond_src = source_slice(
            source,
            cond.location().start_offset(),
            cond.location().end_offset(),
        );
        let body_normalized = normalize_node_source(body, source);
        let replacement = format!("{} {}\n{}\nend", keyword, cond_src, body_normalized);
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        // Mark this range as covered so inner nodes skip their corrections
        self.correction_covered.push((node_start, node_end));
        Some(Correction::replace(node_start, node_end, replacement))
    }

    fn build_correction_for_unless(
        &mut self,
        node: &UnlessNode,
        body: &Node,
    ) -> Option<Correction> {
        let source = self.ctx.source;
        let cond = node.predicate();
        let cond_src = source_slice(
            source,
            cond.location().start_offset(),
            cond.location().end_offset(),
        );
        let body_normalized = normalize_node_source(body, source);
        let replacement = format!("unless {}\n{}\nend", cond_src, body_normalized);
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        self.correction_covered.push((node_start, node_end));
        Some(Correction::replace(node_start, node_end, replacement))
    }

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
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        // Only emit correction if this node is not already covered by an outer correction
        let correction = if !self.is_covered(node_start, node_end) {
            self.build_correction_for_if(node, keyword, body)
        } else {
            None
        };

        let offense = self
            .ctx
            .offense_with_range(
                "Style/IfUnlessModifierOfIfUnless",
                &msg,
                Severity::Convention,
                kw_loc.start_offset(),
                kw_loc.end_offset(),
            );
        let offense = if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        };
        self.offenses.push(offense);
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
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        let correction = if !self.is_covered(node_start, node_end) {
            self.build_correction_for_unless(node, body)
        } else {
            None
        };

        let offense = self
            .ctx
            .offense_with_range(
                "Style/IfUnlessModifierOfIfUnless",
                msg,
                Severity::Convention,
                kw_loc.start_offset(),
                kw_loc.end_offset(),
            );
        let offense = if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        };
        self.offenses.push(offense);
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
