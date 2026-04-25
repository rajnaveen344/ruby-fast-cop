//! Lint/RequireRangeParentheses cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};
use ruby_prism::Visit;

#[derive(Default)]
pub struct RequireRangeParentheses;

impl RequireRangeParentheses {
    pub fn new() -> Self { Self }
}

impl Cop for RequireRangeParentheses {
    fn name(&self) -> &'static str { "Lint/RequireRangeParentheses" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, paren_depth: 0, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    paren_depth: usize,
    out: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        self.paren_depth += 1;
        ruby_prism::visit_parentheses_node(self, node);
        self.paren_depth -= 1;
    }

    fn visit_range_node(&mut self, node: &ruby_prism::RangeNode) {
        if self.paren_depth == 0 {
            if let (Some(l), Some(r)) = (node.left(), node.right()) {
                let lloc = l.location();
                let rloc = r.location();
                let l_end = Location::from_offsets(self.ctx.source, lloc.start_offset(), lloc.end_offset());
                let r_start = Location::from_offsets(self.ctx.source, rloc.start_offset(), rloc.end_offset());
                if l_end.last_line < r_start.line {
                    let op_loc = node.operator_loc();
                    let start = lloc.start_offset();
                    let end = op_loc.end_offset();
                    let op_text = &self.ctx.source[op_loc.start_offset()..op_loc.end_offset()];
                    let begin_text = &self.ctx.source[lloc.start_offset()..lloc.end_offset()];
                    let msg = format!("Wrap the endless range literal `{}{}` to avoid precedence ambiguity.", begin_text, op_text);
                    self.out.push(self.ctx.offense_with_range(
                        "Lint/RequireRangeParentheses", &msg, Severity::Warning, start, end,
                    ));
                }
            }
        }
        ruby_prism::visit_range_node(self, node);
    }
}

crate::register_cop!("Lint/RequireRangeParentheses", |_cfg| Some(Box::new(RequireRangeParentheses::new())));
