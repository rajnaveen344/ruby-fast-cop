//! Layout/LineContinuationLeadingSpace - Strings broken over multiple lines (by
//! a backslash) should contain trailing spaces (default) or leading spaces.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/line_continuation_leading_space.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineContinuationLeadingSpaceStyle {
    Trailing,
    Leading,
}

pub struct LineContinuationLeadingSpace {
    style: LineContinuationLeadingSpaceStyle,
}

impl Default for LineContinuationLeadingSpace {
    fn default() -> Self {
        Self { style: LineContinuationLeadingSpaceStyle::Trailing }
    }
}

impl LineContinuationLeadingSpace {
    pub fn new(style: LineContinuationLeadingSpaceStyle) -> Self {
        Self { style }
    }
}

struct Visitor<'a> {
    source: &'a str,
    style: LineContinuationLeadingSpaceStyle,
    cop_name: &'static str,
    filename: &'a str,
    severity: Severity,
    offenses: Vec<Offense>,
    /// Line indices already inspected (avoid duplicate offenses from nested dstrs).
    handled_lines: Vec<usize>,
}

impl<'a, 'pr> Visit<'pr> for Visitor<'a> {
    fn visit_interpolated_string_node(
        &mut self,
        node: &ruby_prism::InterpolatedStringNode<'pr>,
    ) {
        self.process_dstr(node);
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn process_dstr(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();

        let node_src = &self.source[start..end];
        if !node_src.contains('\\') {
            return;
        }

        // Determine first-line column (column of the opening quote in source).
        let first_line_start = self.line_start_byte(start);
        let first_line_col = start - first_line_start;

        // Get the raw lines covering this dstr — index by absolute line numbers.
        let first_line_num = self.line_of(start); // 1-indexed
        let last_line_num = self.line_of(end.saturating_sub(1));
        let mut all_lines: Vec<&str> = self.source.lines().collect();
        if all_lines.len() < last_line_num {
            // Defensive
            all_lines.push("");
        }

        // Children for "continuation?" — need to know if any child spans across
        // a given line boundary as multiline (i.e. blocks the line continuation
        // detection because the backslash is inside a multiline string token).
        let children: Vec<ruby_prism::Node> = node.parts().iter().collect();

        // We need raw lines starting from the first line of dstr to last line.
        // Iterate consecutive pairs.
        // Track end-of-first-line absolute byte position.
        let mut end_of_first_line_abs = first_line_start + first_line_col + 0;
        // Actually RuboCop sets `end_of_first_line = node.source_range.begin_pos - node.source_range.column`
        // = beginning of the line (line_start_byte). Then accumulates raw_line_one.length each iter.
        // raw_lines = all source lines covering the dstr (from first_line to last_line).
        let mut end_of_first_line = first_line_start;

        for line_idx in first_line_num..last_line_num {
            // line_idx is 1-indexed first line of the pair; line_idx+1 is second line.
            let raw_line_one = format!(
                "{}\n",
                all_lines.get(line_idx - 1).copied().unwrap_or("")
            );
            let raw_line_two = format!(
                "{}\n",
                all_lines.get(line_idx).copied().unwrap_or("")
            );
            let line_one_len = raw_line_one.len();
            end_of_first_line += line_one_len;

            // continuation?: line1 ends with `\\\n` AND no child crosses this line boundary.
            if !raw_line_one.ends_with("\\\n") {
                continue;
            }
            if children.iter().any(|c| {
                let cl = c.location();
                let c_first = self.line_of(cl.start_offset());
                let c_last = self.line_of(cl.end_offset().saturating_sub(1));
                c_first <= line_idx && c_last > line_idx && c_first != c_last
            }) {
                continue;
            }

            if self.handled_lines.contains(&line_idx) {
                continue;
            }
            self.handled_lines.push(line_idx);
            self.investigate(&raw_line_one, &raw_line_two, end_of_first_line);
        }
    }

    fn investigate(&mut self, line1: &str, line2: &str, end_of_first_line: usize) {
        match self.style {
            LineContinuationLeadingSpaceStyle::Leading => {
                self.investigate_leading(line1, line2, end_of_first_line);
            }
            LineContinuationLeadingSpaceStyle::Trailing => {
                self.investigate_trailing(line1, line2, end_of_first_line);
            }
        }
    }

    /// `LEADING_STYLE_OFFENSE = /(\s+)(['"]\s*\\\n)/`  (trailing spaces in line1)
    fn investigate_leading(&mut self, line1: &str, line2: &str, end_of_first_line: usize) {
        // Find rightmost `'\\\n` or `"\\\n` (with optional ws between quote and `\\`).
        // Then count whitespace before that quote.
        let bytes = line1.as_bytes();
        // find pattern: <quote>\s*\\\n at end. line ends with \\\n.
        if !line1.ends_with("\\\n") {
            return;
        }
        // Position of `\\` — len-2. Walk back over space/tab.
        let mut p = bytes.len() - 2; // position of '\\'
        let mut ending_len = 2; // '\\\n'
        while p > 0 && (bytes[p - 1] == b' ' || bytes[p - 1] == b'\t') {
            p -= 1;
            ending_len += 1;
        }
        if p == 0 {
            return;
        }
        let q = bytes[p - 1];
        if q != b'\'' && q != b'"' {
            return;
        }
        // ending = the matched `'\s*\\\n` portion: length = 1 (quote) + ws + 2 ('\\\n')
        let ending_len = ending_len + 1; // include the quote
        // Now count trailing whitespace BEFORE the quote (inside the string).
        let quote_pos = p - 1;
        let mut ws_end = quote_pos;
        let mut ws_start = ws_end;
        while ws_start > 0 && (bytes[ws_start - 1] == b' ' || bytes[ws_start - 1] == b'\t') {
            ws_start -= 1;
        }
        if ws_start == ws_end {
            return; // no trailing whitespace
        }
        let trailing_len = ws_end - ws_start;

        // Offense range: end_of_first_line - ending_len - trailing_len ..
        //                end_of_first_line - ending_len
        let begin_abs = end_of_first_line - ending_len - trailing_len;
        let end_abs = end_of_first_line - ending_len;

        // Autocorrect: remove offense range, then insert the spaces at the start
        // of the next string (after its opening quote).
        // RuboCop: insert_pos = end_of_first_line - first_line[LINE_1_ENDING].length.
        // LINE_1_ENDING.length here = ending_len (quote+ws+\\\n). That places insert
        // right BEFORE the closing quote of line1. We'll reuse our begin_abs+0 ... wait
        // RuboCop replace at end_of_first_line - first_line[LINE_1_ENDING].length is the
        // position of the quote on line1 (just before closing quote). For Leading style,
        // RuboCop moves the trailing-spaces-on-line1 to the start of line2 (inside the
        // string after its opening quote).
        // So:
        //   - Remove `[ws_start..ws_end]` (the trailing spaces on line1, before closing quote)
        //   - Insert those spaces at the start of line2's string (after opening quote)
        let insert_pos = end_of_first_line + line2_after_quote(line2);
        let spaces: String = self.source[begin_abs..end_abs].to_string();

        let location = Location::from_offsets(self.source, begin_abs, end_abs);
        let edits = vec![
            Edit { start_offset: begin_abs, end_offset: end_abs, replacement: String::new() },
            Edit { start_offset: insert_pos, end_offset: insert_pos, replacement: spaces },
        ];
        let offense = Offense::new(
            self.cop_name,
            "Move trailing spaces to the start of the next line.",
            self.severity,
            location,
            self.filename,
        )
        .with_correction(Correction { edits });
        self.offenses.push(offense);
    }

    /// `TRAILING_STYLE_OFFENSE = /\A\s*(['"])(\s+)/`  (leading spaces in line2)
    fn investigate_trailing(&mut self, line1: &str, line2: &str, end_of_first_line: usize) {
        // line2 starts with optional ws, then quote, then ws (the offense).
        let bytes = line2.as_bytes();
        let mut i = 0;
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if i >= bytes.len() {
            return;
        }
        if bytes[i] != b'\'' && bytes[i] != b'"' {
            return;
        }
        let beginning_len = i + 1; // ws + quote
        // Now count whitespace inside the string after the opening quote.
        let mut ws_start = beginning_len;
        let mut ws_end = ws_start;
        while ws_end < bytes.len() && (bytes[ws_end] == b' ' || bytes[ws_end] == b'\t') {
            ws_end += 1;
        }
        if ws_start == ws_end {
            return;
        }
        let leading_len = ws_end - ws_start;

        let begin_abs = end_of_first_line + beginning_len;
        let end_abs = begin_abs + leading_len;

        // Autocorrect: remove offense range, insert spaces at end of line1's string
        // (i.e. just before the closing quote on line1).
        // insert_pos = end_of_first_line - line1[LINE_1_ENDING].length.
        // LINE_1_ENDING = `'\s*\\\n` — quote + ws + `\\\n`. Find its length on line1.
        let line1_end_len = compute_line1_ending_len(line1);
        let insert_pos = end_of_first_line - line1_end_len;
        let spaces: String = self.source[begin_abs..end_abs].to_string();

        let location = Location::from_offsets(self.source, begin_abs, end_abs);
        let edits = vec![
            Edit { start_offset: begin_abs, end_offset: end_abs, replacement: String::new() },
            Edit { start_offset: insert_pos, end_offset: insert_pos, replacement: spaces },
        ];
        let offense = Offense::new(
            self.cop_name,
            "Move leading spaces to the end of the previous line.",
            self.severity,
            location,
            self.filename,
        )
        .with_correction(Correction { edits });
        self.offenses.push(offense);
    }

    fn line_of(&self, offset: usize) -> usize {
        1 + self.source.as_bytes()[..offset.min(self.source.len())]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
    }

    fn line_start_byte(&self, offset: usize) -> usize {
        self.source[..offset].rfind('\n').map_or(0, |p| p + 1)
    }
}

/// Number of bytes the LINE_1_ENDING regex matches: quote + optional ws + `\\\n`.
fn compute_line1_ending_len(line1: &str) -> usize {
    let bytes = line1.as_bytes();
    if !line1.ends_with("\\\n") {
        return 0;
    }
    let mut p = bytes.len() - 2;
    let mut count = 2;
    while p > 0 && (bytes[p - 1] == b' ' || bytes[p - 1] == b'\t') {
        p -= 1;
        count += 1;
    }
    if p > 0 && (bytes[p - 1] == b'\'' || bytes[p - 1] == b'"') {
        count + 1
    } else {
        0
    }
}

/// Returns the byte offset within `line2` of the position right after the
/// opening quote, i.e. where leading-spaces would be inserted.
fn line2_after_quote(line2: &str) -> usize {
    let bytes = line2.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i < bytes.len() && (bytes[i] == b'\'' || bytes[i] == b'"') {
        i + 1
    } else {
        i
    }
}

impl Cop for LineContinuationLeadingSpace {
    fn name(&self) -> &'static str {
        "Layout/LineContinuationLeadingSpace"
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.source.contains('\\') {
            return vec![];
        }
        let mut v = Visitor {
            source: ctx.source,
            style: self.style,
            cop_name: self.name(),
            filename: ctx.filename,
            severity: self.severity(),
            offenses: Vec::new(),
            handled_lines: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Layout/LineContinuationLeadingSpace", |cfg| {
    let c: Cfg = cfg.typed("Layout/LineContinuationLeadingSpace");
    let style = match c.enforced_style.as_deref() {
        Some("leading") => LineContinuationLeadingSpaceStyle::Leading,
        _ => LineContinuationLeadingSpaceStyle::Trailing,
    };
    Some(Box::new(LineContinuationLeadingSpace::new(style)))
});
