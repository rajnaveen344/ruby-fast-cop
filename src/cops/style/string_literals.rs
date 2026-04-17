//! Style/StringLiterals cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    SingleQuotes,
    DoubleQuotes,
}

pub struct StringLiterals {
    enforced_style: EnforcedStyle,
    consistent_quotes_in_multiline: bool,
}

impl StringLiterals {
    pub fn new(style: EnforcedStyle) -> Self {
        Self {
            enforced_style: style,
            consistent_quotes_in_multiline: false,
        }
    }

    pub fn with_config(style: EnforcedStyle, consistent_quotes_in_multiline: bool) -> Self {
        Self {
            enforced_style: style,
            consistent_quotes_in_multiline,
        }
    }

    fn needs_double_quotes(source_text: &str) -> bool {
        if source_text.len() < 2 { return false; }
        let inner = &source_text[1..source_text.len() - 1];
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '#' && i + 1 < chars.len() && matches!(chars[i + 1], '{' | '@' | '$') {
                return true;
            }
            if chars[i] == '\\' && i + 1 < chars.len() {
                match chars[i + 1] {
                    'n' | 't' | 'r' | 'a' | 'b' | 'e' | 'f' | 's' | 'v' | '0' | 'x' | 'u' | 'c'
                    | 'C' | 'M' | '\n' => return true,
                    '\\' | '\'' | '"' => {}
                    _ => return true,
                }
                i += 2;
                continue;
            }
            if chars[i] == '\'' { return true; }
            i += 1;
        }
        false
    }

    fn to_single_quoted(source_text: &str) -> String {
        let inner = &source_text[1..source_text.len() - 1];
        let mut result = String::with_capacity(source_text.len());
        result.push('\'');
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                match chars[i + 1] {
                    '"' => {
                        result.push('"');
                        i += 2;
                        continue;
                    }
                    '\\' => {
                        result.push('\\');
                        result.push('\\');
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        result.push('\'');
        result
    }

    fn to_double_quoted(source_text: &str) -> String {
        let inner = &source_text[1..source_text.len() - 1];
        let mut result = String::with_capacity(source_text.len());
        result.push('"');
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                match chars[i + 1] {
                    '\'' => {
                        result.push('\'');
                        i += 2;
                        continue;
                    }
                    '\\' => {
                        result.push('\\');
                        result.push('\\');
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        result.push('"');
        result
    }

    fn needs_single_quotes(source_text: &str) -> bool {
        if source_text.len() < 2 { return false; }
        let inner = &source_text[1..source_text.len() - 1];
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '"' { return true; }
            if chars[i] == '#' && i + 1 < chars.len() && matches!(chars[i + 1], '{' | '@' | '$') {
                return true;
            }
            if chars[i] == '\\' && i + 1 < chars.len() {
                match chars[i + 1] {
                    'n' | 't' | 'r' | 'a' | 'b' | 'e' | 'f' | 's' | 'v' | 'x' | 'u' | 'c' | 'C'
                    | 'M' | '0' => return true,
                    '\'' | '\\' => {}
                    _ => return true,
                }
                i += 2;
                continue;
            }
            i += 1;
        }
        false
    }
}

impl Cop for StringLiterals {
    fn name(&self) -> &'static str {
        "Style/StringLiterals"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();

        // Source-based detection for continuation lines that Prism merges.
        // When `"#{x}" \ "y"` is written, Prism merges it into a single InterpolatedStringNode,
        // so "y" is never visited as a separate StringNode. We detect such cases here.
        if !self.consistent_quotes_in_multiline {
            let lines: Vec<&str> = ctx.source.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                if lines[i].trim_end().ends_with('\\') {
                    // Collect the continuation group
                    let start = i;
                    let mut end = i;
                    while end < lines.len() && lines[end].trim_end().ends_with('\\') {
                        end += 1;
                    }
                    if end < lines.len() {
                        end += 1; // include the last line
                    }

                    if end > start + 1 {
                        // Check if any line in the group has interpolation (needs double quotes)
                        let group = &lines[start..end];
                        let has_interpolation = group.iter().any(|line| {
                            let trimmed = line.trim().trim_end_matches('\\').trim();
                            trimmed.starts_with('"') && Self::needs_double_quotes(trimmed)
                        });

                        // Only run source scan for groups Prism would merge (those with interpolation)
                        if has_interpolation {
                            for j in start..end {
                                let trimmed = lines[j].trim().trim_end_matches('\\').trim();
                                if trimmed.starts_with('"') && !Self::needs_double_quotes(trimmed) {
                                    if self.enforced_style == EnforcedStyle::SingleQuotes {
                                        // Find the column of this string in the original line
                                        let col = lines[j].find('"').unwrap_or(0) as u32;
                                        let line_num = (j + 1) as u32;
                                        let str_len = trimmed.chars().count() as u32;
                                        offenses.push(Offense::new(
                                            self.name(),
                                            "Prefer single-quoted strings when you don't need string interpolation or special symbols.",
                                            self.severity(),
                                            Location::new(line_num, col, line_num, col + str_len),
                                            ctx.filename,
                                        ));
                                    }
                                }
                            }
                        }

                        i = end;
                        continue;
                    }
                }
                i += 1;
            }
        }

        // For ConsistentQuotesInMultiline, track continued strings
        if self.consistent_quotes_in_multiline {
            // Check for continued strings (lines ending with \)
            let lines: Vec<&str> = ctx.source.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                if lines[i].trim_end().ends_with('\\') {
                    // This is a continued string line - collect all continued lines
                    let start = i;
                    let mut end = i;
                    while end < lines.len() && lines[end].trim_end().ends_with('\\') {
                        end += 1;
                    }
                    // end is now the last line of the continuation
                    if end < lines.len() {
                        end += 1; // include the last line
                    }

                    if end > start + 1 {
                        // Multi-line continuation - check consistency
                        let continued_lines = &lines[start..end];
                        let mut has_single = false;
                        let mut has_double = false;
                        let mut has_needed_double = false;
                        let mut has_needed_single = false;

                        for line in continued_lines {
                            let trimmed = line.trim().trim_end_matches('\\').trim();
                            if trimmed.starts_with('\'') {
                                has_single = true;
                                if Self::needs_single_quotes(trimmed) {
                                    has_needed_single = true;
                                }
                            } else if trimmed.starts_with('"') {
                                has_double = true;
                                if Self::needs_double_quotes(trimmed) {
                                    has_needed_double = true;
                                }
                            }
                        }

                        if has_single && has_double {
                            // Mixed quotes - check if mixing is necessary
                            if !has_needed_double && !has_needed_single {
                                // All quotes can be changed - report inconsistency
                                let line_num = (start + 1) as u32;
                                let first_line_len = continued_lines[0].chars().count();
                                offenses.push(Offense::new(
                                    self.name(),
                                    "Inconsistent quote style.",
                                    self.severity(),
                                    Location::new(line_num, 0, line_num, first_line_len as u32),
                                    ctx.filename,
                                ));
                                i = end;
                                continue;
                            } else if has_needed_double || has_needed_single {
                                // Some need their quotes - mixing is OK
                                i = end;
                                continue;
                            }
                        }

                        // Check each line for wrong quote style
                        let all_same_wrong = if has_single
                            && !has_double
                            && self.enforced_style == EnforcedStyle::DoubleQuotes
                        {
                            !has_needed_single
                        } else if has_double
                            && !has_single
                            && self.enforced_style == EnforcedStyle::SingleQuotes
                        {
                            !has_needed_double
                        } else {
                            false
                        };

                        if all_same_wrong {
                            let line_num = (start + 1) as u32;
                            let first_line_len = continued_lines[0].chars().count();
                            let msg = match self.enforced_style {
                                EnforcedStyle::SingleQuotes => {
                                    "Prefer single-quoted strings when you don't need string interpolation or special symbols."
                                }
                                EnforcedStyle::DoubleQuotes => {
                                    "Prefer double-quoted strings unless you need single quotes to avoid extra backslashes for escaping."
                                }
                            };
                            offenses.push(Offense::new(
                                self.name(),
                                msg,
                                self.severity(),
                                Location::new(line_num, 0, line_num, first_line_len as u32),
                                ctx.filename,
                            ));
                        }

                        i = end;
                        continue;
                    }
                }
                i += 1;
            }

            // Also check multi-line strings (strings spanning multiple lines)
            // These are handled by the string node visitor below
        }

        // Visit string nodes using Prism AST
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = StringLiteralsVisitor {
            cop: self,
            ctx,
            offenses: &mut offenses,
            inside_interpolation: false,
            inside_word_array: false,
        };
        visitor.visit(&result.node());

        offenses
    }
}

struct StringLiteralsVisitor<'a> {
    cop: &'a StringLiterals,
    ctx: &'a CheckContext<'a>,
    offenses: &'a mut Vec<Offense>,
    inside_interpolation: bool,
    inside_word_array: bool,
}

impl StringLiteralsVisitor<'_> {
    fn check_string_node(&mut self, node: &ruby_prism::StringNode) {
        if self.inside_interpolation || self.inside_word_array { return; }

        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];

        if source_text.starts_with("<<") || source_text.starts_with("%q")
            || source_text.starts_with("%Q") || source_text.starts_with("%(")
            || source_text.starts_with("?") || source_text == "__FILE__"
        { return; }

        let is_single_quoted = source_text.starts_with('\'');
        let is_double_quoted = source_text.starts_with('"');

        if !is_single_quoted && !is_double_quoted { return; }

        if self.cop.consistent_quotes_in_multiline {
            let start_loc = self.ctx.location(&loc);
            let end_loc_line = {
                let end_offset = loc.end_offset();
                let mut line = 1u32;
                for (i, ch) in self.ctx.source.char_indices() {
                    if i >= end_offset {
                        break;
                    }
                    if ch == '\n' {
                        line += 1;
                    }
                }
                line
            };

            // Multi-line string
            if end_loc_line > start_loc.line {
                if is_double_quoted && self.cop.enforced_style == EnforcedStyle::SingleQuotes {
                    if !StringLiterals::needs_double_quotes(source_text) {
                        self.offenses.push(Offense::new(
                            self.cop.name(),
                            "Prefer single-quoted strings when you don't need string interpolation or special symbols.",
                            self.cop.severity(),
                            Location::new(start_loc.line, start_loc.column, start_loc.line, start_loc.column + source_text.lines().next().unwrap_or("").chars().count() as u32),
                            self.ctx.filename,
                        ));
                    }
                }
                return;
            }

            // Check if this string is part of a continuation
            let line_idx = (start_loc.line - 1) as usize;
            let lines: Vec<&str> = self.ctx.source.lines().collect();
            if line_idx > 0 {
                if let Some(prev_line) = lines.get(line_idx - 1) {
                    if prev_line.trim_end().ends_with('\\') {
                        return;
                    }
                }
            }
            if let Some(curr_line) = lines.get(line_idx) {
                if curr_line.trim_end().ends_with('\\') {
                    return;
                }
            }
        }

        match self.cop.enforced_style {
            EnforcedStyle::SingleQuotes => {
                if is_double_quoted && !StringLiterals::needs_double_quotes(source_text) {
                    let location = self.ctx.location(&loc);
                    let corrected = StringLiterals::to_single_quoted(source_text);
                    let correction = Correction::replace(
                        loc.start_offset(),
                        loc.end_offset(),
                        corrected,
                    );
                    self.offenses.push(Offense::new(
                        self.cop.name(),
                        "Prefer single-quoted strings when you don't need string interpolation or special symbols.",
                        self.cop.severity(),
                        location,
                        self.ctx.filename,
                    ).with_correction(correction));
                }
            }
            EnforcedStyle::DoubleQuotes => {
                if is_single_quoted && !StringLiterals::needs_single_quotes(source_text) {
                    let location = self.ctx.location(&loc);
                    let corrected = StringLiterals::to_double_quoted(source_text);
                    let correction = Correction::replace(
                        loc.start_offset(),
                        loc.end_offset(),
                        corrected,
                    );
                    self.offenses.push(Offense::new(
                        self.cop.name(),
                        "Prefer double-quoted strings unless you need single quotes to avoid extra backslashes for escaping.",
                        self.cop.severity(),
                        location,
                        self.ctx.filename,
                    ).with_correction(correction));
                }
            }
        }
    }
}

impl Visit<'_> for StringLiteralsVisitor<'_> {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        self.check_string_node(node);
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let was = self.inside_interpolation;
        self.inside_interpolation = true;
        ruby_prism::visit_interpolated_string_node(self, node);
        self.inside_interpolation = was;
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let loc = node.location();
        let source_text = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        if source_text.starts_with("%w")
            || source_text.starts_with("%W")
            || source_text.starts_with("%i")
            || source_text.starts_with("%I")
        {
            let was = self.inside_word_array;
            self.inside_word_array = true;
            ruby_prism::visit_array_node(self, node);
            self.inside_word_array = was;
        } else {
            ruby_prism::visit_array_node(self, node);
        }
    }
}

crate::register_cop!("Style/StringLiterals", |cfg| {
    let cop_config = cfg.get_cop_config("Style/StringLiterals");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .and_then(|s| match s.as_str() {
            "single_quotes" => Some(EnforcedStyle::SingleQuotes),
            "double_quotes" => Some(EnforcedStyle::DoubleQuotes),
            _ => None,
        });
    let style = match style {
        Some(s) => s,
        None => {
            if cop_config.and_then(|c| c.enforced_style.as_ref()).is_some() {
                return None;
            }
            EnforcedStyle::SingleQuotes
        }
    };
    let consistent = cop_config
        .and_then(|c| c.raw.get("ConsistentQuotesInMultiline"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(Box::new(StringLiterals::with_config(style, consistent)))
});
