//! Style/EmptyMethod - Checks for formatting of empty method definitions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/empty_method.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Style/EmptyMethod";
const MSG_COMPACT: &str = "Put empty method definitions on a single line.";
const MSG_EXPANDED: &str = "Put the `end` of empty method definitions on the next line.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle { Compact, Expanded }

pub struct EmptyMethod {
    style: EnforcedStyle,
    max_line_length: Option<usize>,
}

impl Default for EmptyMethod {
    fn default() -> Self { Self { style: EnforcedStyle::Compact, max_line_length: None } }
}

impl EmptyMethod {
    pub fn new() -> Self { Self::default() }
    pub fn with_style(style: EnforcedStyle) -> Self { Self { style, max_line_length: None } }
    pub fn with_config(style: EnforcedStyle, max_line_length: Option<usize>) -> Self {
        Self { style, max_line_length }
    }
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
                let correction = build_compact_correction(node, ctx.source, start, end, self.max_line_length);
                let off = ctx.offense_with_range(COP_NAME, MSG_COMPACT, Severity::Convention, start, end);
                if let Some(corr) = correction { off.with_correction(corr) } else { off }
            }
            // expanded: bad if single-line
            EnforcedStyle::Expanded => {
                if !is_single_line { return vec![]; }
                let correction = build_expanded_correction(node, ctx.source, start, end);
                let off = ctx.offense_with_range(COP_NAME, MSG_EXPANDED, Severity::Convention, start, end);
                if let Some(corr) = correction { off.with_correction(corr) } else { off }
            }
        };
        vec![offense]
    }
}

/// Build correction for compact style: collapse multiline def to single line.
/// Returns None if correction would exceed max_line_length.
fn build_compact_correction(
    node: &ruby_prism::DefNode,
    source: &str,
    start: usize,
    end: usize,
    max_line_length: Option<usize>,
) -> Option<Correction> {
    // Signature ends at rparen, last parameter, or method name
    let sig_end = if let Some(rp) = node.rparen_loc() {
        rp.end_offset()
    } else if let Some(params) = node.parameters() {
        params.location().end_offset()
    } else {
        node.name_loc().end_offset()
    };
    // Collapse sig (may be multiline if paren on next line)
    let sig_raw = &source[start..sig_end];
    // Collapse internal newlines + surrounding whitespace
    let sig_collapsed: String = sig_raw.split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("");
    let corrected = format!("{}; end", sig_collapsed);
    // Check line length
    if let Some(max) = max_line_length {
        let indent = leading_spaces(source, start);
        if indent + corrected.len() > max {
            return None;
        }
    }
    Some(Correction::replace(start, end, corrected))
}

/// Build correction for expanded style: split `def sig; end` into multiline.
fn build_expanded_correction(
    node: &ruby_prism::DefNode,
    source: &str,
    start: usize,
    _end: usize,
) -> Option<Correction> {
    let end_kw_loc = node.end_keyword_loc()?;
    let end_kw_start = end_kw_loc.start_offset();
    // Find last `;` before `end`
    let before_end = &source[start..end_kw_start];
    let semi_pos = before_end.rfind(';')?;
    let semi_abs = start + semi_pos;
    // Compute indentation of the def keyword
    let indent = " ".repeat(leading_spaces(source, start));
    let replacement = format!("\n{}end", indent);
    Some(Correction::replace(semi_abs, end_kw_loc.end_offset(), replacement))
}

fn leading_spaces(source: &str, byte_offset: usize) -> usize {
    let line_start = source[..byte_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line = &source[line_start..byte_offset];
    line.len() - line.trim_start().len()
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
    let max_line_length = if cfg.is_cop_enabled("Layout/LineLength") {
        cfg.get_cop_config("Layout/LineLength").and_then(|c| c.max).map(|m| m as usize)
    } else { None };
    Some(Box::new(EmptyMethod::with_config(style, max_line_length)))
});
