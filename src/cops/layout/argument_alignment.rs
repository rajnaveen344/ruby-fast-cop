//! Layout/ArgumentAlignment — multiline method call argument alignment.
//!
//! Port of `rubocop/cop/layout/argument_alignment.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::alignment_check::{display_col_of, display_indent_of, each_bad_alignment};
use crate::node_name;
use crate::offense::{Offense, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgAStyle {
    WithFirstArgument,
    WithFixedIndentation,
}

pub struct ArgumentAlignment {
    style: ArgAStyle,
    indentation_width: usize,
}

impl ArgumentAlignment {
    pub fn new(style: ArgAStyle, indentation_width: usize) -> Self {
        Self { style, indentation_width }
    }
}

const ALIGN_MSG: &str =
    "Align the arguments of a method call if they span more than one line.";
const FIXED_MSG: &str =
    "Use one level of indentation for arguments following the first line of a multi-line method call.";

impl Cop for ArgumentAlignment {
    fn name(&self) -> &'static str { "Layout/ArgumentAlignment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip `x[] = y` assignment form.
        let method = node_name!(node);
        if method == "[]=" {
            return vec![];
        }

        let Some(args) = node.arguments() else { return vec![] };
        let arg_nodes: Vec<_> = args.arguments().iter().collect();
        if arg_nodes.is_empty() {
            return vec![];
        }
        if !multiple_arguments(&arg_nodes) {
            return vec![];
        }

        // Build item list depending on style.
        let items = flattened_items(&arg_nodes, self.style);

        // Base column.
        let base_column = match self.style {
            ArgAStyle::WithFirstArgument => {
                let first = items.first().copied();
                match first {
                    Some((s, _)) => display_col_of(ctx, s),
                    None => return vec![],
                }
            }
            ArgAStyle::WithFixedIndentation => {
                let target_offset = target_method_start(node);
                display_indent_of(ctx, target_offset) + self.indentation_width
            }
        };

        let msg = match self.style {
            ArgAStyle::WithFirstArgument => ALIGN_MSG,
            ArgAStyle::WithFixedIndentation => FIXED_MSG,
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

/// Start byte offset of the call's "target line" (selector line, or `(` line
/// for `l.()`).
fn target_method_start(node: &ruby_prism::CallNode<'_>) -> usize {
    if let Some(msg) = node.message_loc() {
        return msg.start_offset();
    }
    if let Some(lp) = node.opening_loc() {
        return lp.start_offset();
    }
    node.location().start_offset()
}

/// True if: #args >= 2, OR single braceless-hash arg with >=2 pairs.
fn multiple_arguments(arg_nodes: &[ruby_prism::Node<'_>]) -> bool {
    if arg_nodes.len() >= 2 {
        return true;
    }
    if let Some(first) = arg_nodes.first() {
        if let Some(kh) = first.as_keyword_hash_node() {
            return kh.elements().iter().count() >= 2;
        }
    }
    false
}

/// Flatten arguments per RuboCop's mixin logic.
///
/// - `with_first_argument`: if first arg is braceless hash, use its pairs;
///   else use the argument list as-is.
/// - `with_fixed_indentation`: use all args except last; if last is braceless
///   hash, append its pairs, else append last.
fn flattened_items<'a>(
    args: &[ruby_prism::Node<'a>],
    style: ArgAStyle,
) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    match style {
        ArgAStyle::WithFirstArgument => {
            if args.len() == 1 {
                if let Some(kh) = args[0].as_keyword_hash_node() {
                    for pair in kh.elements().iter() {
                        out.push(range_of(&pair));
                    }
                    return out;
                }
            }
            for a in args {
                out.push(range_of(a));
            }
        }
        ArgAStyle::WithFixedIndentation => {
            if args.is_empty() {
                return out;
            }
            let last = args.last().unwrap();
            for a in &args[..args.len() - 1] {
                out.push(range_of(a));
            }
            if let Some(kh) = last.as_keyword_hash_node() {
                for pair in kh.elements().iter() {
                    out.push(range_of(&pair));
                }
            } else {
                out.push(range_of(last));
            }
        }
    }
    out
}

fn range_of(n: &ruby_prism::Node<'_>) -> (usize, usize) {
    let loc = n.location();
    (loc.start_offset(), loc.end_offset())
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
    indentation_width: Option<serde_yaml::Value>,
}

crate::register_cop!("Layout/ArgumentAlignment", |cfg| {
    let c: Cfg = cfg.typed("Layout/ArgumentAlignment");
    let style = if c.enforced_style == "with_fixed_indentation" {
        ArgAStyle::WithFixedIndentation
    } else {
        ArgAStyle::WithFirstArgument
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
    Some(Box::new(ArgumentAlignment::new(style, width)))
});
