//! Style/StringLiteralsInInterpolation cop
//!
//! Checks quote style of string literals inside interpolations.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    SingleQuotes,
    DoubleQuotes,
}

pub struct StringLiteralsInInterpolation {
    enforced_style: EnforcedStyle,
}

impl StringLiteralsInInterpolation {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { enforced_style: style }
    }

    fn to_single(s: &str) -> String {
        let inner = &s[1..s.len() - 1];
        let mut result = String::with_capacity(s.len());
        result.push('\'');
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' {
                result.push('"');
                i += 2;
                continue;
            }
            result.push(chars[i]);
            i += 1;
        }
        result.push('\'');
        result
    }

    fn to_double(s: &str) -> String {
        let inner = &s[1..s.len() - 1];
        let mut result = String::with_capacity(s.len());
        result.push('"');
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '\'' {
                result.push('\'');
                i += 2;
                continue;
            }
            result.push(chars[i]);
            i += 1;
        }
        result.push('"');
        result
    }
}

impl Cop for StringLiteralsInInterpolation {
    fn name(&self) -> &'static str {
        "Style/StringLiteralsInInterpolation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = InterpStringVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            inside_interp: false,
        };
        ruby_prism::visit_program_node(&mut visitor, &result.node().as_program_node().unwrap());
        visitor.offenses
    }
}

struct InterpStringVisitor<'a> {
    cop: &'a StringLiteralsInInterpolation,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    inside_interp: bool,
}

impl InterpStringVisitor<'_> {
    fn check_string(&mut self, node: &ruby_prism::StringNode) {
        if !self.inside_interp {
            return;
        }
        let loc = node.location();
        let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        // Only handle plain quoted strings (not heredoc, not %q, etc.)
        let is_single = src.starts_with('\'');
        let is_double = src.starts_with('"');
        if !is_single && !is_double {
            return;
        }
        match self.cop.enforced_style {
            EnforcedStyle::SingleQuotes => {
                if is_double {
                    let corrected = StringLiteralsInInterpolation::to_single(src);
                    let msg = "Prefer single-quoted strings inside interpolations.";
                    let correction = Correction::replace(loc.start_offset(), loc.end_offset(), corrected);
                    self.offenses.push(
                        self.ctx.offense_with_range(self.cop.name(), msg, self.cop.severity(),
                            loc.start_offset(), loc.end_offset())
                            .with_correction(correction)
                    );
                }
            }
            EnforcedStyle::DoubleQuotes => {
                if is_single {
                    let corrected = StringLiteralsInInterpolation::to_double(src);
                    let msg = "Prefer double-quoted strings inside interpolations.";
                    let correction = Correction::replace(loc.start_offset(), loc.end_offset(), corrected);
                    self.offenses.push(
                        self.ctx.offense_with_range(self.cop.name(), msg, self.cop.severity(),
                            loc.start_offset(), loc.end_offset())
                            .with_correction(correction)
                    );
                }
            }
        }
    }
}

impl Visit<'_> for InterpStringVisitor<'_> {
    fn visit_embedded_statements_node(&mut self, node: &ruby_prism::EmbeddedStatementsNode) {
        let was = self.inside_interp;
        self.inside_interp = true;
        ruby_prism::visit_embedded_statements_node(self, node);
        self.inside_interp = was;
    }

    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        self.check_string(node);
        ruby_prism::visit_string_node(self, node);
    }
}

crate::register_cop!("Style/StringLiteralsInInterpolation", |cfg| {
    let raw_style = cfg.get_cop_config("Style/StringLiteralsInInterpolation")
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| s.as_str().to_string());
    let style = match raw_style.as_deref() {
        None | Some("single_quotes") => EnforcedStyle::SingleQuotes,
        Some("double_quotes") => EnforcedStyle::DoubleQuotes,
        Some(_) => return None, // Unknown style value → don't run cop
    };
    Some(Box::new(StringLiteralsInInterpolation::new(style)))
});
