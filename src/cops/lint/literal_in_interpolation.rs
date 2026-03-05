//! Lint/LiteralInInterpolation - Checks for literal values inside string interpolation.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Literal interpolation detected.";

pub struct LiteralInInterpolation;

impl LiteralInInterpolation {
    pub fn new() -> Self { Self }
}

impl Default for LiteralInInterpolation {
    fn default() -> Self { Self::new() }
}

impl Cop for LiteralInInterpolation {
    fn name(&self) -> &'static str { "Lint/LiteralInInterpolation" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = LiteralInInterpolationVisitor {
            ctx, cop: self, offenses: Vec::new(), in_percent_w_or_i: false,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct LiteralInInterpolationVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a LiteralInInterpolation,
    offenses: Vec<Offense>,
    in_percent_w_or_i: bool,
}

impl<'a> LiteralInInterpolationVisitor<'a> {
    fn check_embedded_statements(&mut self, node: &ruby_prism::EmbeddedStatementsNode, parent_is_regexp_slash: bool) {
        let statements = match node.statements() {
            Some(stmts) => stmts,
            None => return,
        };

        let body: Vec<_> = statements.body().iter().collect();
        if body.is_empty() { return; }

        let final_node = &body[body.len() - 1];
        if !self.is_offending(final_node) { return; }

        if self.in_percent_w_or_i {
            let expanded = autocorrected_value(final_node, self.ctx, false);
            if expanded.is_empty() || expanded.contains(|c: char| c.is_whitespace()) { return; }
        }

        let start = final_node.location().start_offset();
        let end = final_node.location().end_offset();
        let mut offense = self.ctx.offense_with_range(self.cop.name(), MSG, self.cop.severity(), start, end);

        if !matches!(final_node, ruby_prism::Node::InterpolatedStringNode { .. }) {
            let embed_start = node.location().start_offset();
            let embed_end = node.location().end_offset();

            let replacement = if is_string_with_invalid_encoding(final_node) {
                strip_outer_quotes(&source_text(final_node, self.ctx))
            } else {
                let mut expanded = autocorrected_value(final_node, self.ctx, false);
                if parent_is_regexp_slash {
                    expanded = handle_special_regexp_chars(&expanded);
                }
                expanded
            };

            offense = offense.with_correction(Correction::replace(embed_start, embed_end, replacement));
        }

        self.offenses.push(offense);
    }

    fn is_offending(&self, node: &ruby_prism::Node) -> bool {
        !self.is_special_keyword(node) && prints_as_self(node) && !self.is_space_literal_at_end_of_heredoc(node)
    }

    fn is_special_keyword(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::StringNode { .. } => node.as_string_node().unwrap().opening_loc().is_none(),
            ruby_prism::Node::SourceLineNode { .. } | ruby_prism::Node::SourceFileNode { .. }
            | ruby_prism::Node::SourceEncodingNode { .. } => true,
            _ => {
                let src = source_text(node, self.ctx);
                matches!(src.as_str(), "__LINE__" | "__END__" | "__FILE__" | "__ENCODING__")
            }
        }
    }

    fn is_space_literal_at_end_of_heredoc(&self, node: &ruby_prism::Node) -> bool {
        if let ruby_prism::Node::StringNode { .. } = node {
            let str_node = node.as_string_node().unwrap();
            let content_loc = str_node.content_loc();
            let content = &self.ctx.source[content_loc.start_offset()..content_loc.end_offset()];
            if content.is_empty() || !content.chars().all(|c| c == ' ' || c == '\t') { return false; }
            let after_brace = node.location().end_offset() + 1;
            if after_brace >= self.ctx.source.len() { return true; }
            return self.ctx.source.as_bytes()[after_brace] == b'\n';
        }
        false
    }

}

/// Check all EmbeddedStatementsNode parts for literal interpolation (non-regexp context).
macro_rules! check_parts {
    ($self:expr, $node:expr) => {
        for part in $node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                $self.check_embedded_statements(&part.as_embedded_statements_node().unwrap(), false);
            }
        }
    };
}

impl Visit<'_> for LiteralInInterpolationVisitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        check_parts!(self, node);
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        check_parts!(self, node);
        ruby_prism::visit_interpolated_symbol_node(self, node);
    }

    fn visit_interpolated_regular_expression_node(&mut self, node: &ruby_prism::InterpolatedRegularExpressionNode) {
        let is_slash = self.ctx.source.as_bytes()[node.opening_loc().start_offset()] == b'/';
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                let embed = part.as_embedded_statements_node().unwrap();
                if let Some(stmts) = embed.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    if body.last().map_or(false, |last| matches!(last, ruby_prism::Node::ArrayNode { .. })) {
                        continue;
                    }
                }
                self.check_embedded_statements(&embed, is_slash);
            }
        }
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        check_parts!(self, node);
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let loc = node.location();
        let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
        let is_percent = src.starts_with("%W") || src.starts_with("%w")
            || src.starts_with("%I") || src.starts_with("%i");

        if is_percent {
            let prev = self.in_percent_w_or_i;
            self.in_percent_w_or_i = true;
            ruby_prism::visit_array_node(self, node);
            self.in_percent_w_or_i = prev;
        } else {
            ruby_prism::visit_array_node(self, node);
        }
    }
}

fn prints_as_self(node: &ruby_prism::Node) -> bool {
    match node {
        ruby_prism::Node::IntegerNode { .. } | ruby_prism::Node::FloatNode { .. }
        | ruby_prism::Node::StringNode { .. } | ruby_prism::Node::SymbolNode { .. }
        | ruby_prism::Node::TrueNode { .. } | ruby_prism::Node::FalseNode { .. }
        | ruby_prism::Node::NilNode { .. } | ruby_prism::Node::ImaginaryNode { .. }
        | ruby_prism::Node::RationalNode { .. } => true,

        ruby_prism::Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            String::from_utf8_lossy(call.name().as_slice()) == "-@"
                && call.receiver().map_or(false, |r| prints_as_self(&r))
        }

        ruby_prism::Node::RangeNode { .. } => {
            let range = node.as_range_node().unwrap();
            range.left().map_or(true, |l| prints_as_self(&l))
                && range.right().map_or(true, |r| prints_as_self(&r))
        }

        ruby_prism::Node::ArrayNode { .. } => {
            node.as_array_node().unwrap().elements().iter().all(|e| prints_as_self(&e))
        }

        ruby_prism::Node::HashNode { .. } => {
            node.as_hash_node().unwrap().elements().iter().all(|e| {
                if let ruby_prism::Node::AssocNode { .. } = &e {
                    let assoc = e.as_assoc_node().unwrap();
                    prints_as_self(&assoc.key()) && prints_as_self(&assoc.value())
                } else {
                    false
                }
            })
        }

        ruby_prism::Node::AssocNode { .. } => {
            let assoc = node.as_assoc_node().unwrap();
            prints_as_self(&assoc.key()) && prints_as_self(&assoc.value())
        }

        _ => false,
    }
}

fn is_string_with_invalid_encoding(node: &ruby_prism::Node) -> bool {
    if let ruby_prism::Node::StringNode { .. } = node {
        std::str::from_utf8(node.as_string_node().unwrap().unescaped().as_ref()).is_err()
    } else {
        false
    }
}

fn source_text(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let loc = node.location();
    ctx.source[loc.start_offset()..loc.end_offset()].to_string()
}

fn strip_outer_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn autocorrected_value(node: &ruby_prism::Node, ctx: &CheckContext, in_hash: bool) -> String {
    match node {
        ruby_prism::Node::IntegerNode { .. } => {
            parse_integer_literal(&source_text(node, ctx)).to_string()
        }
        ruby_prism::Node::FloatNode { .. } => format_float_literal(&source_text(node, ctx)),
        ruby_prism::Node::NilNode { .. } => if in_hash { "nil".to_string() } else { String::new() },
        ruby_prism::Node::TrueNode { .. } => "true".to_string(),
        ruby_prism::Node::FalseNode { .. } => "false".to_string(),

        ruby_prism::Node::StringNode { .. } => {
            if in_hash {
                autocorrected_value_for_string_in_hash(node)
            } else {
                autocorrected_value_for_string(node, ctx)
            }
        }

        ruby_prism::Node::SymbolNode { .. } => {
            if in_hash {
                autocorrected_value_for_symbol_in_hash(node, ctx)
            } else {
                autocorrected_value_for_symbol(node, ctx)
            }
        }

        ruby_prism::Node::ArrayNode { .. } => autocorrected_value_for_array(node, ctx),
        ruby_prism::Node::HashNode { .. } => autocorrected_value_for_hash(node, ctx),

        ruby_prism::Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if String::from_utf8_lossy(call.name().as_slice()) == "-@" {
                if let Some(recv) = call.receiver() {
                    return format!("-{}", autocorrected_value(&recv, ctx, in_hash));
                }
            }
            source_text(node, ctx).replace('"', "\\\"")
        }

        ruby_prism::Node::RangeNode { .. } => source_text(node, ctx).replace('"', "\\\""),
        _ => source_text(node, ctx).replace('"', "\\\""),
    }
}

fn autocorrected_value_for_string(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let str_node = node.as_string_node().unwrap();
    let src = source_text(node, ctx);
    if src.starts_with('\'') || src.starts_with("%q") {
        escape_for_double_quote_context(&String::from_utf8_lossy(str_node.unescaped().as_ref()))
    } else {
        String::from_utf8_lossy(str_node.unescaped().as_ref()).to_string()
    }
}

fn autocorrected_value_for_string_in_hash(node: &ruby_prism::Node) -> String {
    let value = String::from_utf8_lossy(node.as_string_node().unwrap().unescaped().as_ref()).to_string();
    let mut result = String::from("\\\"");
    for ch in value.chars() {
        if ch == '"' { result.push_str("\\\\\\\""); } else { result.push(ch); }
    }
    result.push_str("\\\"");
    result
}

fn autocorrected_value_for_symbol(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let sym_node = node.as_symbol_node().unwrap();
    let src = source_text(node, ctx);

    if src.starts_with(":\"") || src.starts_with(":'") {
        if let Some(value_loc) = sym_node.value_loc() {
            let value = &ctx.source[value_loc.start_offset()..value_loc.end_offset()];
            return if src.starts_with(":'") {
                escape_for_double_quote_context(value)
            } else {
                value.replace('"', "\\\"")
            };
        }
        return src.strip_prefix(':').unwrap_or(&src).to_string();
    }
    src.strip_prefix(':').unwrap_or(&src).to_string()
}

fn autocorrected_value_for_symbol_in_hash(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let sym_node = node.as_symbol_node().unwrap();
    if let Some(value_loc) = sym_node.value_loc() {
        let value = &ctx.source[value_loc.start_offset()..value_loc.end_offset()];
        if value.contains(' ') || value.contains('"') || value.contains('\'') {
            let mut result = String::from(":\\\"");
            for ch in value.chars() {
                if ch == '"' { result.push_str("\\\\\\\""); } else { result.push(ch); }
            }
            result.push_str("\\\"");
            return result;
        }
        return format!(":{}", value);
    }
    let src = source_text(node, ctx);
    format!(":{}", src.strip_prefix(':').unwrap_or(&src))
}

fn autocorrected_value_for_array(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let arr = node.as_array_node().unwrap();
    let src = source_text(node, ctx);

    if src.starts_with("%w") || src.starts_with("%W") || src.starts_with("%i") || src.starts_with("%I") {
        let open_loc = arr.opening_loc().unwrap();
        let close_loc = arr.closing_loc().unwrap();
        let content = ctx.source[open_loc.end_offset()..close_loc.start_offset()].trim();
        if content.is_empty() { return "[]".to_string(); }
        let formatted: Vec<String> = content.split_whitespace().map(|w| format!("\\\"{}\\\"", w)).collect();
        return format!("[{}]", formatted.join(", "));
    }
    source_text(node, ctx).replace('"', "\\\"")
}

fn autocorrected_value_for_hash(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let hash = node.as_hash_node().unwrap();
    let pairs: Vec<String> = hash.elements().iter().map(|e| {
        if let ruby_prism::Node::AssocNode { .. } = &e {
            let assoc = e.as_assoc_node().unwrap();
            format!("{}=>{}", autocorrected_value(&assoc.key(), ctx, true), autocorrected_value(&assoc.value(), ctx, true))
        } else {
            source_text(&e, ctx)
        }
    }).collect();
    format!("{{{}}}", pairs.join(", "))
}

fn parse_integer_literal(src: &str) -> i128 {
    let s = src.replace('_', "");
    if s.starts_with("0x") || s.starts_with("0X") { i128::from_str_radix(&s[2..], 16).unwrap_or(0) }
    else if s.starts_with("0o") || s.starts_with("0O") { i128::from_str_radix(&s[2..], 8).unwrap_or(0) }
    else if s.starts_with("0b") || s.starts_with("0B") { i128::from_str_radix(&s[2..], 2).unwrap_or(0) }
    else if s.starts_with("0d") || s.starts_with("0D") { s[2..].parse().unwrap_or(0) }
    else { s.parse().unwrap_or(0) }
}

fn format_float_literal(src: &str) -> String {
    let val: f64 = src.replace('_', "").parse().unwrap_or(0.0);
    let formatted = val.to_string();
    if !formatted.contains('.') && !formatted.contains('e') && !formatted.contains('E') {
        format!("{}.0", formatted)
    } else {
        formatted
    }
}

fn escape_for_double_quote_context(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    for ch in content.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            _ => result.push(ch),
        }
    }
    result
}

fn handle_special_regexp_chars(value: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '/' {
            let mut backslash_count = 0;
            let mut j = result.len();
            while j > 0 && result.as_bytes()[j - 1] == b'\\' {
                backslash_count += 1;
                j -= 1;
            }
            result.truncate(result.len() - backslash_count);
            let needed = (2 * ((backslash_count + 1) / 4)) + 1;
            for _ in 0..needed { result.push('\\'); }
            result.push('/');
        } else {
            result.push(chars[i]);
        }
        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_source_with_cops;

    fn check(source: &str) -> Vec<Offense> {
        let cops: Vec<Box<dyn crate::cops::Cop>> = vec![Box::new(LiteralInInterpolation::new())];
        check_source_with_cops(source, "test.rb", &cops)
    }

    #[test]
    fn detects_integer_in_interpolation() {
        let offenses = check(r#""this is the #{1}""#);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, MSG);
    }

    #[test]
    fn accepts_variable_in_interpolation() {
        assert_eq!(check(r#""this is #{a} silly""#).len(), 0);
    }

    #[test]
    fn accepts_xstr_in_interpolation() {
        assert_eq!(check(r#""this is #{`a`} silly""#).len(), 0);
    }

    #[test]
    fn detects_string_in_interpolation() {
        assert_eq!(check(r#""this is the #{"foo"}""#).len(), 1);
    }

    #[test]
    fn detects_symbol_in_interpolation() {
        assert_eq!(check(r#""this is the #{:symbol}""#).len(), 1);
    }

    #[test]
    fn detects_float_in_interpolation() {
        assert_eq!(check(r#""this is the #{2.0}""#).len(), 1);
    }

    #[test]
    fn detects_true_in_interpolation() {
        assert_eq!(check(r#""this is the #{true}""#).len(), 1);
    }

    #[test]
    fn detects_false_in_interpolation() {
        assert_eq!(check(r#""this is the #{false}""#).len(), 1);
    }

    #[test]
    fn detects_nil_in_interpolation() {
        assert_eq!(check(r#""this is the #{nil}""#).len(), 1);
    }

    #[test]
    fn detects_range_with_literal_endpoints() {
        assert_eq!(check(r#""this is the #{1..2}""#).len(), 1);
    }

    #[test]
    fn accepts_range_with_nonliteral_endpoints() {
        assert_eq!(check(r#""this is an irange: #{var1..var2}""#).len(), 0);
    }

    #[test]
    fn parse_integer_hex() { assert_eq!(parse_integer_literal("0xaabb"), 43707); }

    #[test]
    fn parse_integer_octal() { assert_eq!(parse_integer_literal("0o377"), 255); }

    #[test]
    fn parse_integer_with_underscores() { assert_eq!(parse_integer_literal("1_123"), 1123); }
}
