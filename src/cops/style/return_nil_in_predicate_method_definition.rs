//! Style/ReturnNilInPredicateMethodDefinition — predicate methods returning nil should return false.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/return_nil_in_predicate_method_definition.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/ReturnNilInPredicateMethodDefinition";
const MSG: &str = "Return `false` instead of `nil` in predicate methods.";

#[derive(Default)]
pub struct ReturnNilInPredicateMethodDefinition {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl ReturnNilInPredicateMethodDefinition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { allowed_methods, allowed_patterns }
    }
}

impl Cop for ReturnNilInPredicateMethodDefinition {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if !name.ends_with('?') {
            return vec![];
        }
        if is_method_allowed(&self.allowed_methods, &self.allowed_patterns, &name, None) {
            return vec![];
        }
        let Some(body) = node.body() else { return vec![] };

        let mut offenses = Vec::new();

        // Walk all descendants for explicit `return` (no value or `return nil`).
        let mut visitor = ReturnVisitor { ctx, offenses: &mut offenses };
        visitor.visit(&body);

        // Implicit-return handling: walk the last expression of the body.
        handle_implicit_returns(&body, ctx, &mut offenses);

        offenses
    }
}

struct ReturnVisitor<'a, 'b> {
    ctx: &'a CheckContext<'a>,
    offenses: &'b mut Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for ReturnVisitor<'a, 'b> {
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {
        // Don't recurse into nested defs.
    }

    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        if is_return_nil(node) {
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            let off = self.ctx
                .offense_with_range(COP_NAME, MSG, Severity::Convention, start, end)
                .with_correction(Correction { edits: vec![Edit {
                    start_offset: start,
                    end_offset: end,
                    replacement: "return false".to_string(),
                }]});
            self.offenses.push(off);
        }
        ruby_prism::visit_return_node(self, node);
    }
}

fn is_return_nil(node: &ruby_prism::ReturnNode) -> bool {
    match node.arguments() {
        None => true,
        Some(args) => {
            let list: Vec<_> = args.arguments().iter().collect();
            if list.is_empty() {
                return true;
            }
            if list.len() != 1 {
                return false;
            }
            matches!(&list[0], Node::NilNode { .. })
        }
    }
}

/// Walk the last expression of `node`. If StatementsNode → its last child; otherwise the node.
/// Then if it's an If, recurse into both branches; if Nil, register an offense.
fn handle_implicit_returns(node: &Node, ctx: &CheckContext, offenses: &mut Vec<Offense>) {
    let last_opt = match node {
        Node::StatementsNode { .. } => {
            let s = node.as_statements_node().unwrap();
            s.body().iter().last()
        }
        _ => None,
    };

    // If body wasn't a StatementsNode, treat node itself as "last"; else use last child.
    let candidate_owned;
    let candidate: &Node = match last_opt {
        Some(n) => { candidate_owned = n; &candidate_owned }
        None => node,
    };

    match candidate {
        Node::IfNode { .. } => {
            let if_n = candidate.as_if_node().unwrap();
            if let Some(stmts) = if_n.statements() {
                let then_node = stmts.as_node();
                handle_implicit_returns(&then_node, ctx, offenses);
            }
            if let Some(sub) = if_n.subsequent() {
                handle_subsequent(&sub, ctx, offenses);
            }
        }
        Node::NilNode { .. } => {
            let start = candidate.location().start_offset();
            let end = candidate.location().end_offset();
            let off = ctx
                .offense_with_range(COP_NAME, MSG, Severity::Convention, start, end)
                .with_correction(Correction { edits: vec![Edit {
                    start_offset: start,
                    end_offset: end,
                    replacement: "false".to_string(),
                }]});
            offenses.push(off);
        }
        _ => {}
    }
}

fn handle_subsequent(sub: &Node, ctx: &CheckContext, offenses: &mut Vec<Offense>) {
    match sub {
        Node::ElseNode { .. } => {
            let en = sub.as_else_node().unwrap();
            if let Some(stmts) = en.statements() {
                let body_node = stmts.as_node();
                handle_implicit_returns(&body_node, ctx, offenses);
            }
        }
        Node::IfNode { .. } => {
            // elsif chain
            let if_n = sub.as_if_node().unwrap();
            if let Some(stmts) = if_n.statements() {
                let body_node = stmts.as_node();
                handle_implicit_returns(&body_node, ctx, offenses);
            }
            if let Some(s) = if_n.subsequent() {
                handle_subsequent(&s, ctx, offenses);
            }
        }
        _ => {}
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

crate::register_cop!("Style/ReturnNilInPredicateMethodDefinition", |cfg| {
    let c: Cfg = cfg.typed(COP_NAME);
    Some(Box::new(ReturnNilInPredicateMethodDefinition::with_config(
        c.allowed_methods,
        c.allowed_patterns,
    )))
});
