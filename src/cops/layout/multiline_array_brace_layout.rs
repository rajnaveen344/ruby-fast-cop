//! Layout/MultilineArrayBraceLayout
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_array_brace_layout.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_literal_brace_layout as helper;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/MultilineArrayBraceLayout";

const MESSAGES: helper::Messages = helper::Messages {
    same_line: "The closing array brace must be on the same line as the last array element \
                when the opening brace is on the same line as the first array element.",
    new_line: "The closing array brace must be on the line after the last array element \
               when the opening brace is on a separate line from the first array element.",
    always_new_line: "The closing array brace must be on the line after the last array element.",
    always_same_line: "The closing array brace must be on the same line as the last array element.",
};

pub struct MultilineArrayBraceLayout {
    style: helper::BraceLayoutStyle,
}

impl MultilineArrayBraceLayout {
    pub fn new(style: helper::BraceLayoutStyle) -> Self {
        Self { style }
    }
}

impl Default for MultilineArrayBraceLayout {
    fn default() -> Self {
        Self::new(helper::BraceLayoutStyle::Symmetrical)
    }
}

impl Cop for MultilineArrayBraceLayout {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        // Implicit (%w, etc. with no bracket) or no brackets: skip.
        let (Some(open), Some(close)) = (node.opening_loc(), node.closing_loc()) else {
            return vec![];
        };
        // Only handle real bracket arrays (skip %w(), %i(), etc.)
        let open_bytes = &ctx.source.as_bytes()[open.start_offset()..open.end_offset()];
        if open_bytes != b"[" {
            return vec![];
        }

        let elements: Vec<ruby_prism::Node> = node.elements().iter().collect();
        if elements.is_empty() {
            return vec![];
        }

        let first = elements.first().unwrap();
        let last = elements.last().unwrap();
        let first_start = first.location().start_offset();
        let last_end = last.location().end_offset();

        // Single line: skip.
        let open_line = line_of(ctx.source, open.start_offset());
        let close_line = line_of(ctx.source, close.start_offset());
        if open_line == close_line {
            return vec![];
        }

        // Heredoc detection: parent = last child (RuboCop's `last_line_heredoc?(node.children.last)`).
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

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}
impl Default for Cfg {
    fn default() -> Self { Self { enforced_style: "symmetrical".into() } }
}

crate::register_cop!("Layout/MultilineArrayBraceLayout", |cfg| {
    let c: Cfg = cfg.typed("Layout/MultilineArrayBraceLayout");
    let style = helper::BraceLayoutStyle::from_str(&c.enforced_style);
    Some(Box::new(MultilineArrayBraceLayout::new(style)))
});
