//! Layout/SpaceBeforeBrackets - Checks for space between the name of a receiver
//! and a left bracket.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_before_brackets.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Location, Offense, Severity};

#[derive(Default)]
pub struct SpaceBeforeBrackets;

impl SpaceBeforeBrackets {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for SpaceBeforeBrackets {
    fn name(&self) -> &'static str {
        "Layout/SpaceBeforeBrackets"
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only `[]` / `[]=` calls (RESTRICT_ON_SEND in RuboCop).
        let method = node_name!(node);
        if method != "[]" && method != "[]=" {
            return vec![];
        }

        // Skip `obj.[](k)` form.
        if node.call_operator_loc().is_some() {
            return vec![];
        }

        let Some(receiver) = node.receiver() else {
            return vec![];
        };
        let Some(msg_loc) = node.message_loc() else {
            return vec![];
        };

        let recv_end = receiver.location().end_offset();
        let msg_start = msg_loc.start_offset();

        if recv_end >= msg_start {
            return vec![];
        }

        // Verify the gap is whitespace (defensive — should always be).
        let gap = &ctx.source[recv_end..msg_start];
        if gap.is_empty() || !gap.chars().all(|c| c == ' ' || c == '\t') {
            return vec![];
        }

        let location = Location::from_offsets(ctx.source, recv_end, msg_start);
        let offense = Offense::new(
            self.name(),
            "Remove the space before the opening brackets.",
            self.severity(),
            location,
            ctx.filename,
        )
        .with_correction(Correction::delete(recv_end, msg_start));

        vec![offense]
    }
}

crate::register_cop!("Layout/SpaceBeforeBrackets", |_cfg| Some(Box::new(
    SpaceBeforeBrackets::new()
)));
