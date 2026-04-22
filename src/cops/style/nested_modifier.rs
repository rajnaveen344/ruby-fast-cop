//! Style/NestedModifier cop
//!
//! Flags nested modifier statements like `something if a if b`.
//! Offense: the inner modifier keyword.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;
use ruby_prism::Node;

const MSG: &str = "Avoid using nested modifiers.";

#[derive(Default)]
pub struct NestedModifier;

impl NestedModifier {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for NestedModifier {
    fn name(&self) -> &'static str {
        "Style/NestedModifier"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = NestedModifierVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            consumed_starts: std::collections::HashSet::new(),
        };
        ruby_prism::visit_program_node(&mut visitor, &result.node().as_program_node().unwrap());
        visitor.offenses
    }
}

/// Info about a modifier node.
struct ModifierInfo {
    keyword: &'static str,
    cond_start: usize,
    cond_end: usize,
    keyword_start: usize,
    keyword_end: usize,
    node_start: usize,
    node_end: usize,
    /// The body node start/end (the statement being guarded)
    body_start: usize,
    body_end: usize,
    is_while_until: bool,
}

fn extract_if_info(node: &ruby_prism::IfNode, source: &str) -> Option<ModifierInfo> {
    let kw_loc = node.if_keyword_loc()?;
    let kw_src = &source[kw_loc.start_offset()..kw_loc.end_offset()];
    if kw_src != "if" {
        return None;
    }
    let stmts = node.statements()?;
    let parts: Vec<_> = stmts.body().iter().collect();
    if parts.len() != 1 {
        return None;
    }
    let stmt_loc = parts[0].location();
    let pred = node.predicate();
    Some(ModifierInfo {
        keyword: "if",
        cond_start: pred.location().start_offset(),
        cond_end: pred.location().end_offset(),
        keyword_start: kw_loc.start_offset(),
        keyword_end: kw_loc.end_offset(),
        node_start: node.location().start_offset(),
        node_end: node.location().end_offset(),
        body_start: stmt_loc.start_offset(),
        body_end: stmt_loc.end_offset(),
        is_while_until: false,
    })
}

fn extract_unless_info(node: &ruby_prism::UnlessNode, source: &str) -> Option<ModifierInfo> {
    let kw_loc = node.keyword_loc();
    let stmts = node.statements()?;
    let parts: Vec<_> = stmts.body().iter().collect();
    if parts.len() != 1 {
        return None;
    }
    let stmt_loc = parts[0].location();
    let pred = node.predicate();
    Some(ModifierInfo {
        keyword: "unless",
        cond_start: pred.location().start_offset(),
        cond_end: pred.location().end_offset(),
        keyword_start: kw_loc.start_offset(),
        keyword_end: kw_loc.end_offset(),
        node_start: node.location().start_offset(),
        node_end: node.location().end_offset(),
        body_start: stmt_loc.start_offset(),
        body_end: stmt_loc.end_offset(),
        is_while_until: false,
    })
}

fn extract_while_info(node: &ruby_prism::WhileNode, _source: &str) -> Option<ModifierInfo> {
    let kw_loc = node.keyword_loc();
    let stmts = node.statements()?;
    let parts: Vec<_> = stmts.body().iter().collect();
    if parts.len() != 1 {
        return None;
    }
    let pred = node.predicate();
    let stmt_loc = parts[0].location();
    Some(ModifierInfo {
        keyword: "while",
        cond_start: pred.location().start_offset(),
        cond_end: pred.location().end_offset(),
        keyword_start: kw_loc.start_offset(),
        keyword_end: kw_loc.end_offset(),
        node_start: node.location().start_offset(),
        node_end: node.location().end_offset(),
        body_start: stmt_loc.start_offset(),
        body_end: stmt_loc.end_offset(),
        is_while_until: true,
    })
}

fn extract_until_info(node: &ruby_prism::UntilNode, _source: &str) -> Option<ModifierInfo> {
    let kw_loc = node.keyword_loc();
    let stmts = node.statements()?;
    let parts: Vec<_> = stmts.body().iter().collect();
    if parts.len() != 1 {
        return None;
    }
    let pred = node.predicate();
    let stmt_loc = parts[0].location();
    Some(ModifierInfo {
        keyword: "until",
        cond_start: pred.location().start_offset(),
        cond_end: pred.location().end_offset(),
        keyword_start: kw_loc.start_offset(),
        keyword_end: kw_loc.end_offset(),
        node_start: node.location().start_offset(),
        node_end: node.location().end_offset(),
        body_start: stmt_loc.start_offset(),
        body_end: stmt_loc.end_offset(),
        is_while_until: true,
    })
}

fn inner_modifier_info(body_start: usize, body_end: usize, source: &str) -> Option<ModifierInfo> {
    // Parse the body range to find the inner modifier
    let body_src = &source[body_start..body_end];
    let result = ruby_prism::parse(body_src.as_bytes());
    let root = result.node();
    let prog = root.as_program_node()?;
    let stmts = prog.statements();
    let parts: Vec<_> = stmts.body().iter().collect();
    if parts.len() != 1 {
        return None;
    }
    let inner = &parts[0];
    let offset = body_start; // adjust offsets
    let mut info = match inner {
        Node::IfNode { .. } => {
            let n = inner.as_if_node().unwrap();
            extract_if_info(&n, body_src)?
        }
        Node::UnlessNode { .. } => {
            let n = inner.as_unless_node().unwrap();
            extract_unless_info(&n, body_src)?
        }
        Node::WhileNode { .. } => {
            let n = inner.as_while_node().unwrap();
            extract_while_info(&n, body_src)?
        }
        Node::UntilNode { .. } => {
            let n = inner.as_until_node().unwrap();
            extract_until_info(&n, body_src)?
        }
        _ => return None,
    };
    info.cond_start += offset;
    info.cond_end += offset;
    info.keyword_start += offset;
    info.keyword_end += offset;
    info.node_start += offset;
    info.node_end += offset;
    info.body_start += offset;
    info.body_end += offset;
    Some(info)
}

fn maybe_parens_for_and(s: &str) -> String {
    // When combining with &&, only `||` expressions need parens (|| has lower precedence than &&)
    if s.contains(" || ") {
        format!("({})", s)
    } else {
        s.to_string()
    }
}

fn maybe_parens_for_or(s: &str) -> String {
    // When combining with ||, nothing needs parens (|| is lowest)
    s.to_string()
}

fn maybe_parens(s: &str) -> String {
    // Default: add parens if contains || (for && context)
    maybe_parens_for_and(s)
}

fn negate_cond(s: &str) -> String {
    // Check if it's a method call without parens: `receiver.method? arg` or `method? arg`
    // If so, add parens around the argument: `!receiver.method?(arg)`
    if let Some(negated) = try_negate_as_method_call(s) {
        return negated;
    }
    if s.contains(' ') {
        format!("!({})", s)
    } else {
        format!("!{}", s)
    }
}

fn try_negate_as_method_call(s: &str) -> Option<String> {
    // Match pattern: `expr.method? arg` or `expr.method? arg1, arg2`
    // where method name ends with `?` and arg is separated by space (no parens)
    // Find last occurrence of `? ` in s
    let q_pos = s.rfind("? ")?;
    let method_end = q_pos + 1; // position after `?`
    let arg_str = s[method_end..].trim_start();
    // The call part is everything before method_end + 1 (including the `?`)
    let call_part = &s[..method_end];
    // arg must not be empty and must be a simple expression
    if arg_str.is_empty() {
        return None;
    }
    // Ensure call_part looks like a method call (contains `.method?`)
    if !call_part.contains('.') && !call_part.ends_with('?') {
        return None;
    }
    Some(format!("!{}({})", call_part, arg_str))
}

struct NestedModifierVisitor<'a> {
    cop: &'a NestedModifier,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Byte offsets of nodes that are "inner" modifiers (already consumed as body of outer)
    consumed_starts: std::collections::HashSet<usize>,
}

impl NestedModifierVisitor<'_> {
    fn process(&mut self, outer: ModifierInfo) {
        // Skip if this node was already consumed as inner of another outer
        if self.consumed_starts.contains(&outer.node_start) {
            return;
        }

        let inner = match inner_modifier_info(outer.body_start, outer.body_end, self.ctx.source) {
            Some(i) => i,
            None => return,
        };

        // Mark inner as consumed so it won't be processed again as outer.
        // Also recursively mark all deeply-nested inner nodes.
        self.mark_all_inner_consumed(&inner);

        // Offense at the inner keyword
        let correctable = !outer.is_while_until && !inner.is_while_until;

        if correctable {
            let correction = self.make_correction(&outer, &inner);
            self.offenses.push(
                self.ctx.offense_with_range(self.cop.name(), MSG, self.cop.severity(),
                    inner.keyword_start, inner.keyword_end)
                    .with_correction(correction)
            );
        } else {
            self.offenses.push(
                self.ctx.offense_with_range(self.cop.name(), MSG, self.cop.severity(),
                    inner.keyword_start, inner.keyword_end)
            );
        }
    }

    fn mark_all_inner_consumed(&mut self, info: &ModifierInfo) {
        self.consumed_starts.insert(info.node_start);
        // Try to find deeper inner nodes and mark them too
        if let Some(deeper) = inner_modifier_info(info.body_start, info.body_end, self.ctx.source) {
            self.mark_all_inner_consumed(&deeper);
        }
    }

    fn make_correction(&self, outer: &ModifierInfo, inner: &ModifierInfo) -> Correction {
        let source = self.ctx.source;
        let outer_cond = &source[outer.cond_start..outer.cond_end];
        let inner_cond = &source[inner.cond_start..inner.cond_end];
        let stmt = &source[inner.body_start..inner.body_end];

        let (new_kw, new_cond) = match (outer.keyword, inner.keyword) {
            ("if", "if") => {
                // `stmt if inner if outer` → `stmt if outer && inner`
                let o = maybe_parens_for_and(outer_cond);
                let i = maybe_parens_for_and(inner_cond);
                ("if", format!("{} && {}", o, i))
            }
            ("unless", "unless") => {
                // `stmt unless inner unless outer` → `stmt unless outer || inner`
                let o = maybe_parens_for_or(outer_cond);
                let i = maybe_parens_for_or(inner_cond);
                ("unless", format!("{} || {}", o, i))
            }
            ("if", "unless") => {
                // `stmt unless inner_cond if outer_cond` → `stmt if outer && !inner`
                let o = maybe_parens_for_and(outer_cond);
                let i = negate_cond(inner_cond);
                ("if", format!("{} && {}", o, i))
            }
            ("unless", "if") => {
                // `stmt if inner_cond unless outer_cond` → `stmt unless outer || !inner`
                let o = maybe_parens_for_or(outer_cond);
                let i = negate_cond(inner_cond);
                ("unless", format!("{} || {}", o, i))
            }
            _ => return Correction::replace(0, 0, String::new()),
        };

        let new_src = format!("{} {} {}", stmt, new_kw, new_cond);
        Correction::replace(outer.node_start, outer.node_end, new_src)
    }
}

impl Visit<'_> for NestedModifierVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(info) = extract_if_info(node, self.ctx.source) {
            self.process(info);
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if let Some(info) = extract_unless_info(node, self.ctx.source) {
            self.process(info);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if let Some(info) = extract_while_info(node, self.ctx.source) {
            self.process(info);
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if let Some(info) = extract_until_info(node, self.ctx.source) {
            self.process(info);
        }
        ruby_prism::visit_until_node(self, node);
    }
}

crate::register_cop!("Style/NestedModifier", |_cfg| {
    Some(Box::new(NestedModifier::new()))
});
