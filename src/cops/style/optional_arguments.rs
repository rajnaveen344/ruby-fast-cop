//! Style/OptionalArguments cop
//!
//! Checks that optional arguments appear at the end of the argument list.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Style/OptionalArguments";
const MSG: &str = "Optional arguments should appear at the end of the argument list.";

#[derive(Default)]
pub struct OptionalArguments;

impl OptionalArguments {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for OptionalArguments {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![],
        };

        // Build ordered list of positional params (required + optional) with their position
        // to determine if any optional precedes a required.
        // ParametersNode has: requireds(), optionals() — these are position-ordered.
        // We need to interleave them by offset to find misplaced optionals.

        // In Prism, required params before optionals are in requireds(),
        // required params AFTER optionals are in posts().
        let requireds: Vec<Node> = params.requireds().iter().collect();
        let posts: Vec<Node> = params.posts().iter().collect();
        let optionals: Vec<Node> = params.optionals().iter().collect();

        if optionals.is_empty() {
            return vec![];
        }

        // If there are no required params at all, no offense
        if requireds.is_empty() && posts.is_empty() {
            return vec![];
        }

        // RuboCop flags optional args that precede required args.
        // posts() = required args that follow optionals/rest → always after optionals.
        // But if posts is non-empty, that means required args come AFTER optionals — not an offense.
        // The offense is: optional comes BEFORE a required positional.
        // requireds() = required args that come first (before optionals) — fine.
        // posts() = required args after optionals — also fine (this IS the expected pattern).
        // HOWEVER: if optionals come before requireds (in the source), flag.
        // So: collect all required start offsets (from requireds + posts),
        // and flag any optional whose start offset < max(required starts).

        // Actually the pattern is: `def foo(a = 1, b)` — `b` is in posts.
        // RuboCop flags `a = 1` because `b` comes after it (required after optional).
        // So we need: any optional that has a required (from requireds OR posts) somewhere after it.

        // posts() items are always after optionals — so if posts is non-empty, flag optionals.
        // requireds() items that come after some optional — find them by offset comparison.

        let all_required_starts: Vec<usize> = requireds.iter()
            .chain(posts.iter())
            .map(|n| n.location().start_offset())
            .collect();

        if all_required_starts.is_empty() {
            return vec![];
        }

        let last_required_start = all_required_starts.iter().copied().max().unwrap_or(0);

        let mut offenses = vec![];
        for opt in &optionals {
            let opt_start = opt.location().start_offset();
            if opt_start < last_required_start {
                let opt_end = opt.location().end_offset();
                offenses.push(ctx.offense_with_range(
                    COP_NAME, MSG, Severity::Convention, opt_start, opt_end,
                ));
            }
        }

        offenses
    }
}

crate::register_cop!("Style/OptionalArguments", |_cfg| {
    Some(Box::new(OptionalArguments::new()))
});
