//! Layout/MultilineMethodDefinitionBraceLayout
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_method_definition_brace_layout.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::multiline_literal_brace_layout as helper;
use crate::offense::{Offense, Severity};

const COP_NAME: &str = "Layout/MultilineMethodDefinitionBraceLayout";

const MESSAGES: helper::Messages = helper::Messages {
    same_line: "Closing method definition brace must be on the same line as the last parameter \
                when opening brace is on the same line as the first parameter.",
    new_line: "Closing method definition brace must be on the line after the last parameter \
               when opening brace is on a separate line from the first parameter.",
    always_new_line: "Closing method definition brace must be on the line after the last parameter.",
    always_same_line: "Closing method definition brace must be on the same line as the last parameter.",
};

pub struct MultilineMethodDefinitionBraceLayout {
    style: helper::BraceLayoutStyle,
}

impl MultilineMethodDefinitionBraceLayout {
    pub fn new(style: helper::BraceLayoutStyle) -> Self {
        Self { style }
    }
}

impl Default for MultilineMethodDefinitionBraceLayout {
    fn default() -> Self {
        Self::new(helper::BraceLayoutStyle::Symmetrical)
    }
}

fn line_of(src: &str, offset: usize) -> usize {
    1 + src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

fn check_def_node(
    style: helper::BraceLayoutStyle,
    node: &ruby_prism::DefNode<'_>,
    ctx: &CheckContext,
) -> Vec<Offense> {
    let open = match node.lparen_loc() {
        Some(o) => o,
        None => return vec![], // no explicit parens
    };
    let close = match node.rparen_loc() {
        Some(c) => c,
        None => return vec![],
    };

    // Single line: skip
    let open_line = line_of(ctx.source, open.start_offset());
    let close_line = line_of(ctx.source, close.start_offset());
    if open_line == close_line {
        return vec![];
    }

    // Must have params
    let params = match node.parameters() {
        Some(p) => p,
        None => return vec![],
    };

    // Collect all parameters as nodes
    let mut all_params: Vec<ruby_prism::Node> = Vec::new();
    all_params.extend(params.requireds().iter());
    all_params.extend(params.optionals().iter());
    all_params.extend(params.posts().iter());
    all_params.extend(params.keywords().iter());
    if let Some(rest) = params.rest() { all_params.push(rest); }
    if let Some(kwrest) = params.keyword_rest() { all_params.push(kwrest); }
    // block param
    if let Some(bp) = params.block() {
        all_params.push(bp.as_node());
    }

    if all_params.is_empty() {
        return vec![];
    }

    // Sort by start offset
    all_params.sort_by_key(|n| n.location().start_offset());

    let first = all_params.first().unwrap();
    let last = all_params.last().unwrap();
    let first_start = first.location().start_offset();
    let last_end = last.location().end_offset();

    // Heredoc detection
    let last_child_last_line = line_of(ctx.source, last_end.saturating_sub(1));
    if helper::last_line_heredoc(ctx.source, last, last_child_last_line) {
        return vec![];
    }

    helper::check(
        ctx,
        &helper::BraceCheck {
            cop_name: COP_NAME,
            style,
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

impl Cop for MultilineMethodDefinitionBraceLayout {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        check_def_node(self.style, node, ctx)
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}
impl Default for Cfg {
    fn default() -> Self { Self { enforced_style: "symmetrical".into() } }
}

crate::register_cop!("Layout/MultilineMethodDefinitionBraceLayout", |cfg| {
    let c: Cfg = cfg.typed("Layout/MultilineMethodDefinitionBraceLayout");
    let style = helper::BraceLayoutStyle::from_str(&c.enforced_style);
    Some(Box::new(MultilineMethodDefinitionBraceLayout::new(style)))
});
