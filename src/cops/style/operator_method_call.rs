//! Style/OperatorMethodCall - Flag redundant dot before operator method.
//!
//! Ported from `lib/rubocop/cop/style/operator_method_call.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Redundant dot detected.";

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>",
    "+", "-", "*", "/", "%", "**", "~", "!", "!=", "!~",
];

const NONMUTATING_UNARY: &[&str] = &["~", "!", "+", "-"];

#[derive(Default)]
pub struct OperatorMethodCall;

impl OperatorMethodCall {
    pub fn new() -> Self { Self }
}

impl Cop for OperatorMethodCall {
    fn name(&self) -> &'static str { "Style/OperatorMethodCall" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if !OPERATOR_METHODS.contains(&method.as_ref()) {
            return vec![];
        }

        // Need a `.` operator (skip safe-nav `&.` — not a normal call shape here anyway)
        let dot_loc = match node.call_operator_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let dot_src = &ctx.source[dot_loc.start_offset()..dot_loc.end_offset()];
        if dot_src != "." {
            return vec![];
        }

        // Receiver must be present and not a constant.
        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if matches!(recv, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }) {
            return vec![];
        }

        // Selector source (the operator chars, e.g. `+`, `==`, `~@`).
        let sel_loc = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let selector_src = &ctx.source[sel_loc.start_offset()..sel_loc.end_offset()];

        // Unary methods like `~@`, `!@`, `+@`, `-@` are stored as method `~` `!` `+` `-`
        // but selector source has the trailing `@` (or even just `~` with backtick `\``).
        // Skip if not the operator form.
        if NONMUTATING_UNARY.contains(&method.as_ref()) && selector_src != method.as_ref() {
            return vec![];
        }

        // Need exactly one argument
        let args_node = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() != 1 {
            return vec![];
        }

        let rhs = &args[0];

        // Skip splat/kwsplat/forwarding/block_pass.
        // For Hash arg, look at first element instead (RuboCop's `argument.children.first`).
        if is_invalid_arg(rhs) {
            return vec![];
        }

        // Special case `foo.+(@bar).to_s` — when call is parenthesized AND chained
        // AND the argument is NOT a bare-word call (i.e., has a meaningful first child),
        // RuboCop skips it. Mirror RuboCop's `method_call_with_parenthesized_arg?`.
        let is_parenthesized = node.opening_loc().is_some() && node.closing_loc().is_some();
        let is_chained = is_chained_after(node, ctx);
        if is_chained && is_parenthesized && !is_bare_call_without_receiver(rhs) {
            return vec![];
        }

        // Build offense
        let dot_start = dot_loc.start_offset();
        let dot_end = dot_loc.end_offset();

        let mut edits: Vec<Edit> = Vec::new();

        if is_chained {
            // Replace dot with ' '
            edits.push(mk_edit(dot_start, dot_end, " "));
            // Insert space after selector if no space exists
            let sel_end = sel_loc.end_offset();
            let after_sel = ctx.source.as_bytes().get(sel_end).copied();
            if after_sel != Some(b' ') {
                edits.push(mk_edit(sel_end, sel_end, " "));
            }
            // Wrap in parens: insert `(` at node start, `)` at node end (after removing
            // existing `(` and `)` if argument is parenthesized).
            let node_loc = node.location();
            let node_start = node_loc.start_offset();
            let node_end = node_loc.end_offset();

            // Remove existing opening/closing parens around argument
            if let (Some(open), Some(close)) = (node.opening_loc(), node.closing_loc()) {
                edits.push(mk_edit(open.start_offset(), open.end_offset(), ""));
                edits.push(mk_edit(close.start_offset(), close.end_offset(), ""));
            }
            edits.push(mk_edit(node_start, node_start, "("));
            edits.push(mk_edit(node_end, node_end, ")"));
        } else {
            // Simple replace
            edits.push(mk_edit(dot_start, dot_end, " "));

            // Insert space after selector if needed
            let sel_end = sel_loc.end_offset();
            let rhs_loc = rhs.location();
            let rhs_start = rhs_loc.start_offset();

            if sel_end == rhs_start {
                // No space between selector and rhs (e.g., `foo.|bar`)
                edits.push(mk_edit(sel_end, sel_end, " "));
            } else if method.as_ref() == "/" {
                // For `/`, if RHS starts with `(` directly after selector with no
                // intervening space, insert space.
                let between = &ctx.source[sel_end..rhs_start];
                if between == "(" {
                    edits.push(mk_edit(sel_end, sel_end, " "));
                }
            }
        }

        let offense = ctx
            .offense_with_range(self.name(), MSG, self.severity(), dot_start, dot_end)
            .with_correction(Correction { edits });
        vec![offense]
    }
}

fn is_invalid_arg(arg: &Node) -> bool {
    match arg {
        Node::SplatNode { .. }
        | Node::BlockArgumentNode { .. }
        | Node::ForwardingArgumentsNode { .. } => true,
        Node::HashNode { .. } => {
            // Look at first element — if it's an AssocSplat, treat as kwsplat.
            let h = arg.as_hash_node().unwrap();
            if let Some(first) = h.elements().iter().next() {
                matches!(first, Node::AssocSplatNode { .. })
            } else {
                false
            }
        }
        Node::KeywordHashNode { .. } => {
            let h = arg.as_keyword_hash_node().unwrap();
            if let Some(first) = h.elements().iter().next() {
                matches!(first, Node::AssocSplatNode { .. })
            } else {
                false
            }
        }
        _ => false,
    }
}

/// True for `(send nil :name)` — bareword call, no receiver, no args.
/// Matches RuboCop's "argument.children.first is nil" check.
fn is_bare_call_without_receiver(arg: &Node) -> bool {
    if let Node::CallNode { .. } = arg {
        let c = arg.as_call_node().unwrap();
        if c.receiver().is_some() { return false; }
        if c.arguments().is_some() { return false; }
        if c.block().is_some() { return false; }
        return true;
    }
    false
}

/// Scan source to detect if the call is followed by `.IDENT` (chained).
fn is_chained_after(node: &ruby_prism::CallNode, ctx: &CheckContext) -> bool {
    let end = node.location().end_offset();
    let bytes = ctx.source.as_bytes();
    let mut i = end;
    // Skip whitespace
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    if i >= bytes.len() { return false; }
    if bytes[i] == b'.' {
        // Check next char is alpha/underscore (a method name, not another dot)
        if let Some(&c) = bytes.get(i + 1) {
            return c.is_ascii_alphabetic() || c == b'_';
        }
    }
    false
}

fn mk_edit(start: usize, end: usize, text: &str) -> Edit {
    Edit { start_offset: start, end_offset: end, replacement: text.to_string() }
}

crate::register_cop!("Style/OperatorMethodCall", |_cfg| Some(Box::new(OperatorMethodCall::new())));
