//! Lint/SafeNavigationConsistency - Check consistent safe navigation in && / || chains.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/safe_navigation_consistency.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Lint/SafeNavigationConsistency";
const USE_DOT_MSG: &str = "Use `.` instead of unnecessary `&.`.";
const USE_SAFE_NAV_MSG: &str = "Use `&.` for consistency with safe navigation.";

const NIL_METHODS: &[&str] = &[
    "!", "!=", "==", "===", "instance_of?", "kind_of?", "is_a?", "eql?", "equal?",
    "__id__", "object_id", "hash", "nil?", "respond_to?", "tap", "then", "yield_self",
    "inspect", "to_s", "frozen?",
];

pub struct SafeNavigationConsistency {
    allowed_methods: Vec<String>,
}

impl Default for SafeNavigationConsistency {
    fn default() -> Self {
        Self { allowed_methods: vec!["present?".into(), "blank?".into(), "try".into(), "presence".into()] }
    }
}

impl SafeNavigationConsistency {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(allowed_methods: Vec<String>) -> Self { Self { allowed_methods } }
}

impl Cop for SafeNavigationConsistency {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Finder { cop: self, ctx, offenses: Vec::new(), inside_logical: false };
        v.visit_program_node(node);
        // Dedupe by (start, end)
        let mut seen = std::collections::HashSet::new();
        v.offenses.retain(|o| seen.insert((o.location.line, o.location.column, o.location.last_column)));
        v.offenses
    }
}

struct Finder<'a, 'b> {
    cop: &'a SafeNavigationConsistency,
    ctx: &'a CheckContext<'b>,
    offenses: Vec<Offense>,
    inside_logical: bool,
}

impl<'a, 'b> Visit<'_> for Finder<'a, 'b> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        self.process_root(Logical::And(node));
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        self.process_root(Logical::Or(node));
        ruby_prism::visit_or_node(self, node);
    }
}

enum Logical<'a, 'pr> {
    And(&'a ruby_prism::AndNode<'pr>),
    Or(&'a ruby_prism::OrNode<'pr>),
}

#[derive(Debug, Clone)]
struct Operand {
    full_start: usize,
    full_end: usize,
    receiver_key: String,
    method_name: String,
    is_csend: bool,
    is_operator_method: bool,
    /// Range of dot token (. or &.)
    dot_start: usize,
    dot_end: usize,
    has_dot_text: bool, // "." present (not csend, not operator)
    in_and: bool,
    in_or: bool,
}

impl<'a, 'b> Finder<'a, 'b> {
    fn process_root(&mut self, root: Logical) {
        let mut ops: Vec<Operand> = Vec::new();
        match &root {
            Logical::And(n) => self.collect_from_logical_node(&n.left(), &n.right(), true, false, &mut ops),
            Logical::Or(n) => self.collect_from_logical_node(&n.left(), &n.right(), false, true, &mut ops),
        };

        // Group by receiver_key
        let mut groups: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
        for (i, op) in ops.iter().enumerate() {
            groups.entry(op.receiver_key.clone()).or_default().push(i);
        }

        for (_, idxs) in &groups {
            if idxs.len() < 2 { continue; }
            let grouped: Vec<&Operand> = idxs.iter().map(|&i| &ops[i]).collect();
            let Some((dot_op, begin_rest)) = find_consistent_parts(&grouped, self.cop) else { continue; };
            for op in grouped.iter().skip(begin_rest) {
                if already_appropriate_call(op, dot_op) { continue; }
                self.register(op, dot_op);
            }
        }
    }

    fn collect_from_logical_node(
        &self,
        lhs: &Node,
        rhs: &Node,
        in_and: bool,
        in_or: bool,
        out: &mut Vec<Operand>,
    ) {
        self.collect_operand(lhs, in_and, in_or, out);
        self.collect_operand(rhs, in_and, in_or, out);
    }

    fn collect_operand(&self, node: &Node, in_and: bool, in_or: bool, out: &mut Vec<Operand>) {
        // Recurse through nested and/or (but stop at parens/begin)
        match node {
            Node::AndNode { .. } => {
                let n = node.as_and_node().unwrap();
                self.collect_from_logical_node(&n.left(), &n.right(), true, false, out);
            }
            Node::OrNode { .. } => {
                let n = node.as_or_node().unwrap();
                self.collect_from_logical_node(&n.left(), &n.right(), false, true, out);
            }
            Node::CallNode { .. } | Node::IndexOperatorWriteNode { .. } | Node::CallOperatorWriteNode { .. } => {
                if let Some(op) = call_to_operand(node, self.ctx.source, in_and, in_or) {
                    out.push(op);
                }
            }
            _ => {}
        }
    }

    fn register(&mut self, op: &Operand, dot_op: &'static str) {
        let (start, end) = if op.is_operator_method {
            (op.full_start, op.full_end)
        } else {
            (op.dot_start, op.dot_end)
        };
        let msg = if dot_op == "." { USE_DOT_MSG } else { USE_SAFE_NAV_MSG };
        let mut offense = self.ctx.offense_with_range(COP_NAME, msg, Severity::Warning, start, end);
        if !op.is_operator_method {
            offense = offense.with_correction(Correction::replace(op.dot_start, op.dot_end, dot_op));
        }
        self.offenses.push(offense);
    }
}

fn call_to_operand(node: &Node, source: &str, in_and: bool, in_or: bool) -> Option<Operand> {
    let call = node.as_call_node()?;

    let method_name = node_name!(call).to_string();
    let recv = call.receiver()?;
    let recv_src = source[recv.location().start_offset()..recv.location().end_offset()].to_string();

    // operator vs csend/dot detection
    let call_op_loc = call.call_operator_loc();
    let (is_csend, dot_start, dot_end, has_dot_text, is_operator_method) = match call_op_loc {
        Some(loc) => {
            let text = &source[loc.start_offset()..loc.end_offset()];
            let is_csend = text == "&.";
            (is_csend, loc.start_offset(), loc.end_offset(), text == ".", false)
        }
        None => {
            // operator method call (e.g. `foo > 1`)
            (false, 0, 0, false, true)
        }
    };

    // receiver_key: per RuboCop receiver_name_as_key — if method.parent.call_type? use receiver of parent,
    // else use this call's receiver source. Without parent tracking we use recv_src.
    let receiver_key = recv_src.clone();

    Some(Operand {
        full_start: node.location().start_offset(),
        full_end: node.location().end_offset(),
        receiver_key,
        method_name,
        is_csend,
        is_operator_method,
        dot_start,
        dot_end,
        has_dot_text,
        in_and,
        in_or,
    })
}

fn find_consistent_parts<'o>(grouped: &[&'o Operand], cop: &SafeNavigationConsistency) -> Option<(&'static str, usize)> {
    let mut csend_in_and = None;
    let mut csend_in_or = None;
    let mut send_in_and = None;
    let mut send_in_or = None;
    for (i, op) in grouped.iter().enumerate() {
        if op.in_and && op.is_csend && csend_in_and.is_none() { csend_in_and = Some(i); }
        if op.in_or && op.is_csend && csend_in_or.is_none() { csend_in_or = Some(i); }
        if op.in_and && !nilable(op, cop) && send_in_and.is_none() { send_in_and = Some(i); }
        if op.in_or && !nilable(op, cop) && send_in_or.is_none() { send_in_or = Some(i); }
    }

    if let (Some(a), Some(b)) = (csend_in_and, csend_in_or) {
        if a < b { return None; }
    }

    if let Some(cand) = csend_in_and {
        let end = match send_in_and {
            Some(sa) => sa.min(cand) + 1,
            None => cand + 1,
        };
        return Some((".", end));
    }
    if let (Some(so), Some(co)) = (send_in_or, csend_in_or) {
        return Some(if so < co { (".", so + 1) } else { ("&.", co + 1) });
    }
    if let (Some(sa), Some(co)) = (send_in_and, csend_in_or) {
        if sa < co { return Some((".", co)); }
    }
    None
}

fn nilable(op: &Operand, cop: &SafeNavigationConsistency) -> bool {
    if op.is_csend { return true; }
    if NIL_METHODS.contains(&op.method_name.as_str()) { return true; }
    if cop.allowed_methods.iter().any(|m| m == &op.method_name) { return true; }
    false
}

fn already_appropriate_call(op: &Operand, dot_op: &str) -> bool {
    if op.is_csend && dot_op == "&." { return true; }
    if (op.has_dot_text || op.is_operator_method) && dot_op == "." { return true; }
    false
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_methods: Vec<String>,
}

impl Default for Cfg {
    fn default() -> Self {
        Self {
            allowed_methods: vec!["present?".into(), "blank?".into(), "try".into(), "presence".into()],
        }
    }
}

crate::register_cop!("Lint/SafeNavigationConsistency", |cfg| {
    let c: Cfg = cfg.typed("Lint/SafeNavigationConsistency");
    Some(Box::new(SafeNavigationConsistency::with_config(c.allowed_methods)))
});
