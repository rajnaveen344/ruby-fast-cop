//! Style/NilComparison cop
//!
//! Checks for `x == nil` (prefer `x.nil?`) or `x.nil?` (prefer `x == nil`).

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Visit};

pub struct NilComparison {
    prefer_predicate: bool, // true = predicate style (default), false = comparison style
}

impl NilComparison {
    pub fn new(prefer_predicate: bool) -> Self {
        Self { prefer_predicate }
    }
}

impl Default for NilComparison {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Cop for NilComparison {
    fn name(&self) -> &'static str {
        "Style/NilComparison"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = NilComparisonVisitor {
            ctx,
            prefer_predicate: self.prefer_predicate,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct NilComparisonVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    prefer_predicate: bool,
    offenses: Vec<Offense>,
}

impl<'a> NilComparisonVisitor<'a> {
    fn check_call(&mut self, node: &CallNode) {
        let method = node_name!(node);

        if self.prefer_predicate {
            // Flag `x == nil` or `x === nil`
            match method.as_ref() {
                "==" | "===" => {}
                _ => return,
            }
            // Must have receiver
            if node.receiver().is_none() {
                return;
            }
            // Single argument must be nil
            let args = match node.arguments() {
                Some(a) => a,
                None => return,
            };
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() != 1 {
                return;
            }
            if arg_list[0].as_nil_node().is_none() {
                return;
            }
            let msg = "Prefer the use of the `nil?` predicate.";
            let sel_loc = node.message_loc().unwrap_or_else(|| node.location());
            // Correction: replace entire call node (recv == nil / recv === nil / recv.==(nil)) with recv.nil?
            let recv = node.receiver().unwrap();
            let recv_src = self.ctx.src(recv.location().start_offset(), recv.location().end_offset()).to_string();
            let node_start = node.location().start_offset();
            let node_end = node.location().end_offset();
            let correction = Correction::replace(node_start, node_end, format!("{}.nil?", recv_src));
            self.offenses.push(self.ctx.offense_with_range(
                "Style/NilComparison",
                msg,
                Severity::Convention,
                sel_loc.start_offset(),
                sel_loc.end_offset(),
            ).with_correction(correction));
        } else {
            // Flag `x.nil?`
            if method.as_ref() != "nil?" {
                return;
            }
            // Must have receiver
            if node.receiver().is_none() {
                return;
            }
            // No args
            if node.arguments().is_some() {
                return;
            }
            let msg = "Prefer the use of the `==` comparison.";
            let sel_loc = node.message_loc().unwrap_or_else(|| node.location());
            // Correction: replace entire call node (recv.nil?) with (recv == nil) or recv == nil
            // RuboCop wraps in parens when parent is a negation — detect by checking byte before node start
            let recv = node.receiver().unwrap();
            let recv_src = self.ctx.src(recv.location().start_offset(), recv.location().end_offset()).to_string();
            let node_start = node.location().start_offset();
            let node_end = node.location().end_offset();
            let bytes = self.ctx.source.as_bytes();
            let preceded_by_bang = node_start > 0 && bytes[node_start - 1] == b'!';
            let replacement = if preceded_by_bang {
                format!("({} == nil)", recv_src)
            } else {
                format!("{} == nil", recv_src)
            };
            let correction = Correction::replace(node_start, node_end, replacement);
            self.offenses.push(self.ctx.offense_with_range(
                "Style/NilComparison",
                msg,
                Severity::Convention,
                sel_loc.start_offset(),
                sel_loc.end_offset(),
            ).with_correction(correction));
        }
    }
}

impl Visit<'_> for NilComparisonVisitor<'_> {
    fn visit_call_node(&mut self, node: &CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

crate::register_cop!("Style/NilComparison", |cfg| {
    let c: Cfg = cfg.typed("Style/NilComparison");
    let prefer_predicate = c.enforced_style != "comparison";
    Some(Box::new(NilComparison::new(prefer_predicate)))
});
