use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

#[derive(Default)]
pub struct WhenThen;

impl WhenThen {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for WhenThen {
    fn name(&self) -> &'static str {
        "Style/WhenThen"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_when(&self, node: &ruby_prism::WhenNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only flag single-line when clauses with body
        if node.statements().is_none() {
            return vec![];
        }

        // If there's a `then` keyword, check if it's already `then` (not `;`)
        if let Some(then_loc) = node.then_keyword_loc() {
            if then_loc.as_slice() == b"then" {
                return vec![]; // already good
            }
            // It's some other separator (shouldn't normally happen)
        }
        // then_keyword_loc is None for semicolon case in Prism

        // Must be single-line
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        if ctx.source.as_bytes()[node_start..node_end].contains(&b'\n') {
            return vec![];
        }

        // Find semicolon after last condition and before statements
        // Get last condition end offset
        let last_cond_end = node.conditions().iter().last().map_or(
            node.keyword_loc().end_offset(),
            |c| c.location().end_offset(),
        );

        let stmts = node.statements().unwrap();
        let first_stmt_start = stmts.body().iter().next().map_or(
            node.location().end_offset(),
            |s| s.location().start_offset(),
        );

        // Scan from last_cond_end to first_stmt_start for `;`
        let between = &ctx.source[last_cond_end..first_stmt_start];
        let semi_offset = match between.find(';') {
            Some(o) => last_cond_end + o,
            None => return vec![], // no semicolon found — no offense
        };

        // Build conditions string for message
        let conditions: Vec<&str> = node
            .conditions()
            .iter()
            .map(|c| {
                let s = c.location().start_offset();
                let e = c.location().end_offset();
                &ctx.source[s..e]
            })
            .collect();
        let expr = conditions.join(", ");
        let msg = format!(
            "Do not use `when {};`. Use `when {} then` instead.",
            expr, expr
        );

        let loc = Location::from_offsets(ctx.source, semi_offset, semi_offset + 1);
        vec![Offense::new(self.name(), &msg, self.severity(), loc, ctx.filename)]
    }
}

crate::register_cop!("Style/WhenThen", |_cfg| Some(Box::new(WhenThen::new())));
