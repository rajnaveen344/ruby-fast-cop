//! Lint/UselessOr cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/useless_or.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, OrNode, Visit};

const TRUTHY_METHODS: &[&str] = &[
    "to_a", "to_c", "to_d", "to_i", "to_f", "to_h", "to_r",
    "to_s", "to_sym", "intern", "inspect", "hash", "object_id", "__id__",
];

#[derive(Default)]
pub struct UselessOr;

impl UselessOr {
    pub fn new() -> Self { Self }
}

impl Cop for UselessOr {
    fn name(&self) -> &'static str { "Lint/UselessOr" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = UselessOrVisitor { ctx, offenses: Vec::new(), in_chain: 0 };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct UselessOrVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_chain: usize,
}

/// True iff `node` is a non-safe-navigation method call to a truthy method without args.
fn is_truthy_method_call(node: &Node) -> bool {
    let call = match node.as_call_node() {
        Some(c) => c,
        None => return false,
    };
    if call.is_safe_navigation() { return false; }
    let method = String::from_utf8_lossy(call.name().as_slice()).to_string();
    if !TRUTHY_METHODS.contains(&method.as_str()) { return false; }
    if call.arguments().is_some() || call.block().is_some() { return false; }
    call.receiver().is_some()
}

/// Strip a single `(...)` paren wrapping if present.
fn strip_parens<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let p = node.as_parentheses_node()?;
    let body = p.body()?;
    let stmts = body.as_statements_node()?;
    let mut iter = stmts.body().iter();
    let first = iter.next()?;
    if iter.next().is_some() { return None; }
    Some(first)
}

/// Returns true if every reachable terminal value of `node` (across `||` and parens)
/// is a truthy method call. Only an `||` chain is considered: any other shape yields false
/// unless it itself is a truthy_method_call.
#[allow(dead_code)]
fn is_always_truthy(node: &Node) -> bool {
    if is_truthy_method_call(node) { return true; }
    if let Some(inner) = strip_parens(node) {
        return is_always_truthy(&inner);
    }
    if let Some(or) = node.as_or_node() {
        // a || b: always truthy iff b is always truthy
        return is_always_truthy(&or.right());
    }
    false
}

/// Find the deepest truthy-method call inside an "always-truthy" expression
/// (peeling parens and `||` rhs).
fn find_truthy_call<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    if is_truthy_method_call(node) {
        return Some(unsafe { std::ptr::read(node) });
    }
    if let Some(inner) = strip_parens(node) {
        return find_truthy_call(&inner);
    }
    if let Some(or) = node.as_or_node() {
        let r = or.right();
        return find_truthy_call(&r);
    }
    None
}

/// Flatten a left-leaning OR chain into operands [leftmost, ..., rightmost].
/// `((a||b)||c)` -> [a, b, c].  Stops at non-OR (parens kept as single operand).
fn flatten_or_chain<'a>(node: &OrNode<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    fn rec<'a>(n: Node<'a>, out: &mut Vec<Node<'a>>) {
        if let Some(or) = n.as_or_node() {
            rec(or.left(), out);
            out.push(or.right());
        } else {
            out.push(n);
        }
    }
    rec(node.left(), &mut out);
    out.push(node.right());
    out
}

impl<'a> Visit<'_> for UselessOrVisitor<'a> {
    fn visit_or_node(&mut self, node: &OrNode) {
        let is_top = self.in_chain == 0;
        self.in_chain += 1;

        if is_top {
            let operands = flatten_or_chain(node);
            // Find first index whose operand is "always truthy".
            let mut truthy_idx: Option<usize> = None;
            for (i, op) in operands.iter().enumerate() {
                if i + 1 == operands.len() { break; } // last operand can't make anything useless
                if is_always_truthy(op) {
                    truthy_idx = Some(i);
                    break;
                }
            }
            if let Some(i) = truthy_idx {
                let truthy_op = &operands[i];
                let useless_first = &operands[i + 1];
                // Find the OR operator location between operands[i] and operands[i+1].
                // Easiest: it's the byte range between truthy_op.end and useless_first.start
                // (covering whitespace + `||` or `or`).
                let truthy_end = truthy_op.location().end_offset();
                let useless_start = useless_first.location().start_offset();
                // Skip whitespace forward to find op
                let src = self.ctx.source;
                let bytes = src.as_bytes();
                let mut k = truthy_end;
                while k < useless_start && (bytes[k] == b' ' || bytes[k] == b'\t' || bytes[k] == b'\n') {
                    k += 1;
                }
                let op_start = k;

                let useless_end_full = operands.last().unwrap().location().end_offset();
                let useless_first_end = useless_first.location().end_offset();
                // For message: use the inner truthy method call (deepest reachable truthy call)
                let truthy_call_node = find_truthy_call(truthy_op).unwrap_or_else(|| {
                    // fallback (shouldn't happen): use the operand itself
                    unsafe { std::ptr::read(truthy_op) }
                });
                let truthy_src = self.ctx.src(truthy_call_node.location().start_offset(), truthy_call_node.location().end_offset()).to_string();
                let useless_src = self.ctx.src(useless_first.location().start_offset(), useless_first_end).to_string();
                let msg = format!(
                    "`{}` will never evaluate because `{}` always returns a truthy value.",
                    useless_src, truthy_src
                );

                let or_start = node.location().start_offset();
                let or_end = node.location().end_offset();
                // Replacement = source from or_start .. truthy_op.end
                let replacement = self.ctx.src(or_start, truthy_end).to_string();

                let off = self.ctx.offense_with_range(
                    "Lint/UselessOr",
                    &msg,
                    Severity::Warning,
                    op_start,
                    useless_first_end,
                ).with_correction(Correction::replace(or_start, or_end, replacement));
                self.offenses.push(off);
            }
        }

        ruby_prism::visit_or_node(self, node);
        self.in_chain -= 1;
    }
}

crate::register_cop!("Lint/UselessOr", |_cfg| {
    Some(Box::new(UselessOr::new()))
});
