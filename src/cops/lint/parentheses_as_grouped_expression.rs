//! Lint/ParenthesesAsGroupedExpression cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct ParenthesesAsGroupedExpression;

impl Default for ParenthesesAsGroupedExpression {
    fn default() -> Self {
        Self
    }
}

impl ParenthesesAsGroupedExpression {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ParenthesesAsGroupedExpression {
    fn name(&self) -> &'static str {
        "Lint/ParenthesesAsGroupedExpression"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = PGEVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct PGEVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> PGEVisitor<'a> {
    /// Check if first argument is parenthesized (i.e., parser saw `func (x)` where `(x)` is
    /// the grouped expression, not `func(x)` which has an opening_loc).
    /// The space-before-paren can be detected by checking if the source byte before the
    /// argument's start is a space AND the method's `opening_loc` is None.
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        // Must have exactly one argument and no opening paren on the call itself
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return;
        }

        // Multiple args means no ambiguity
        // opening_loc present = explicit parens `func(x)`
        if node.opening_loc().is_some() {
            return;
        }

        // Must not be operator/setter method
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if is_operator_method(&method) || is_setter_method(&method) {
            return;
        }

        let first_arg = &arg_list[0];

        // The argument must be parenthesized (a ParenthesesNode or something that starts with `(`)
        // In Prism, `func (x)` parses `(x)` as a ParenthesesNode.
        if first_arg.as_parentheses_node().is_none() {
            return;
        }

        // Must have a space before the `(` of the argument
        let arg_start = first_arg.location().start_offset();
        let src = self.ctx.source;
        let bytes = src.as_bytes();

        // Check there's a space before the paren (between method name end and arg start)
        // The method's message_loc gives us the method name position
        let method_end = match node.message_loc() {
            Some(loc) => loc.end_offset(),
            None => return,
        };

        // Check there's whitespace between method end and arg start
        if arg_start <= method_end {
            return;
        }
        let between = &src[method_end..arg_start];
        if !between.chars().all(|c| c == ' ' || c == '\t') || between.is_empty() {
            return;
        }

        // If it's a block-pass argument, skip (but block pass args are separate)
        // first_arg is ParenthesesNode — check if it contains only a block (a.concat ((1..1).map {...}))
        // If the parentheses contain a block, skip: `a.concat ((1..1).map { ... })`
        // RuboCop: return true if first_argument.any_block_type?
        // In Prism: parentheses_node.body is a statements containing a call with a block
        // This is case where the inner expr is a call ending in a block — not parenthesized_call
        // The fixture shows this IS flagged: `a.concat ((1..1).map { |i| i * 10 })` IS an offense
        // because the outer parens ARE grouped expression parens.
        // RuboCop: `return true if node.first_argument.any_block_type?` — any_block_type means
        // the first_arg itself is a block node. A ParenthesesNode is not a block_type, so we don't return.
        // So if ParenthesesNode wraps a call-with-block, it IS flagged.

        // valid_first_argument check:
        // operator_keyword? (and/or/not/||/&&) => skip
        // hash_type? => skip
        // ternary? => skip
        // compound_range? (range with parenthesized_call) => skip
        // For compound_range: first_arg.range_type? && first_arg.parenthesized_call?
        // first_arg is ParenthesesNode here, not a range_type, so compound_range doesn't apply.
        // BUT the body inside might be a range — we need to check the inner node.
        // Check inner node for valid_first_argument conditions
        if paren_body_matches(first_arg, is_operator_keyword) {
            return;
        }
        if paren_body_matches(first_arg, |n| n.as_hash_node().is_some()) {
            return;
        }
        if paren_body_matches(first_arg, is_ternary) {
            return;
        }

        // compound_range: range_type AND the range itself is a ParenthesesNode (parenthesized_call)
        // i.e. the inner body is a range like `(a - b)..(c - d)` — this is a range where at
        // least one end is parenthesized. But actually the RuboCop check is:
        // compound_range?(first_arg) = first_arg.range_type? && first_arg.parenthesized_call?
        // Since first_arg is ParenthesesNode (not range), this is always false here.
        // But the test "parenthesis_for_compound_range_literals" shows `rand (a - b)..(c - d)` is ok.
        // That means the argument is NOT a ParenthesesNode but a RangeNode — so opening_loc check works.
        // If `rand (1..10)` — ParenthesesNode wrapping a RangeNode — IS an offense.
        // If `rand (a - b)..(c - d)` — the first arg is a RangeNode (no parens wrapping the whole range)
        //   — our check fails at first_arg.as_parentheses_node().is_none() → skip. Correct!

        // chained_calls check:
        // chained_calls?(node) = first_argument.call_type? && (node.children.last...) > 1
        // Since first_arg is a ParenthesesNode, inner might be a call.
        // In RuboCop's original: chained_calls? checks if first_argument is call_type AND
        // has multiple children chained after. For us: `func (x).func.func...` — the first
        // arg is NOT a ParenthesesNode in Prism for that case; the chain wraps the whole thing.
        // Actually for `func (x).func`, Prism would parse as: func (called as "func") with arg
        // being a method call `.func` called on `(x)`. So first_arg would be a CallNode, not
        // a ParenthesesNode. So our ParenthesesNode check already handles this.
        //
        // Let's verify: `do_something.eq (foo * bar).to_i` — the first_arg is a CallNode
        // (`.to_i` called on `(foo * bar)`). Not a ParenthesesNode — correctly skipped.

        // Check for `assert_equal (0..1.9), acceleration.domain` — two args, skipped above ✓

        // Build offense on the space range (from method_end to arg_start)
        let space_start = method_end;
        let space_end = arg_start; // this is the `(` char
        let space_len = space_end - space_start;
        if space_len == 0 {
            return;
        }

        // Message uses the full source of the first_arg
        let arg_src = src.get(arg_start..first_arg.location().end_offset()).unwrap_or("");
        let msg = format!("`{}` interpreted as grouped expression.", arg_src);

        // Correction: remove the space
        let correction = Correction::delete(space_start, space_end);

        // Offense range: the space (from method_end to `(`)
        let offense = self.ctx.offense_with_range(
            "Lint/ParenthesesAsGroupedExpression",
            &msg,
            Severity::Warning,
            space_start,
            space_end,
        );
        self.offenses.push(offense.with_correction(correction));
    }
}

/// Check if the single body inside a ParenthesesNode satisfies a predicate.
fn paren_body_matches<'pr>(node: &Node<'pr>, check: impl Fn(&Node<'pr>) -> bool) -> bool {
    let paren = match node.as_parentheses_node() {
        Some(p) => p,
        None => return false,
    };
    let body = match paren.body() {
        Some(b) => b,
        None => return false,
    };
    let stmts = match body.as_statements_node() {
        Some(s) => s,
        None => return false,
    };
    let list: Vec<Node> = stmts.body().iter().collect();
    if list.len() == 1 {
        check(&list[0])
    } else {
        false
    }
}

fn is_operator_method(name: &str) -> bool {
    matches!(
        name,
        "+" | "-" | "*" | "/" | "%" | "**" | "==" | "!=" | "<" | ">" | "<=" | ">="
        | "<=>" | "<<" | ">>" | "&" | "|" | "^" | "[]" | "[]=" | "=~" | "!~" | "!"
    )
}

fn is_setter_method(name: &str) -> bool {
    name.ends_with('=') && !matches!(name, "==" | "!=" | "<=" | ">=")
}

fn is_operator_keyword(node: &Node) -> bool {
    node.as_and_node().is_some() || node.as_or_node().is_some()
}

fn is_ternary(node: &Node) -> bool {
    if let Some(if_node) = node.as_if_node() {
        // Ternary: `cond ? then : else` — then_keyword is `?`
        return if_node.then_keyword_loc().map_or(false, |loc| {
            loc.as_slice() == b"?"
        });
    }
    false
}

impl<'a> Visit<'_> for PGEVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/ParenthesesAsGroupedExpression", |_cfg| {
    Some(Box::new(ParenthesesAsGroupedExpression::new()))
});
