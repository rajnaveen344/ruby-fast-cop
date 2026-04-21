//! Style/EmptyMethod - Checks for formatting of empty method definitions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/empty_method.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Style/EmptyMethod";
const MSG_COMPACT: &str = "Put empty method definitions on a single line.";
const MSG_EXPANDED: &str = "Put the `end` of empty method definitions on the next line.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle { Compact, Expanded }

pub struct EmptyMethod {
    style: EnforcedStyle,
}

impl Default for EmptyMethod {
    fn default() -> Self { Self { style: EnforcedStyle::Compact } }
}

impl EmptyMethod {
    pub fn new() -> Self { Self::default() }
    pub fn with_style(style: EnforcedStyle) -> Self { Self { style } }
}

impl Cop for EmptyMethod {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        // body is None = empty
        if node.body().is_some() { return vec![]; }

        // Skip if contains comment in def range
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        if region_contains_comment(ctx.source, start, end) { return vec![]; }

        // source on single line?
        let src = &ctx.source[start..end];
        let is_single_line = !src.contains('\n');

        let offense = match self.style {
            // compact: bad if multi-line
            EnforcedStyle::Compact => {
                if is_single_line { return vec![]; }
                ctx.offense_with_range(COP_NAME, MSG_COMPACT, Severity::Convention, start, end)
            }
            // expanded: bad if single-line
            EnforcedStyle::Expanded => {
                if !is_single_line { return vec![]; }
                ctx.offense_with_range(COP_NAME, MSG_EXPANDED, Severity::Convention, start, end)
            }
        };
        vec![offense]
    }
}

fn region_contains_comment(source: &str, start: usize, end: usize) -> bool {
    let start_line = 1 + source.as_bytes()[..start].iter().filter(|&&b| b == b'\n').count();
    let end_line = 1 + source.as_bytes()[..end].iter().filter(|&&b| b == b'\n').count();
    for line_num in start_line..=end_line {
        let line_offset = source::line_byte_offset(source, line_num);
        let line_end = source[line_offset..].find('\n').map(|p| line_offset + p).unwrap_or(source.len());
        let line = &source[line_offset..line_end];
        if source::find_comment_start(line).is_some() { return true; }
    }
    false
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/EmptyMethod", |cfg| {
    let c: Cfg = cfg.typed("Style/EmptyMethod");
    let style = match c.enforced_style.as_str() {
        "expanded" => EnforcedStyle::Expanded,
        _ => EnforcedStyle::Compact,
    };
    Some(Box::new(EmptyMethod::with_style(style)))
});
