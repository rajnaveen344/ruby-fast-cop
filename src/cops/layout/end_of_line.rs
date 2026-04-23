//! Layout/EndOfLine - Checks for incorrect line endings.
//!
//! Only one offense is reported (first violation found).
//! Stops checking past __END__ (no tokens beyond that in RuboCop).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/end_of_line.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

#[derive(Clone, Copy, PartialEq)]
pub enum EolStyle {
    Native,
    Lf,
    Crlf,
}

pub struct EndOfLine {
    style: EolStyle,
}

impl EndOfLine {
    pub fn new(style: EolStyle) -> Self {
        Self { style }
    }
}

impl Default for EndOfLine {
    fn default() -> Self {
        Self { style: EolStyle::Native }
    }
}

/// Find the byte offset of `__END__` on its own line, if present.
fn end_marker_line(source: &str) -> Option<usize> {
    for (i, line) in source.lines().enumerate() {
        if line.trim_end_matches('\r') == "__END__" {
            return Some(i);
        }
    }
    None
}

impl Cop for EndOfLine {
    fn name(&self) -> &'static str {
        "Layout/EndOfLine"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // native = lf on non-Windows (we assume non-Windows)
        let effective = match self.style {
            EolStyle::Native => EolStyle::Lf,
            other => other,
        };

        let source = ctx.source;
        let stop_at = end_marker_line(source).unwrap_or(usize::MAX);
        let ends_with_nl = source.ends_with('\n');

        // split('\n') segments: if source ends with '\n', the final segment is a
        // trailing empty placeholder — skip it. Otherwise, the final segment is
        // real content without a newline terminator and still needs checking.
        let segments: Vec<&str> = source.split('\n').collect();
        let total = segments.len();
        let real_lines = if ends_with_nl { total - 1 } else { total };

        let mut byte_offset: usize = 0;
        for (i, seg) in segments.iter().enumerate().take(real_lines) {
            if i >= stop_at {
                break;
            }

            let line_start = byte_offset;
            byte_offset += seg.len();
            if i + 1 < total {
                byte_offset += 1; // the \n separator
            }

            let content_len = if seg.ends_with('\r') { seg.len() - 1 } else { seg.len() };

            match effective {
                EolStyle::Lf => {
                    if seg.ends_with('\r') {
                        return vec![Offense::new(
                            self.name(),
                            "Carriage return character detected.",
                            self.severity(),
                            Location::from_offsets(source, line_start, line_start + content_len),
                            ctx.filename,
                        )];
                    }
                }
                EolStyle::Crlf => {
                    // Only flag a missing CR on lines that have a following '\n'.
                    // A final line with no newline terminator at EOF is exempt.
                    let has_terminator = i + 1 < total || ends_with_nl;
                    if has_terminator && !seg.ends_with('\r') {
                        return vec![Offense::new(
                            self.name(),
                            "Carriage return character missing.",
                            self.severity(),
                            Location::from_offsets(source, line_start, line_start + seg.len()),
                            ctx.filename,
                        )];
                    }
                }
                EolStyle::Native => unreachable!(),
            }
        }

        vec![]
    }
}

crate::register_cop!("Layout/EndOfLine", |cfg| {
    let style = cfg
        .get_cop_config("Layout/EndOfLine")
        .and_then(|c| c.enforced_style.as_deref())
        .map(|s| match s {
            "lf" => EolStyle::Lf,
            "crlf" => EolStyle::Crlf,
            _ => EolStyle::Native,
        })
        .unwrap_or(EolStyle::Native);
    Some(Box::new(EndOfLine::new(style)))
});
