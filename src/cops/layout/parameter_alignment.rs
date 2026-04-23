//! Layout/ParameterAlignment — method definition parameter alignment.
//!
//! Port of `rubocop/cop/layout/parameter_alignment.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::alignment_check::{display_col_of, display_indent_of, each_bad_alignment};
use crate::offense::{Offense, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PAStyle {
    WithFirstParameter,
    WithFixedIndentation,
}

pub struct ParameterAlignment {
    style: PAStyle,
    indentation_width: usize,
}

impl ParameterAlignment {
    pub fn new(style: PAStyle, indentation_width: usize) -> Self {
        Self { style, indentation_width }
    }
}

const ALIGN_MSG: &str =
    "Align the parameters of a method definition if they span more than one line.";
const FIXED_MSG: &str =
    "Use one level of indentation for parameters following the first line of a multi-line method definition.";

impl Cop for ParameterAlignment {
    fn name(&self) -> &'static str { "Layout/ParameterAlignment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(params) = node.parameters() else { return vec![] };
        let items = collect_param_ranges(&params);
        if items.len() < 2 {
            return vec![];
        }

        let base_column = match self.style {
            PAStyle::WithFirstParameter => display_col_of(ctx, items[0].0),
            PAStyle::WithFixedIndentation => {
                // target line = line of `def` keyword
                let def_kw = node.def_keyword_loc();
                display_indent_of(ctx, def_kw.start_offset()) + self.indentation_width
            }
        };

        let msg = match self.style {
            PAStyle::WithFirstParameter => ALIGN_MSG,
            PAStyle::WithFixedIndentation => FIXED_MSG,
        };

        each_bad_alignment(ctx, &items, base_column)
            .into_iter()
            .map(|m| {
                ctx.offense_with_range(
                    self.name(),
                    msg,
                    self.severity(),
                    m.start_offset,
                    m.end_offset,
                )
            })
            .collect()
    }
}

/// Collect (start_offset, end_offset) for each method parameter, in source order.
fn collect_param_ranges(params: &ruby_prism::ParametersNode<'_>) -> Vec<(usize, usize)> {
    let mut items: Vec<(usize, usize)> = Vec::new();
    let push = |items: &mut Vec<(usize, usize)>, loc: ruby_prism::Location<'_>| {
        items.push((loc.start_offset(), loc.end_offset()));
    };
    for n in params.requireds().iter() { push(&mut items, n.location()); }
    for n in params.optionals().iter() { push(&mut items, n.location()); }
    if let Some(n) = params.rest() { push(&mut items, n.location()); }
    for n in params.posts().iter() { push(&mut items, n.location()); }
    for n in params.keywords().iter() { push(&mut items, n.location()); }
    if let Some(n) = params.keyword_rest() { push(&mut items, n.location()); }
    if let Some(n) = params.block() { push(&mut items, n.location()); }
    items.sort_by_key(|&(s, _)| s);
    items
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
    indentation_width: Option<serde_yaml::Value>,
}

crate::register_cop!("Layout/ParameterAlignment", |cfg| {
    let c: Cfg = cfg.typed("Layout/ParameterAlignment");
    let style = if c.enforced_style == "with_fixed_indentation" {
        PAStyle::WithFixedIndentation
    } else {
        PAStyle::WithFirstParameter
    };
    let width = match &c.indentation_width {
        Some(serde_yaml::Value::Number(n)) => n.as_u64().map(|n| n as usize),
        Some(serde_yaml::Value::String(s)) if !s.is_empty() => s.parse::<usize>().ok(),
        _ => None,
    };
    let width = width
        .or_else(|| {
            cfg.get_cop_config("Layout/IndentationWidth")
                .and_then(|c| c.raw.get("Width"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
        })
        .unwrap_or(2);
    Some(Box::new(ParameterAlignment::new(style, width)))
});
