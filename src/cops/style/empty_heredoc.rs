//! Style/EmptyHeredoc cop
//!
//! `<<~EOS\nEOS\n` → `''`. Empty heredoc body → string literal.

use crate::cops::{CheckContext, Cop};
use crate::config::Config;
use crate::offense::{Correction, Edit, Offense, Severity};

const MSG: &str = "Use an empty string literal instead of heredoc.";

pub struct EmptyHeredoc {
    double_quotes: bool,
}

impl EmptyHeredoc {
    pub fn new() -> Self {
        Self { double_quotes: false }
    }

    pub fn with_config(double_quotes: bool) -> Self {
        Self { double_quotes }
    }

    fn preferred(&self) -> &'static str {
        if self.double_quotes {
            "\"\""
        } else {
            "''"
        }
    }
}

fn is_heredoc_opening(source: &str, open_start: usize, open_end: usize) -> bool {
    let s = &source[open_start..open_end];
    s.starts_with("<<")
}

fn line_end(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = offset;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    if i < bytes.len() {
        i + 1 // include newline
    } else {
        i
    }
}

impl Cop for EmptyHeredoc {
    fn name(&self) -> &'static str {
        "Style/EmptyHeredoc"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_string(
        &self,
        node: &ruby_prism::StringNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let (open, close) = match (node.opening_loc(), node.closing_loc()) {
            (Some(o), Some(c)) => (o, c),
            _ => return vec![],
        };
        let open_start = open.start_offset();
        let open_end = open.end_offset();
        if !is_heredoc_opening(ctx.source, open_start, open_end) {
            return vec![];
        }
        // Body empty? Use content_loc (start..end) of content.
        // Heredoc body starts after the line containing `<<~EOS`, ends at closing start.
        let body_start = line_end(ctx.source, open_end);
        let body_end = close.start_offset();
        if body_start > body_end {
            return vec![];
        }
        let body = &ctx.source[body_start..body_end];
        if !body.is_empty() {
            return vec![];
        }

        // Offense range: the opening only.
        let start = open_start;
        let end = open_end;

        // Correction:
        //   1. replace opening (<<~EOS) with '' (or "")
        //   2. delete the heredoc body + closing line
        let preferred = self.preferred();
        // Closing line range = from body_start to end of closing line.
        let closing_line_end = line_end(ctx.source, close.end_offset().saturating_sub(1));
        let edits = vec![
            Edit {
                start_offset: start,
                end_offset: end,
                replacement: preferred.to_string(),
            },
            Edit {
                start_offset: body_start,
                end_offset: closing_line_end,
                replacement: String::new(),
            },
        ];
        vec![ctx
            .offense_with_range(self.name(), MSG, self.severity(), start, end)
            .with_correction(Correction { edits })]
    }
}

fn _config_double_quotes(cfg: &Config) -> bool {
    if let Some(sl) = cfg.get_cop_config("Style/StringLiterals") {
        if let Some(style) = &sl.enforced_style {
            return style == "double_quotes";
        }
    }
    false
}

crate::register_cop!("Style/EmptyHeredoc", |cfg| {
    let double = _config_double_quotes(cfg);
    Some(Box::new(EmptyHeredoc::with_config(double)))
});
