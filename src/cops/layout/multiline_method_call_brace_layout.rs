//! Layout/MultilineMethodCallBraceLayout
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_method_call_brace_layout.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_literal_brace_layout as helper;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/MultilineMethodCallBraceLayout";

const MESSAGES: helper::Messages = helper::Messages {
    same_line: "Closing method call brace must be on the same line as the last argument \
                when opening brace is on the same line as the first argument.",
    new_line: "Closing method call brace must be on the line after the last argument \
               when opening brace is on a separate line from the first argument.",
    always_new_line: "Closing method call brace must be on the line after the last argument.",
    always_same_line: "Closing method call brace must be on the same line as the last argument.",
};

pub struct MultilineMethodCallBraceLayout {
    style: helper::BraceLayoutStyle,
}

impl MultilineMethodCallBraceLayout {
    pub fn new(style: helper::BraceLayoutStyle) -> Self {
        Self { style }
    }
}

impl Default for MultilineMethodCallBraceLayout {
    fn default() -> Self {
        Self::new(helper::BraceLayoutStyle::Symmetrical)
    }
}

impl Cop for MultilineMethodCallBraceLayout {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Implicit (no parens): skip.
        let (Some(open), Some(close)) = (node.opening_loc(), node.closing_loc()) else {
            return vec![];
        };

        // single_line_ignoring_receiver: open and close on same line.
        let open_line = line_of(ctx.source, open.start_offset());
        let close_line = line_of(ctx.source, close.start_offset());
        if open_line == close_line {
            return vec![];
        }

        // Empty args: skip.
        let Some(args_node) = node.arguments() else {
            return vec![];
        };
        let args: Vec<ruby_prism::Node> = args_node.arguments().iter().collect();
        if args.is_empty() {
            return vec![];
        }

        let first = args.first().unwrap();
        let last = args.last().unwrap();
        let first_start = first.location().start_offset();
        let last_end = last.location().end_offset();

        // Heredoc detection: parent = last argument.
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

crate::register_cop!("Layout/MultilineMethodCallBraceLayout", |cfg| {
    let style = cfg
        .get_cop_config("Layout/MultilineMethodCallBraceLayout")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| helper::BraceLayoutStyle::from_str(s))
        .unwrap_or(helper::BraceLayoutStyle::Symmetrical);
    Some(Box::new(MultilineMethodCallBraceLayout::new(style)))
});
