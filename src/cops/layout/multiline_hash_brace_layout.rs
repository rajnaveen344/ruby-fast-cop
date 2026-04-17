//! Layout/MultilineHashBraceLayout
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_hash_brace_layout.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_literal_brace_layout as helper;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/MultilineHashBraceLayout";

const MESSAGES: helper::Messages = helper::Messages {
    same_line: "Closing hash brace must be on the same line as the last hash element \
                when opening brace is on the same line as the first hash element.",
    new_line: "Closing hash brace must be on the line after the last hash element \
               when opening brace is on a separate line from the first hash element.",
    always_new_line: "Closing hash brace must be on the line after the last hash element.",
    always_same_line: "Closing hash brace must be on the same line as the last hash element.",
};

pub struct MultilineHashBraceLayout {
    style: helper::BraceLayoutStyle,
}

impl MultilineHashBraceLayout {
    pub fn new(style: helper::BraceLayoutStyle) -> Self {
        Self { style }
    }
}

impl Default for MultilineHashBraceLayout {
    fn default() -> Self {
        Self::new(helper::BraceLayoutStyle::Symmetrical)
    }
}

impl Cop for MultilineHashBraceLayout {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_hash(&self, node: &ruby_prism::HashNode, ctx: &CheckContext) -> Vec<Offense> {
        let open = node.opening_loc();
        let close = node.closing_loc();

        let elements: Vec<ruby_prism::Node> = node.elements().iter().collect();
        if elements.is_empty() {
            return vec![];
        }

        let first = elements.first().unwrap();
        let last = elements.last().unwrap();
        let first_start = first.location().start_offset();
        let last_end = last.location().end_offset();

        // Single-line: skip.
        let open_line = line_of(ctx.source, open.start_offset());
        let close_line = line_of(ctx.source, close.start_offset());
        if open_line == close_line {
            return vec![];
        }

        // Heredoc detection: parent = last child.
        let last_child_last_line = line_of(ctx.source, last_end.saturating_sub(1));
        if helper::last_line_heredoc(ctx.source, last, last_child_last_line) {
            return vec![];
        }

        helper::check(
            ctx,
            &helper::BraceCheck {
                cop_name: COP_NAME,
                style: self.style,
                messages: &MESSAGES,
                open_start: open.start_offset(),
                open_end: open.end_offset(),
                close_start: close.start_offset(),
                close_end: close.end_offset(),
                first_child_start: first_start,
                last_child_end: last_end,
            },
        )
    }
}

fn line_of(src: &str, offset: usize) -> usize {
    1 + src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

crate::register_cop!("Layout/MultilineHashBraceLayout", |cfg| {
    let style = cfg
        .get_cop_config("Layout/MultilineHashBraceLayout")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| helper::BraceLayoutStyle::from_str(s))
        .unwrap_or(helper::BraceLayoutStyle::Symmetrical);
    Some(Box::new(MultilineHashBraceLayout::new(style)))
});
