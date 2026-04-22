//! Style/RedundantConditional cop
//!
//! Checks for conditionals that return true/false and can be simplified.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/RedundantConditional";

#[derive(Default)]
pub struct RedundantConditional;

impl RedundantConditional {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantConditional {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RedundantConditionalVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct RedundantConditionalVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

fn is_true_node(node: &Node) -> bool {
    matches!(node, Node::TrueNode { .. })
}

fn is_false_node(node: &Node) -> bool {
    matches!(node, Node::FalseNode { .. })
}

/// Get the single body node of statements if there's exactly one.
/// Returns (start, end) offsets so we can check without cloning.
fn single_body_offsets<'pr>(stmts: &ruby_prism::StatementsNode<'pr>) -> Option<(usize, usize, bool, bool)> {
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 { return None; }
    let node = &body[0];
    Some((
        node.location().start_offset(),
        node.location().end_offset(),
        is_true_node(node),
        is_false_node(node),
    ))
}

impl<'a> RedundantConditionalVisitor<'a> {
    fn src(&self, start: usize, end: usize) -> &'a str {
        self.ctx.src(start, end)
    }

    fn is_ternary(node: &ruby_prism::IfNode, ctx: &CheckContext) -> bool {
        let loc = node.location();
        let s = ctx.src(loc.start_offset(), loc.end_offset());
        !s.starts_with("if") && !s.starts_with("elsif") && !s.starts_with("unless")
    }

    fn is_modifier(node: &ruby_prism::IfNode, ctx: &CheckContext) -> bool {
        // modifier: no `end` keyword and not ternary
        node.end_keyword_loc().is_none() && !Self::is_ternary(node, ctx)
    }

    fn is_elsif(node: &ruby_prism::IfNode, ctx: &CheckContext) -> bool {
        let start = node.location().start_offset();
        ctx.source[start..].starts_with("elsif")
    }

    fn check_if(&mut self, node: &ruby_prism::IfNode) {
        if Self::is_modifier(node, self.ctx) {
            return;
        }
        if Self::is_elsif(node, self.ctx) {
            return;
        }

        let is_ternary = Self::is_ternary(node, self.ctx);

        // Get single if-body info
        let if_info = match node.statements().and_then(|s| single_body_offsets(&s)) {
            Some(i) => i,
            None => return,
        };
        let (_, _, if_is_true, if_is_false) = if_info;

        // Check for elsif branch
        if let Some(sub) = node.subsequent() {
            if let Some(elsif) = sub.as_if_node() {
                self.check_elsif(&elsif);
                return;
            }
        }

        // Get single else-body info
        let else_info = match node.subsequent()
            .and_then(|s| s.as_else_node())
            .and_then(|e| e.statements())
            .and_then(|s| single_body_offsets(&s))
        {
            Some(i) => i,
            None => return,
        };
        let (_, _, else_is_true, else_is_false) = else_info;

        let cond = node.predicate();
        let cond_src = self.src(cond.location().start_offset(), cond.location().end_offset());

        let (start, end) = if is_ternary {
            (node.location().start_offset(), node.location().end_offset())
        } else {
            (node.location().start_offset(), cond.location().end_offset())
        };

        // cond ? true : false → cond
        if if_is_true && else_is_false {
            let msg = format!("This conditional expression can just be replaced by `{cond_src}`.");
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, end));
            return;
        }

        // cond ? false : true → !(cond)
        if if_is_false && else_is_true {
            let msg = format!("This conditional expression can just be replaced by `!({cond_src})`.");
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, end));
        }
    }

    fn check_elsif(&mut self, elsif: &ruby_prism::IfNode) {
        // elsif must have exactly one body and an else (not another elsif)
        let if_info = match elsif.statements().and_then(|s| single_body_offsets(&s)) {
            Some(i) => i,
            None => return,
        };
        let (_, _, if_is_true, if_is_false) = if_info;

        // Must have a plain else, not another elsif
        let sub = match elsif.subsequent() {
            Some(s) => s,
            None => return,
        };
        if sub.as_if_node().is_some() {
            return;
        }

        let else_info = match sub.as_else_node()
            .and_then(|e| e.statements())
            .and_then(|s| single_body_offsets(&s))
        {
            Some(i) => i,
            None => return,
        };
        let (_, _, else_is_true, else_is_false) = else_info;

        let cond = elsif.predicate();
        let cond_src = self.src(cond.location().start_offset(), cond.location().end_offset());

        let start = elsif.location().start_offset();
        let cond_end = cond.location().end_offset();

        if if_is_true && else_is_false {
            let replacement = format!("\nelse\n  {cond_src}");
            let msg = format!("This conditional expression can just be replaced by `{replacement}`.");
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, cond_end));
        } else if if_is_false && else_is_true {
            let replacement = format!("\nelse\n  !({cond_src})");
            let msg = format!("This conditional expression can just be replaced by `{replacement}`.");
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, cond_end));
        }
    }

    fn check_unless(&mut self, node: &ruby_prism::UnlessNode) {
        if node.end_keyword_loc().is_none() {
            return; // modifier form
        }

        let if_info = match node.statements().and_then(|s| single_body_offsets(&s)) {
            Some(i) => i,
            None => return,
        };
        let (_, _, if_is_true, if_is_false) = if_info;

        let else_info = match node.else_clause()
            .and_then(|e| e.statements())
            .and_then(|s| single_body_offsets(&s))
        {
            Some(i) => i,
            None => return,
        };
        let (_, _, else_is_true, else_is_false) = else_info;

        let cond = node.predicate();
        let cond_src = self.src(cond.location().start_offset(), cond.location().end_offset());

        let start = node.location().start_offset();
        let cond_end = cond.location().end_offset();

        // unless cond; true; else; false → !(cond)
        if if_is_true && else_is_false {
            let msg = format!("This conditional expression can just be replaced by `!({cond_src})`.");
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, cond_end));
        }
        // unless cond; false; else; true → cond
        else if if_is_false && else_is_true {
            let msg = format!("This conditional expression can just be replaced by `{cond_src}`.");
            self.offenses.push(self.ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, start, cond_end));
        }
    }
}

impl Visit<'_> for RedundantConditionalVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless(node);
        ruby_prism::visit_unless_node(self, node);
    }
}

crate::register_cop!("Style/RedundantConditional", |_cfg| {
    Some(Box::new(RedundantConditional::new()))
});
