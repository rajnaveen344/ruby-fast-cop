//! Layout/FirstParameterIndentation
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/first_parameter_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Location, Offense, Severity};

const COP_NAME: &str = "Layout/FirstParameterIndentation";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FirstParamStyle {
    Consistent,
    AlignParentheses,
}

pub struct FirstParameterIndentation {
    style: FirstParamStyle,
    indentation_width: usize,
}

impl FirstParameterIndentation {
    pub fn new(style: FirstParamStyle, indentation_width: usize) -> Self {
        Self { style, indentation_width }
    }
}

impl Default for FirstParameterIndentation {
    fn default() -> Self {
        Self::new(FirstParamStyle::Consistent, 2)
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source[..offset.min(source.len())].bytes().filter(|&b| b == b'\n').count()
}

fn line_start_of(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |p| p + 1)
}

fn col_of(source: &str, offset: usize) -> usize {
    offset - line_start_of(source, offset)
}

impl Cop for FirstParameterIndentation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;

        // Must have explicit parens
        let open = match node.lparen_loc() {
            Some(o) => o,
            None => return vec![],
        };

        // Must have params
        let params = match node.parameters() {
            Some(p) => p,
            None => return vec![],
        };

        // Must be multiline (open paren on different line than first param)
        let open_line = line_of(source, open.start_offset());

        // First param: collect all params, find the one with smallest start offset
        let mut first_param_start: Option<usize> = None;

        macro_rules! update_first {
            ($iter:expr) => {
                for n in $iter {
                    let s = n.location().start_offset();
                    first_param_start = Some(first_param_start.map_or(s, |cur| cur.min(s)));
                }
            };
        }

        update_first!(params.requireds().iter());
        update_first!(params.optionals().iter());
        update_first!(params.posts().iter());
        update_first!(params.keywords().iter());
        if let Some(rest) = params.rest() {
            let s = rest.location().start_offset();
            first_param_start = Some(first_param_start.map_or(s, |cur| cur.min(s)));
        }
        if let Some(kwrest) = params.keyword_rest() {
            let s = kwrest.location().start_offset();
            first_param_start = Some(first_param_start.map_or(s, |cur| cur.min(s)));
        }
        if let Some(block) = params.block() {
            let s = block.location().start_offset();
            first_param_start = Some(first_param_start.map_or(s, |cur| cur.min(s)));
        }

        let first_start = match first_param_start {
            Some(s) => s,
            None => return vec![],
        };

        let first_line = line_of(source, first_start);
        if first_line == open_line {
            // Single line params: skip
            return vec![];
        }

        let actual_col = col_of(source, first_start);

        let expected_col = match self.style {
            FirstParamStyle::Consistent => {
                // consistent: indentation of the line containing `(` + indentation_width
                let open_line_start = line_start_of(source, open.start_offset());
                let open_line_text = &source[open_line_start..];
                let line_indent = open_line_text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                line_indent + self.indentation_width
            }
            FirstParamStyle::AlignParentheses => {
                // align_parentheses: column of `(` + indentation_width
                // RuboCop's indent_base for brace_alignment_style returns left_brace.column (NOT +1)
                let paren_col = col_of(source, open.start_offset());
                paren_col + self.indentation_width
            }
        };

        if actual_col == expected_col {
            return vec![];
        }

        // Find end of first param on its line
        let first_line_start = line_start_of(source, first_start);
        let first_line_end = source[first_line_start..].find('\n')
            .map_or(source.len(), |p| first_line_start + p);
        // Offense: the first param token (just the first word on the line)
        let first_param_end = {
            // Find end of first param: use its node end but cap to current line
            let mut end_candidates = Vec::new();
            for n in params.requireds().iter() {
                if n.location().start_offset() == first_start {
                    end_candidates.push(n.location().end_offset());
                }
            }
            for n in params.optionals().iter() {
                if n.location().start_offset() == first_start {
                    end_candidates.push(n.location().end_offset());
                }
            }
            for n in params.keywords().iter() {
                if n.location().start_offset() == first_start {
                    end_candidates.push(n.location().end_offset());
                }
            }
            end_candidates.into_iter().min().unwrap_or(first_start + 1)
        };

        let message = match self.style {
            FirstParamStyle::Consistent => format!(
                "Use {} spaces for indentation in method args, relative to the start of the line where the left parenthesis is.",
                self.indentation_width
            ),
            FirstParamStyle::AlignParentheses => format!(
                "Use {} spaces for indentation in method args, relative to the position of the opening parenthesis.",
                self.indentation_width
            ),
        };

        let loc = Location::from_offsets(source, first_start, first_param_end.min(first_line_end));

        // Correction: replace the leading whitespace on the first param line
        let delta = expected_col as isize - actual_col as isize;
        let new_indent = " ".repeat(expected_col);
        let correction = Correction::replace(first_line_start, first_start, new_indent);

        vec![Offense::new(COP_NAME, &message, Severity::Convention, loc, ctx.filename)
            .with_correction(correction)]
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
    indentation_width: Option<serde_yaml::Value>,
}

crate::register_cop!("Layout/FirstParameterIndentation", |cfg| {
    let c: Cfg = cfg.typed("Layout/FirstParameterIndentation");
    let style = if c.enforced_style == "align_parentheses" {
        FirstParamStyle::AlignParentheses
    } else {
        FirstParamStyle::Consistent
    };
    // IndentationWidth can be "" (empty string) meaning use Layout/IndentationWidth
    let width = match &c.indentation_width {
        Some(serde_yaml::Value::Number(n)) => n.as_u64().map(|n| n as usize),
        Some(serde_yaml::Value::String(s)) if !s.is_empty() => s.parse::<usize>().ok(),
        _ => None,
    };
    let width = width
        .or_else(|| cfg.get_cop_config("Layout/IndentationWidth").and_then(|c| c.raw.get("Width")).and_then(|v| v.as_u64()).map(|n| n as usize))
        .unwrap_or(2);
    Some(Box::new(FirstParameterIndentation::new(style, width)))
});
