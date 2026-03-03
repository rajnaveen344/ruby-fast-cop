//! Style/NumericLiterals - Checks for large numeric literals without underscores.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/numeric_literals.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;

pub struct NumericLiterals {
    min_digits: usize,
    strict: bool,
    allowed_numbers: Vec<i64>,
    allowed_patterns: Vec<String>,
}

impl NumericLiterals {
    pub fn new(min_digits: usize) -> Self {
        Self {
            min_digits,
            strict: false,
            allowed_numbers: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }

    pub fn with_config(
        min_digits: usize,
        strict: bool,
        allowed_numbers: Vec<i64>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self {
            min_digits,
            strict,
            allowed_numbers,
            allowed_patterns,
        }
    }

    /// Check if a numeric literal source needs underscores.
    /// Returns true if the number violates the rule.
    fn check_integer_source(&self, source: &str) -> bool {
        // Strip leading minus/plus and whitespace
        let s = source.trim().trim_start_matches('-').trim_start_matches('+').trim();

        // Skip non-decimal literals (0x, 0b, 0o, 0...)
        if s.starts_with("0x")
            || s.starts_with("0X")
            || s.starts_with("0b")
            || s.starts_with("0B")
            || s.starts_with("0o")
            || s.starts_with("0O")
        {
            return false;
        }
        // Octal literals starting with 0 followed by digits
        if s.starts_with('0') && s.len() > 1 && s.chars().nth(1).map_or(false, |c| c.is_ascii_digit()) {
            return false;
        }

        // Split at decimal point and/or exponent
        let (integer_part, _) = self.split_numeric(s);

        // Count digits (excluding underscores)
        let digit_count: usize = integer_part.chars().filter(|c| c.is_ascii_digit()).count();

        if digit_count < self.min_digits {
            return false;
        }

        // Check AllowedNumbers
        if let Ok(num) = integer_part.replace('_', "").parse::<i64>() {
            if self.allowed_numbers.contains(&num) {
                return false;
            }
        }

        // Check AllowedPatterns against the integer part (including underscores)
        for pattern in &self.allowed_patterns {
            let pat = pattern.trim_matches('/');
            if let Ok(re) = Regex::new(&format!("^{}$", pat)) {
                if re.is_match(integer_part) {
                    return false;
                }
            }
        }

        // Check if underscores are correctly placed
        if integer_part.contains('_') {
            // Has underscores - check if they're in the right places
            if self.strict {
                // Strict mode: must be exactly every 3 digits from right
                return !Self::has_correct_underscores(integer_part);
            }
            // Non-strict: allow groups of 3 or less at the end
            return !Self::has_acceptable_underscores(integer_part);
        }

        // No underscores and enough digits - offense
        true
    }

    /// Split a numeric string into integer part and rest (decimal/exponent).
    fn split_numeric<'a>(&self, s: &'a str) -> (&'a str, &'a str) {
        // Find decimal point
        if let Some(dot_pos) = s.find('.') {
            return (&s[..dot_pos], &s[dot_pos..]);
        }
        // Find exponent
        if let Some(e_pos) = s.find(|c: char| c == 'e' || c == 'E') {
            return (&s[..e_pos], &s[e_pos..]);
        }
        (s, "")
    }

    /// Check if underscores are in correct positions (every 3 digits from right)
    fn has_correct_underscores(integer_part: &str) -> bool {
        let parts: Vec<&str> = integer_part.split('_').collect();
        if parts.is_empty() {
            return true;
        }

        // First group can be 1-3 digits
        if parts[0].is_empty() || parts[0].len() > 3 {
            return false;
        }

        // All subsequent groups must be exactly 3 digits
        for part in &parts[1..] {
            if part.len() != 3 {
                return false;
            }
        }

        true
    }

    /// Check if underscores are in acceptable positions (non-strict mode).
    /// Allows groups where the last group can be 1-3 digits.
    fn has_acceptable_underscores(integer_part: &str) -> bool {
        let parts: Vec<&str> = integer_part.split('_').collect();
        if parts.is_empty() {
            return true;
        }

        // First group can be 1-3 digits
        if parts[0].is_empty() || parts[0].len() > 3 {
            return false;
        }

        // Middle groups must be exactly 3 digits
        if parts.len() > 2 {
            for part in &parts[1..parts.len() - 1] {
                if part.len() != 3 {
                    return false;
                }
            }
        }

        // Last group can be 1-3 digits (non-strict allows this)
        if let Some(last) = parts.last() {
            if last.is_empty() || last.len() > 3 {
                return false;
            }
        }

        true
    }
}

impl Cop for NumericLiterals {
    fn name(&self) -> &'static str {
        "Style/NumericLiterals"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = NumericVisitor {
            cop: self,
            ctx,
            offenses: &mut offenses,
        };
        visitor.visit(&result.node());
        offenses
    }
}

/// Scan backwards from a node's start_offset over whitespace/newlines looking for a
/// unary minus sign. If found and preceded by an assignment/expression-start character,
/// return the byte offset of the `-`. This handles cases like `a = -\n  12345` where
/// Prism's IntegerNode only covers `12345` but RuboCop expects the offense at `-`.
fn find_unary_minus_offset(source: &str, node_start: usize) -> Option<usize> {
    if node_start == 0 {
        return None;
    }

    let bytes = source.as_bytes();
    let mut pos = node_start;

    // Scan backwards over whitespace/newlines
    while pos > 0 {
        pos -= 1;
        match bytes[pos] {
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b'-' => {
                // Found a minus — check it's a unary minus (preceded by operator/delimiter)
                if pos == 0 {
                    return Some(pos);
                }
                // Scan backwards over more whitespace to find preceding character
                let mut check = pos;
                while check > 0 {
                    check -= 1;
                    match bytes[check] {
                        b' ' | b'\t' => continue,
                        // Assignment or expression-start characters
                        b'=' | b'(' | b',' | b'[' | b':' | b';' | b'|' | b'!' | b'>'
                        | b'<' | b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'&'
                        | b'~' | b'?' | b'\n' => return Some(pos),
                        _ => return None,
                    }
                }
                // At start of file — also a valid unary minus position
                return Some(pos);
            }
            _ => return None,
        }
    }

    None
}

struct NumericVisitor<'a> {
    cop: &'a NumericLiterals,
    ctx: &'a CheckContext<'a>,
    offenses: &'a mut Vec<Offense>,
}

impl Visit<'_> for NumericVisitor<'_> {
    fn visit_integer_node(&mut self, node: &ruby_prism::IntegerNode) {
        let loc = node.location();
        let node_source = &self.ctx.source[loc.start_offset()..loc.end_offset()];

        // Check if there's a unary minus on a preceding line
        let (check_source, offense_start, offense_end) =
            if let Some(minus_offset) = find_unary_minus_offset(self.ctx.source, loc.start_offset()) {
                // Include the minus sign in the source we check
                let full_source = &self.ctx.source[minus_offset..loc.end_offset()];
                (full_source, minus_offset, minus_offset + 1)
            } else {
                (node_source, loc.start_offset(), loc.end_offset())
            };

        // Check for rubocop:disable comment on the same line
        let start_loc = Location::from_offsets(self.ctx.source, offense_start, offense_end);
        let line_idx = (start_loc.line - 1) as usize;
        if let Some(line) = self.ctx.source.lines().nth(line_idx) {
            if line.contains("rubocop:disable") && line.contains("Style/NumericLiterals") {
                return;
            }
        }

        if self.cop.check_integer_source(check_source) {
            let location = if offense_start != loc.start_offset() {
                // Report offense at the minus sign position
                Location::from_offsets(self.ctx.source, offense_start, offense_end)
            } else {
                self.ctx.location(&loc)
            };
            self.offenses.push(Offense::new(
                self.cop.name(),
                "Use underscores(_) as thousands separator and separate every 3 digits with them.",
                self.cop.severity(),
                location,
                self.ctx.filename,
            ));
        }

        ruby_prism::visit_integer_node(self, node);
    }

    fn visit_float_node(&mut self, node: &ruby_prism::FloatNode) {
        let loc = node.location();
        let node_source = &self.ctx.source[loc.start_offset()..loc.end_offset()];

        // Check if there's a unary minus on a preceding line
        let (check_source, offense_start, offense_end) =
            if let Some(minus_offset) = find_unary_minus_offset(self.ctx.source, loc.start_offset()) {
                let full_source = &self.ctx.source[minus_offset..loc.end_offset()];
                (full_source, minus_offset, minus_offset + 1)
            } else {
                (node_source, loc.start_offset(), loc.end_offset())
            };

        if self.cop.check_integer_source(check_source) {
            let location = if offense_start != loc.start_offset() {
                Location::from_offsets(self.ctx.source, offense_start, offense_end)
            } else {
                self.ctx.location(&loc)
            };
            self.offenses.push(Offense::new(
                self.cop.name(),
                "Use underscores(_) as thousands separator and separate every 3 digits with them.",
                self.cop.severity(),
                location,
                self.ctx.filename,
            ));
        }

        ruby_prism::visit_float_node(self, node);
    }
}
