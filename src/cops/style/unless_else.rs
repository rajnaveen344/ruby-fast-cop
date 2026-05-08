use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

const MSG: &str =
    "Do not use `unless` with `else`. Rewrite these with the positive case first.";

#[derive(Default)]
pub struct UnlessElse;

impl UnlessElse {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for UnlessElse {
    fn name(&self) -> &'static str {
        "Style/UnlessElse"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        // Flag if unless has an else clause
        let else_clause = match node.else_clause() {
            Some(e) => e,
            None => return vec![],
        };

        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();

        // Build correction: replace `unless` with `if`, swap body ↔ else-body
        let keyword_start = node.keyword_loc().start_offset();
        let keyword_end = node.keyword_loc().end_offset();

        // body_range starts after `then` if present, else after predicate
        let pred_end = if let Some(then_loc) = node.then_keyword_loc() {
            then_loc.end_offset()
        } else {
            node.predicate().location().end_offset()
        };
        let else_kw_start = else_clause.else_keyword_loc().start_offset();
        let else_kw_end = else_clause.else_keyword_loc().end_offset();
        let end_kw_start = match node.end_keyword_loc() {
            Some(e) => e.start_offset(),
            None => return vec![ctx.offense_with_range(self.name(), MSG, self.severity(), node_start, node_end)],
        };

        // body_range: from pred_end to else_kw_start
        let body_src = ctx.source[pred_end..else_kw_start].to_string();
        // else_range: from else_kw_end to end_kw_start
        let else_src = ctx.source[else_kw_end..end_kw_start].to_string();

        // Swap: edit 1 = replace unless→if, edit 2 = replace body with else_src, edit 3 = replace else_src with body_src
        // Apply in reverse order (desc by offset)
        let correction = Correction {
            edits: vec![
                Edit { start_offset: else_kw_end, end_offset: end_kw_start, replacement: body_src },
                Edit { start_offset: pred_end, end_offset: else_kw_start, replacement: else_src },
                Edit { start_offset: keyword_start, end_offset: keyword_end, replacement: "if".to_string() },
            ],
        };

        let offense = ctx.offense_with_range(self.name(), MSG, self.severity(), node_start, node_end)
            .with_correction(correction);
        vec![offense]
    }
}

crate::register_cop!("Style/UnlessElse", |_cfg| Some(Box::new(UnlessElse::new())));
