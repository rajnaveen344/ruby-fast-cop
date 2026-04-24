//! Style/RedundantHeredocDelimiterQuotes - Checks for redundant quotes in heredoc delimiters.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_heredoc_delimiter_quotes.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Location, Visit};

#[derive(Default)]
pub struct RedundantHeredocDelimiterQuotes;

impl RedundantHeredocDelimiterQuotes {
    pub fn new() -> Self {
        Self
    }
}

/// For a heredoc whose opening source is e.g. `<<~'EOS'`, return
/// (heredoc_type, delimiter_string) = ("<<~", "EOS"), or None if not a
/// quoted heredoc we handle.
fn parse_opening(opening: &str) -> Option<(String, String, char)> {
    if !opening.starts_with("<<") {
        return None;
    }
    // heredoc_type: <<, <<~, <<-
    let after_ltlt = &opening[2..];
    let (rest, hd_type) = if let Some(s) = after_ltlt.strip_prefix('~') {
        (s, "<<~".to_string())
    } else if let Some(s) = after_ltlt.strip_prefix('-') {
        (s, "<<-".to_string())
    } else {
        (after_ltlt, "<<".to_string())
    };
    // Now rest starts with ', ", or ` (quote) or alphanumeric (unquoted)
    let first = rest.chars().next()?;
    if first != '\'' && first != '"' {
        // Not a quoted delimiter we handle. Skip backtick too (not in scope).
        return None;
    }
    // Find closing same quote
    let rest_after_q = &rest[1..];
    let end = rest_after_q.find(first)?;
    let delim = &rest_after_q[..end];
    Some((hd_type, delim.to_string(), first))
}

/// Find the body and end-line source ranges for a heredoc whose opening
/// token lives at `source[opening_start..opening_end]` with delimiter `delim`.
/// Returns (body_start, body_end_before_end_line, end_line_text).
/// RuboCop uses node.loc.heredoc_body and node.loc.heredoc_end — Prism
/// doesn't expose these, so we scan manually.
fn find_heredoc_body_and_end<'a>(
    source: &'a str,
    opening_end: usize,
    delim: &str,
    hd_type: &str,
) -> Option<(usize, usize, &'a str)> {
    // Heredoc body begins on the line FOLLOWING the line containing opening.
    // Find next newline after opening_end, then body starts there.
    let bytes = source.as_bytes();
    let mut i = opening_end;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let body_start = i + 1; // after \n

    // Scan line by line for closing. For <<~ or <<-, closing may be indented.
    // For <<, closing must be at col 0.
    let allow_indent = hd_type == "<<~" || hd_type == "<<-";
    let mut line_start = body_start;
    while line_start < source.len() {
        let line_end = source[line_start..]
            .find('\n')
            .map(|p| line_start + p)
            .unwrap_or(source.len());
        let line = &source[line_start..line_end];
        let trimmed = if allow_indent { line.trim_start() } else { line };
        if trimmed == delim {
            return Some((body_start, line_start, line));
        }
        if line_end == source.len() {
            break;
        }
        line_start = line_end + 1;
    }
    None
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    visited_opening_starts: std::collections::HashSet<usize>,
}

impl<'a> Visitor<'a> {
    fn handle(&mut self, opening_loc: &Location, is_interp: bool) {
        let opening_start = opening_loc.start_offset();
        if !self.visited_opening_starts.insert(opening_start) {
            return;
        }
        let opening_end = opening_loc.end_offset();
        let src = self.ctx.source;
        if opening_end > src.len() || opening_start >= opening_end {
            return;
        }
        let opening_src = &src[opening_start..opening_end];
        if !opening_src.starts_with("<<") {
            return;
        }
        let (hd_type, delim, _quote) = match parse_opening(opening_src) {
            Some(v) => v,
            None => return,
        };

        // Empty delimiter -> skip (not a heredoc we flag)
        if delim.is_empty() {
            return;
        }

        // Find body + end line
        let (body_start, body_end, end_line) = match find_heredoc_body_and_end(src, opening_end, &delim, &hd_type) {
            Some(v) => v,
            None => return,
        };

        // Need heredoc delimiter quotes? Conditions from RuboCop:
        // 1. Any non-word char in the trimmed end line (the delim itself checked): but the check is on `loc.heredoc_end.source.strip`. Since we derived delim from opening and loc end == delim, this check on delim matches \W in delim chars (EDGE'CASE has quote, EDGE"CASE etc.). If delim matches /\W/ -> need quotes.
        // 2. body contains `#{`, `#@`, `#$`, OR `\`.
        if delim.chars().any(|c| !c.is_alphanumeric() && c != '_') {
            return;
        }

        let body = &src[body_start..body_end];
        // Check for interpolation/escape patterns.
        let mut idx = 0;
        let bytes = body.as_bytes();
        while idx < bytes.len() {
            let b = bytes[idx];
            if b == b'#' && idx + 1 < bytes.len() {
                let next = bytes[idx + 1];
                if next == b'{' || next == b'@' || next == b'$' {
                    return;
                }
            }
            if b == b'\\' {
                return;
            }
            idx += 1;
        }

        let _ = is_interp;
        let _ = end_line;

        // Offense range = opening_loc. Replacement: hd_type + delim.
        let replacement = format!("{}{}", hd_type, delim);
        let msg = format!(
            "Remove the redundant heredoc delimiter quotes, use `{}` instead.",
            replacement
        );
        self.offenses.push(
            self.ctx
                .offense_with_range(
                    "Style/RedundantHeredocDelimiterQuotes",
                    &msg,
                    Severity::Convention,
                    opening_start,
                    opening_end,
                )
                .with_correction(Correction::replace(opening_start, opening_end, replacement)),
        );
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        if let Some(opening) = node.opening_loc() {
            self.handle(&opening, false);
        }
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if let Some(opening) = node.opening_loc() {
            self.handle(&opening, true);
        }
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

impl Cop for RedundantHeredocDelimiterQuotes {
    fn name(&self) -> &'static str {
        "Style/RedundantHeredocDelimiterQuotes"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            visited_opening_starts: std::collections::HashSet::new(),
        };
        v.visit(&result.node());
        v.offenses
    }
}

crate::register_cop!("Style/RedundantHeredocDelimiterQuotes", |_cfg| Some(Box::new(RedundantHeredocDelimiterQuotes::new())));
