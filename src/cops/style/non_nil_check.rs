//! Style/NonNilCheck cop
//!
//! Checks for non-nil checks, which are usually redundant.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG_REPLACEMENT: &str = "Prefer `%pref%` over `%cur%`.";
const MSG_REDUNDANCY: &str = "Explicit non-nil checks are usually redundant.";

pub struct NonNilCheck {
    include_semantic_changes: bool,
    /// When Style/NilComparison enforces "comparison" style, `x != nil` is preferred — don't flag it
    nil_comparison_comparison_style: bool,
}

impl NonNilCheck {
    pub fn new(include_semantic_changes: bool, nil_comparison_comparison_style: bool) -> Self {
        Self { include_semantic_changes, nil_comparison_comparison_style }
    }
}

impl Default for NonNilCheck {
    fn default() -> Self {
        Self::new(false, false)
    }
}

impl Cop for NonNilCheck {
    fn name(&self) -> &'static str {
        "Style/NonNilCheck"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = NonNilCheckVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            // Track ignored nodes (last expression in predicate methods)
            ignored_ranges: Vec::new(),
        };
        // First pass: collect ignored nodes from predicate method bodies
        visitor.collect_ignored(node);
        // Second pass: find offenses
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct NonNilCheckVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a NonNilCheck,
    offenses: Vec<Offense>,
    /// byte ranges of nodes to ignore (last expression in predicate method bodies)
    ignored_ranges: Vec<(usize, usize)>,
}

impl<'a> NonNilCheckVisitor<'a> {
    fn is_ignored(&self, start: usize, end: usize) -> bool {
        self.ignored_ranges.iter().any(|&(s, e)| s == start && e == end)
    }

    fn collect_ignored(&mut self, program: &ruby_prism::ProgramNode) {
        // Walk all def nodes, find predicate methods, ignore their last expression
        let mut collector = IgnoredCollector { ranges: Vec::new() };
        collector.visit_program_node(program);
        self.ignored_ranges = collector.ranges;
    }

    /// Check if `x != nil`
    fn check_not_equal_to_nil(&mut self, node: &ruby_prism::CallNode) {
        let method = node.name();
        if method.as_slice() != b"!=" {
            return;
        }
        // Receiver must exist, argument must be nil
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

        // When Style/NilComparison uses "comparison" style and we're not doing semantic changes,
        // `!= nil` is the *preferred* form — don't flag it.
        if self.cop.nil_comparison_comparison_style && !self.cop.include_semantic_changes {
            return;
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if self.is_ignored(start, end) {
            return;
        }

        let msg = if self.cop.include_semantic_changes {
            MSG_REDUNDANCY.to_string()
        } else {
            // Build "Prefer `!x.nil?` over `x != nil`."
            let recv_src = match node.receiver() {
                Some(r) => self.ctx.source[r.location().start_offset()..r.location().end_offset()].to_string(),
                None => return,
            };
            let cur = &self.ctx.source[start..end];
            format!("Prefer `!{}.nil?` over `{}`.", recv_src, cur)
        };

        self.offenses.push(self.ctx.offense_with_range(
            "Style/NonNilCheck",
            &msg,
            Severity::Convention,
            start,
            end,
        ));
    }

    /// Check if `!x.nil?` (include_semantic_changes must be true)
    fn check_not_nil_check(&mut self, node: &ruby_prism::CallNode) {
        if !self.cop.include_semantic_changes {
            return;
        }
        let method = node.name();
        if method.as_slice() != b"!" {
            return;
        }
        // Receiver must be `x.nil?` (a CallNode with method `nil?`)
        let recv = match node.receiver() {
            Some(r) => r,
            None => return,
        };
        let recv_call = match recv.as_call_node() {
            Some(c) => c,
            None => return,
        };
        if recv_call.name().as_slice() != b"nil?" {
            return;
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if self.is_ignored(start, end) {
            return;
        }

        self.offenses.push(self.ctx.offense_with_range(
            "Style/NonNilCheck",
            MSG_REDUNDANCY,
            Severity::Convention,
            start,
            end,
        ));
    }

    /// Check `unless x.nil?` (UnlessNode) → report on the `nil?` call node
    fn check_unless_nil_node(&mut self, cond: &ruby_prism::Node) {
        if !self.cop.include_semantic_changes {
            return;
        }
        let cond_call = match cond.as_call_node() {
            Some(c) => c,
            None => return,
        };
        if cond_call.name().as_slice() != b"nil?" {
            return;
        }

        let start = cond.location().start_offset();
        let end = cond.location().end_offset();

        if self.is_ignored(start, end) {
            return;
        }

        self.offenses.push(self.ctx.offense_with_range(
            "Style/NonNilCheck",
            MSG_REDUNDANCY,
            Severity::Convention,
            start,
            end,
        ));
    }
}

struct IgnoredCollector {
    ranges: Vec<(usize, usize)>,
}

impl IgnoredCollector {
    fn collect_predicate_method_ignored(&mut self, body: Option<ruby_prism::Node>) {
        let body = match body {
            Some(b) => b,
            None => return,
        };
        // If body is BeginNode or StatementsNode, get last child
        let last = if let Some(stmts) = body.as_statements_node() {
            let children: Vec<_> = stmts.body().iter().collect();
            children.into_iter().last()
        } else {
            Some(body)
        };
        if let Some(last_node) = last {
            self.ranges.push((last_node.location().start_offset(), last_node.location().end_offset()));
        }
    }

    fn method_name_ends_with_question(node: &ruby_prism::DefNode) -> bool {
        let name_bytes = node.name().as_slice();
        name_bytes.last() == Some(&b'?')
    }
}

impl Visit<'_> for IgnoredCollector {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if Self::method_name_ends_with_question(node) {
            if let Some(body) = node.body() {
                self.collect_predicate_method_ignored(Some(body));
            }
        }
        ruby_prism::visit_def_node(self, node);
    }
}

impl<'a> Visit<'_> for NonNilCheckVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = node.name();
        let method_bytes = method.as_slice();

        if method_bytes == b"!=" {
            self.check_not_equal_to_nil(node);
        } else if method_bytes == b"!" {
            self.check_not_nil_check(node);
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless_nil_node(&node.predicate());
        ruby_prism::visit_unless_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    include_semantic_changes: bool,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct NilComparisonCfg {
    enforced_style: String,
}

crate::register_cop!("Style/NonNilCheck", |cfg| {
    let c: Cfg = cfg.typed("Style/NonNilCheck");
    let nil_cmp: NilComparisonCfg = cfg.typed("Style/NilComparison");
    let nil_comparison_comparison_style = nil_cmp.enforced_style == "comparison";
    Some(Box::new(NonNilCheck::new(c.include_semantic_changes, nil_comparison_comparison_style)))
});
