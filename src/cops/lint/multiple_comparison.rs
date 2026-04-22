//! Lint/MultipleComparison - Checks for multiple comparison like `x < y < z`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Use the `&&` operator to compare multiple values.";
const COMPARISON_METHODS: &[&str] = &["<", ">", "<=", ">="];
const SET_OPERATION_OPERATORS: &[&str] = &["&", "|", "^"];

#[derive(Default)]
pub struct MultipleComparison;

impl MultipleComparison {
    pub fn new() -> Self { Self }
}

impl Cop for MultipleComparison {
    fn name(&self) -> &'static str { "Lint/MultipleComparison" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let outer_method = String::from_utf8_lossy(node.name().as_slice());
        let outer_method_str = outer_method.as_ref();

        if !COMPARISON_METHODS.contains(&outer_method_str) {
            return;
        }

        // The receiver must be a comparison call too: (recv_call op center) op2 rhs
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        let recv_call = match receiver.as_call_node() {
            Some(c) => c,
            None => return,
        };

        let inner_method = String::from_utf8_lossy(recv_call.name().as_slice());
        let inner_method_str = inner_method.as_ref();

        if !COMPARISON_METHODS.contains(&inner_method_str) {
            return;
        }

        // `center` is the rhs of the inner call (and lhs of the outer call)
        let center = match recv_call.arguments().and_then(|args| {
            let items: Vec<_> = args.arguments().iter().collect();
            items.into_iter().next()
        }) {
            Some(c) => c,
            None => return,
        };

        // Skip if center is a set-operation call (& | ^)
        if let Some(center_call) = center.as_call_node() {
            let center_method = String::from_utf8_lossy(center_call.name().as_slice());
            if SET_OPERATION_OPERATORS.contains(&center_method.as_ref()) {
                return;
            }
        }

        // Get source of center
        let center_src = &self.ctx.source[center.location().start_offset()..center.location().end_offset()];

        // Offense range is the whole outer call
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        // Correction: replace `center` with `center && center`
        let new_center = format!("{} && {}", center_src, center_src);
        let correction = Correction::replace(
            center.location().start_offset(),
            center.location().end_offset(),
            new_center,
        );

        let mut offense = self.ctx.offense_with_range(
            "Lint/MultipleComparison",
            MSG,
            Severity::Warning,
            start,
            end,
        );
        offense.correction = Some(correction);
        self.offenses.push(offense);
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/MultipleComparison", |_cfg| Some(Box::new(MultipleComparison::new())));
