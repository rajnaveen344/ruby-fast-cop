//! Lint/AmbiguousOperatorPrecedence — flag mixed-precedence binary ops without parens.
//! Ports `RuboCop::Cop::Lint::AmbiguousOperatorPrecedence`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct AmbiguousOperatorPrecedence;

impl AmbiguousOperatorPrecedence {
    pub fn new() -> Self { Self }
}

const MSG: &str =
    "Wrap expressions with varying precedence with parentheses to avoid ambiguity.";

/// PRECEDENCE table: index 0 = highest. Returns Some(idx) for ranked operators.
fn precedence_of_op(name: &str) -> Option<usize> {
    Some(match name {
        "**" => 0,
        "*" | "/" | "%" => 1,
        "+" | "-" => 2,
        "<<" | ">>" => 3,
        "&" => 4,
        "|" | "^" => 5,
        "&&" => 6,
        "||" => 7,
        _ => return None,
    })
}

impl Cop for AmbiguousOperatorPrecedence {
    fn name(&self) -> &'static str { "Lint/AmbiguousOperatorPrecedence" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = V { ctx, parents: Vec::new(), out: Vec::new() };
        v.visit(&result.node());
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    /// Stack of node kinds we use for parent lookup. We push a marker for each node we visit.
    parents: Vec<ParentKind>,
    out: Vec<Offense>,
}

#[derive(Clone, Copy)]
enum ParentKind {
    Operator(usize), // precedence index
    Other,
}

impl<'a, 'b> V<'a, 'b> {
    fn current_parent(&self) -> Option<ParentKind> {
        self.parents.last().copied()
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name_b = node.name();
        let name = String::from_utf8_lossy(name_b.as_slice()).to_string();
        let parenthesized = node.opening_loc().is_some();
        let node_prec = if parenthesized { None } else { precedence_of_op(&name) };
        if let Some(np) = node_prec {
            if let Some(ParentKind::Operator(pp)) = self.current_parent() {
                if pp > np {
                    let l = node.location();
                    let s = l.start_offset();
                    let e = l.end_offset();
                    let mut c = Correction::insert(s, "(");
                    c.edits.push(Edit { start_offset: e, end_offset: e, replacement: ")".to_string() });
                    self.out.push(
                        self.ctx
                            .offense_with_range(
                                "Lint/AmbiguousOperatorPrecedence",
                                MSG,
                                Severity::Warning,
                                s,
                                e,
                            )
                            .with_correction(c),
                    );
                }
            }
        }
        // Push this node as parent.
        let kind = match node_prec {
            Some(p) => ParentKind::Operator(p),
            None => ParentKind::Other,
        };
        self.parents.push(kind);
        ruby_prism::visit_call_node(self, node);
        self.parents.pop();
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        // Flag this AndNode if parent is OrNode (per on_and).
        if let Some(ParentKind::Operator(7)) = self.current_parent() {
            let l = node.location();
            let s = l.start_offset();
            let e = l.end_offset();
            let mut c = Correction::insert(s, "(");
            c.edits.push(Edit { start_offset: e, end_offset: e, replacement: ")".to_string() });
            self.out.push(
                self.ctx
                    .offense_with_range(
                        "Lint/AmbiguousOperatorPrecedence",
                        MSG,
                        Severity::Warning,
                        s,
                        e,
                    )
                    .with_correction(c),
            );
        }
        // operator src determines whether parent_kind is Operator(6) (&&) or Other (and).
        let opsrc = std::str::from_utf8(node.operator_loc().as_slice()).unwrap_or("");
        let kind = if opsrc == "&&" { ParentKind::Operator(6) } else { ParentKind::Other };
        self.parents.push(kind);
        ruby_prism::visit_and_node(self, node);
        self.parents.pop();
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let opsrc = std::str::from_utf8(node.operator_loc().as_slice()).unwrap_or("");
        let kind = if opsrc == "||" { ParentKind::Operator(7) } else { ParentKind::Other };
        self.parents.push(kind);
        ruby_prism::visit_or_node(self, node);
        self.parents.pop();
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        // Parens reset parent context — children's parent is "Other" not the outer operator.
        self.parents.push(ParentKind::Other);
        ruby_prism::visit_parentheses_node(self, node);
        self.parents.pop();
    }
}

crate::register_cop!("Lint/AmbiguousOperatorPrecedence", |_cfg| {
    Some(Box::new(AmbiguousOperatorPrecedence::new()))
});
