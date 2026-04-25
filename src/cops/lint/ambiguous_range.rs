//! Lint/AmbiguousRange - flag complex range boundaries lacking parens.
//!
//! Ports `RuboCop::Cop::Lint::AmbiguousRange`. Each boundary of a `..`/`...`
//! must be a "simple" expression (literal, variable, constant, self, paren-
//! wrapped). Otherwise wrap with `( )`.
//!
//! Config:
//!   - `RequireParenthesesForMethodChains` (Bool): when true, even chained
//!     method calls (`a.foo..b.bar`) require parens.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct AmbiguousRange { require_parens_for_chains: bool }

impl AmbiguousRange {
    pub fn new(require_parens_for_chains: bool) -> Self {
        Self { require_parens_for_chains }
    }
}

const MSG: &str = "Wrap complex range boundaries with parentheses to avoid ambiguity.";

impl Cop for AmbiguousRange {
    fn name(&self) -> &'static str { "Lint/AmbiguousRange" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = V {
            ctx,
            require_parens_for_chains: self.require_parens_for_chains,
            out: vec![],
        };
        v.visit(&result.node());
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    require_parens_for_chains: bool,
    out: Vec<Offense>,
}

impl<'a, 'b> V<'a, 'b> {
    fn check_boundary(&mut self, node: &Node) {
        if self.acceptable(node) { return; }
        let l = node.location();
        self.out.push(
            self.ctx.offense_with_range(
                "Lint/AmbiguousRange",
                MSG,
                Severity::Warning,
                l.start_offset(), l.end_offset(),
            ).with_correction({
                let mut c = Correction::insert(l.start_offset(), "(");
                c.edits.push(crate::offense::Edit {
                    start_offset: l.end_offset(),
                    end_offset: l.end_offset(),
                    replacement: ")".to_string(),
                });
                c
            }),
        );
    }

    fn acceptable(&self, node: &Node) -> bool {
        // Parenthesized => begin_type? in RuboCop -> ParenthesesNode
        if matches!(node, Node::ParenthesesNode { .. }) { return true; }
        if is_literal(node) { return true; }
        if is_rational_literal(node) { return true; }
        if is_variable(node) { return true; }
        if matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }) { return true; }
        if matches!(node, Node::SelfNode { .. }) { return true; }

        if let Node::CallNode { .. } = node {
            return self.acceptable_call(node);
        }
        false
    }

    fn acceptable_call(&self, node: &Node) -> bool {
        let call = node.as_call_node().unwrap();
        let method_name = String::from_utf8_lossy(call.name().as_slice()).into_owned();

        // Element reference `x[1]`
        if method_name == "[]" { return true; }

        // Unary +/- /! / ~ on a value: Prism represents `+a` as a CallNode
        // with method name "+@" / "-@" / "!" / "~" and the receiver being
        // the operand.
        if matches!(method_name.as_str(), "+@" | "-@" | "!" | "~") {
            return true;
        }

        // Method on a basic literal => not acceptable (`1..2.to_a`).
        if let Some(recv) = call.receiver() {
            if is_basic_literal(&recv) { return false; }
        }

        // Operator method (binary): not acceptable unless it's `[]`.
        if is_operator_method(&method_name) { return false; }

        // Method call:
        //   - if `RequireParenthesesForMethodChains` true: only acceptable
        //     when it has no receiver (bareword call).
        //   - else: acceptable always (method calls allowed).
        if self.require_parens_for_chains {
            call.receiver().is_none()
        } else {
            true
        }
    }
}

/// Mirror RuboCop's `node.literal?` for atoms that don't need parens.
/// Includes: int, float, str, sym, true, false, nil, regexp, xstr,
/// dstr, dsym, array, hash. Also: simple range with literal bounds.
fn is_literal(node: &Node) -> bool {
    match node {
        Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::StringNode { .. }
        | Node::InterpolatedStringNode { .. }
        | Node::SymbolNode { .. }
        | Node::InterpolatedSymbolNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::NilNode { .. }
        | Node::RegularExpressionNode { .. }
        | Node::InterpolatedRegularExpressionNode { .. }
        | Node::XStringNode { .. }
        | Node::InterpolatedXStringNode { .. }
        | Node::ArrayNode { .. }
        | Node::HashNode { .. }
        | Node::ImaginaryNode { .. }
        | Node::SourceFileNode { .. }
        | Node::SourceLineNode { .. }
        | Node::SourceEncodingNode { .. } => true,
        _ => false,
    }
}

/// Recognize `1/3r`, `1/10r`, etc. RuboCop's RationalLiteral concern.
fn is_rational_literal(node: &Node) -> bool {
    if matches!(node, Node::RationalNode { .. }) { return true; }
    // `1/3r` parses as `(/ 1 3r)` -- a CallNode `/` with rational rhs.
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        let mname = String::from_utf8_lossy(call.name().as_slice()).into_owned();
        if mname == "/" {
            if let Some(recv) = call.receiver() {
                if matches!(recv, Node::IntegerNode { .. }) {
                    if let Some(args) = call.arguments() {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if arg_list.len() == 1 && matches!(arg_list[0], Node::RationalNode { .. }) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn is_variable(node: &Node) -> bool {
    matches!(node,
        Node::LocalVariableReadNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::GlobalVariableReadNode { .. }
        | Node::NumberedReferenceReadNode { .. }
        | Node::BackReferenceReadNode { .. }
        | Node::ItLocalVariableReadNode { .. }
    )
}

/// "basic_literal" in RuboCop is a node responding to true to `basic_literal?`.
/// Mainly numbers, strings, symbols, true/false/nil, regexp.
fn is_basic_literal(node: &Node) -> bool {
    matches!(node,
        Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. }
        | Node::StringNode { .. }
        | Node::SymbolNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::NilNode { .. }
        | Node::RegularExpressionNode { .. }
    )
}

fn is_operator_method(name: &str) -> bool {
    matches!(name,
        "+" | "-" | "*" | "/" | "%" | "**"
        | "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>"
        | "===" | "!~" | "=~"
        | "<<" | ">>" | "&" | "|" | "^"
        | "&&" | "||"
    )
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_range_node(&mut self, node: &ruby_prism::RangeNode) {
        if let Some(left) = node.left() { self.check_boundary(&left); }
        if let Some(right) = node.right() { self.check_boundary(&right); }
        ruby_prism::visit_range_node(self, node);
    }
}

crate::register_cop!("Lint/AmbiguousRange", |cfg| {
    let require_chains = cfg
        .get_cop_config("Lint/AmbiguousRange")
        .and_then(|c| c.raw.get("RequireParenthesesForMethodChains"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(Box::new(AmbiguousRange::new(require_chains)))
});
