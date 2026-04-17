//! Style/NumericLiterals cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
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

    fn check_integer_source(&self, source: &str) -> bool {
        let s = source.trim().trim_start_matches('-').trim_start_matches('+').trim();
        if s.starts_with("0x") || s.starts_with("0X") || s.starts_with("0b")
            || s.starts_with("0B") || s.starts_with("0o") || s.starts_with("0O")
        { return false; }
        if s.starts_with('0') && s.len() > 1 && s.chars().nth(1).map_or(false, |c| c.is_ascii_digit()) {
            return false;
        }

        let (integer_part, _) = self.split_numeric(s);
        let digit_count: usize = integer_part.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count < self.min_digits { return false; }

        if let Ok(num) = integer_part.replace('_', "").parse::<i64>() {
            if self.allowed_numbers.contains(&num) { return false; }
        }
        for pattern in &self.allowed_patterns {
            if let Ok(re) = Regex::new(&format!("^{}$", pattern.trim_matches('/'))) {
                if re.is_match(integer_part) { return false; }
            }
        }

        if integer_part.contains('_') {
            return if self.strict { !Self::has_correct_underscores(integer_part) }
                   else { !Self::has_acceptable_underscores(integer_part) };
        }
        true
    }

    fn split_numeric<'a>(&self, s: &'a str) -> (&'a str, &'a str) {
        if let Some(dot_pos) = s.find('.') { return (&s[..dot_pos], &s[dot_pos..]); }
        if let Some(e_pos) = s.find(|c: char| c == 'e' || c == 'E') { return (&s[..e_pos], &s[e_pos..]); }
        (s, "")
    }

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

    fn format_with_underscores(integer_part: &str) -> String {
        let digits: String = integer_part.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() <= 3 {
            return digits;
        }
        let mut result = String::new();
        let remainder = digits.len() % 3;
        if remainder > 0 {
            result.push_str(&digits[..remainder]);
        }
        for i in (remainder..digits.len()).step_by(3) {
            if !result.is_empty() {
                result.push('_');
            }
            result.push_str(&digits[i..i + 3]);
        }
        result
    }

    fn format_number(source: &str) -> String {
        let s = source.trim().trim_start_matches('-').trim_start_matches('+').trim();
        let is_negative = source.trim().starts_with('-');

        // Find decimal point
        if let Some(dot_pos) = s.find('.') {
            let integer_part = &s[..dot_pos];
            let rest = &s[dot_pos..]; // .789 or .789e3
            let formatted = Self::format_with_underscores(integer_part);
            if is_negative {
                format!("-{}{}", formatted, rest)
            } else {
                format!("{}{}", formatted, rest)
            }
        } else if let Some(e_pos) = s.find(|c: char| c == 'e' || c == 'E') {
            let integer_part = &s[..e_pos];
            let rest = &s[e_pos..];
            let formatted = Self::format_with_underscores(integer_part);
            if is_negative {
                format!("-{}{}", formatted, rest)
            } else {
                format!("{}{}", formatted, rest)
            }
        } else {
            let formatted = Self::format_with_underscores(s);
            if is_negative {
                format!("-{}", formatted)
            } else {
                formatted
            }
        }
    }

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

impl<'a> NumericVisitor<'a> {
    fn check_numeric(&mut self, loc: &ruby_prism::Location) {
        let node_source = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        let (check_source, offense_start, offense_end) =
            if let Some(minus_offset) = find_unary_minus_offset(self.ctx.source, loc.start_offset()) {
                (&self.ctx.source[minus_offset..loc.end_offset()], minus_offset, minus_offset + 1)
            } else {
                (node_source, loc.start_offset(), loc.end_offset())
            };

        let start_loc = Location::from_offsets(self.ctx.source, offense_start, offense_end);
        if let Some(line) = self.ctx.source.lines().nth((start_loc.line - 1) as usize) {
            if line.contains("rubocop:disable") && line.contains("Style/NumericLiterals") { return; }
        }

        if self.cop.check_integer_source(check_source) {
            let location = if offense_start != loc.start_offset() {
                start_loc
            } else {
                self.ctx.location(loc)
            };
            let corr_start = if offense_start != loc.start_offset() { offense_start } else { loc.start_offset() };
            let correction = Correction::replace(corr_start, loc.end_offset(), NumericLiterals::format_number(check_source));
            self.offenses.push(Offense::new(
                self.cop.name(),
                "Use underscores(_) as thousands separator and separate every 3 digits with them.",
                self.cop.severity(),
                location,
                self.ctx.filename,
            ).with_correction(correction));
        }
    }
}

impl Visit<'_> for NumericVisitor<'_> {
    fn visit_integer_node(&mut self, node: &ruby_prism::IntegerNode) {
        self.check_numeric(&node.location());
        ruby_prism::visit_integer_node(self, node);
    }

    fn visit_float_node(&mut self, node: &ruby_prism::FloatNode) {
        self.check_numeric(&node.location());
        ruby_prism::visit_float_node(self, node);
    }
}

crate::register_cop!("Style/NumericLiterals", |cfg| {
    let cop_config = cfg.get_cop_config("Style/NumericLiterals");
    let min_digits = cop_config
        .and_then(|c| c.raw.get("MinDigits"))
        .and_then(|v| v.as_u64())
        .unwrap_or(6) as usize;
    let strict = cop_config
        .and_then(|c| c.raw.get("Strict"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let allowed_numbers = cop_config
        .and_then(|c| c.raw.get("AllowedNumbers"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| {
                    v.as_i64().or_else(|| {
                        v.as_str().and_then(|s| s.parse::<i64>().ok())
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let allowed_patterns = cop_config
        .and_then(|c| c.raw.get("AllowedPatterns"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(NumericLiterals::with_config(
        min_digits,
        strict,
        allowed_numbers,
        allowed_patterns,
    )))
});
