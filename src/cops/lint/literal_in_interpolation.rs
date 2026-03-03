//! Lint/LiteralInInterpolation - Checks for literal values inside string interpolation.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/literal_in_interpolation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Literal interpolation detected.";

pub struct LiteralInInterpolation;

impl LiteralInInterpolation {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LiteralInInterpolation {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for LiteralInInterpolation {
    fn name(&self) -> &'static str {
        "Lint/LiteralInInterpolation"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = LiteralInInterpolationVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            in_percent_w_or_i: false,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct LiteralInInterpolationVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a LiteralInInterpolation,
    offenses: Vec<Offense>,
    /// Whether we are inside a %W[] or %I[] array literal
    in_percent_w_or_i: bool,
}

impl<'a> LiteralInInterpolationVisitor<'a> {
    /// Check an EmbeddedStatementsNode (the `#{...}` part) for literal content.
    fn check_embedded_statements(&mut self, node: &ruby_prism::EmbeddedStatementsNode, parent_is_regexp_slash: bool) {
        // Get the body (StatementsNode inside #{...})
        let statements = match node.statements() {
            Some(stmts) => stmts,
            None => return, // empty #{} - no offense
        };

        let body: Vec<_> = statements.body().iter().collect();
        if body.is_empty() {
            return;
        }

        // Only check the final statement in the interpolation
        // (RuboCop only flags the last expression in multi-statement interpolations like #{1;1})
        let final_node = &body[body.len() - 1];

        if !self.is_offending(final_node) {
            return;
        }

        // For %W and %I, check if expanded value contains space or is empty
        if self.in_percent_w_or_i {
            let expanded = autocorrected_value(final_node, self.ctx, false);
            if expanded.is_empty() || expanded.contains(|c: char| c.is_whitespace()) {
                return;
            }
        }

        // Create offense at the final_node's location
        let start = final_node.location().start_offset();
        let end = final_node.location().end_offset();

        let mut offense = self.ctx.offense_with_range(
            self.cop.name(),
            MSG,
            self.cop.severity(),
            start,
            end,
        );

        // Don't autocorrect nested dstr (InterpolatedStringNode inside interpolation)
        if !is_dstr_type(final_node) {
            // The correction replaces the entire #{...} (parent EmbeddedStatementsNode) with the literal value
            let embed_start = node.location().start_offset();
            let embed_end = node.location().end_offset();

            let replacement = if is_string_with_invalid_encoding(final_node, self.ctx) {
                // For invalid encoding strings, use source without outer quotes
                let src = source_text(final_node, self.ctx);
                strip_outer_quotes(&src)
            } else {
                let mut expanded = autocorrected_value(final_node, self.ctx, false);
                if parent_is_regexp_slash {
                    expanded = handle_special_regexp_chars(&expanded);
                }
                expanded
            };

            let correction = Correction::replace(embed_start, embed_end, replacement);
            offense = offense.with_correction(correction);
        }

        self.offenses.push(offense);
    }

    /// Determines if a node is an offending literal (should be flagged).
    fn is_offending(&self, node: &ruby_prism::Node) -> bool {
        if self.is_special_keyword(node) {
            return false;
        }

        if !prints_as_self(node) {
            return false;
        }

        // Special case: space literal at end of heredoc line
        if self.is_space_literal_at_end_of_heredoc(node) {
            return false;
        }

        true
    }

    /// Check if this is a special keyword like __FILE__, __LINE__, __END__, __ENCODING__
    fn is_special_keyword(&self, node: &ruby_prism::Node) -> bool {
        match node {
            // __FILE__ is a StringNode without quotes (no opening_loc)
            ruby_prism::Node::StringNode { .. } => {
                let str_node = node.as_string_node().unwrap();
                // StringNode without opening delimiter is a keyword string like __FILE__
                str_node.opening_loc().is_none()
            }
            // __LINE__ is a SourceLineNode (Prism represents it as IntegerNode or SourceLineNode)
            ruby_prism::Node::SourceLineNode { .. }
            | ruby_prism::Node::SourceFileNode { .. }
            | ruby_prism::Node::SourceEncodingNode { .. } => true,
            _ => {
                // Check source text for __LINE__, __END__
                let src = source_text(node, self.ctx);
                src == "__LINE__" || src == "__END__" || src == "__FILE__" || src == "__ENCODING__"
            }
        }
    }

    /// Check if this is a blank/space string literal at the end of a heredoc line.
    /// In that case, the interpolation is preserving trailing whitespace intentionally.
    fn is_space_literal_at_end_of_heredoc(&self, node: &ruby_prism::Node) -> bool {
        // Must be a string node with blank content
        if let ruby_prism::Node::StringNode { .. } = node {
            let str_node = node.as_string_node().unwrap();
            let content_loc = str_node.content_loc();
            let content = &self.ctx.source[content_loc.start_offset()..content_loc.end_offset()];
            if !content.chars().all(|c| c == ' ' || c == '\t') {
                return false;
            }
            if content.is_empty() {
                return false;
            }

            // Check if the EmbeddedStatementsNode ends at end of heredoc line
            // We approximate: check if the character after the closing `}` of interpolation
            // is a newline or end of string (accounting for heredoc)
            let node_end = node.location().end_offset();
            // Find the closing `}` after this node (part of the EmbeddedStatementsNode)
            // The `}` follows immediately after the node
            let after_brace = node_end + 1; // skip the `}`
            if after_brace >= self.ctx.source.len() {
                return true; // at end of source, close enough
            }
            let next_char = self.ctx.source.as_bytes()[after_brace];
            if next_char == b'\n' {
                return true;
            }
        }
        false
    }
}

impl Visit<'_> for LiteralInInterpolationVisitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                let embed = part.as_embedded_statements_node().unwrap();
                self.check_embedded_statements(&embed, false);
            }
        }
        // Continue visiting nested nodes
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_interpolated_symbol_node(&mut self, node: &ruby_prism::InterpolatedSymbolNode) {
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                let embed = part.as_embedded_statements_node().unwrap();
                self.check_embedded_statements(&embed, false);
            }
        }
        ruby_prism::visit_interpolated_symbol_node(self, node);
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        // Determine if this is a slash-delimited regexp (not %r{})
        let is_slash_literal = {
            let open_loc = node.opening_loc();
            let open_byte = self.ctx.source.as_bytes()[open_loc.start_offset()];
            open_byte == b'/'
        };

        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                let embed = part.as_embedded_statements_node().unwrap();
                // Check for array literals in regexp - skip those (handled by Lint/ArrayLiteralInRegexp)
                if let Some(stmts) = embed.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    if let Some(last) = body.last() {
                        if matches!(last, ruby_prism::Node::ArrayNode { .. }) {
                            continue;
                        }
                    }
                }
                self.check_embedded_statements(&embed, is_slash_literal);
            }
        }
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                let embed = part.as_embedded_statements_node().unwrap();
                self.check_embedded_statements(&embed, false);
            }
        }
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        // Detect %W[] and %I[] arrays
        let src = source_text_node_loc(node.location(), self.ctx);
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

/// Does the node "print as itself" -- i.e., is it a basic literal or a composite of literals?
fn prints_as_self(node: &ruby_prism::Node) -> bool {
    match node {
        // Basic literals
        ruby_prism::Node::IntegerNode { .. }
        | ruby_prism::Node::FloatNode { .. }
        | ruby_prism::Node::StringNode { .. }
        | ruby_prism::Node::SymbolNode { .. }
        | ruby_prism::Node::TrueNode { .. }
        | ruby_prism::Node::FalseNode { .. }
        | ruby_prism::Node::NilNode { .. }
        | ruby_prism::Node::ImaginaryNode { .. }
        | ruby_prism::Node::RationalNode { .. } => true,

        // Unary minus on numeric (e.g., -1)
        ruby_prism::Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let method = String::from_utf8_lossy(call.name().as_slice());
            if method == "-@" {
                // Unary minus -- check if operand is literal
                if let Some(recv) = call.receiver() {
                    return prints_as_self(&recv);
                }
            }
            false
        }

        // Ranges with literal endpoints
        ruby_prism::Node::RangeNode { .. } => {
            let range = node.as_range_node().unwrap();
            let left_ok = range.left().map_or(true, |l| prints_as_self(&l));
            let right_ok = range.right().map_or(true, |r| prints_as_self(&r));
            left_ok && right_ok
        }

        // Arrays with all literal elements
        ruby_prism::Node::ArrayNode { .. } => {
            let arr = node.as_array_node().unwrap();
            arr.elements().iter().all(|e| prints_as_self(&e))
        }

        // Hashes with all literal key/value pairs
        ruby_prism::Node::HashNode { .. } => {
            let hash = node.as_hash_node().unwrap();
            hash.elements().iter().all(|e| {
                if let ruby_prism::Node::AssocNode { .. } = &e {
                    let assoc = e.as_assoc_node().unwrap();
                    prints_as_self(&assoc.key()) && prints_as_self(&assoc.value())
                } else {
                    false
                }
            })
        }

        // AssocNode (pair) within a hash
        ruby_prism::Node::AssocNode { .. } => {
            let assoc = node.as_assoc_node().unwrap();
            prints_as_self(&assoc.key()) && prints_as_self(&assoc.value())
        }

        _ => false,
    }
}

/// Check if a node is an InterpolatedStringNode (dstr type)
fn is_dstr_type(node: &ruby_prism::Node) -> bool {
    matches!(node, ruby_prism::Node::InterpolatedStringNode { .. })
}

/// Check if a string node has invalid encoding (the unescaped/interpreted value isn't valid UTF-8)
fn is_string_with_invalid_encoding(node: &ruby_prism::Node, _ctx: &CheckContext) -> bool {
    if let ruby_prism::Node::StringNode { .. } = node {
        let str_node = node.as_string_node().unwrap();
        let unescaped = str_node.unescaped();
        // Check if the unescaped bytes are valid UTF-8
        std::str::from_utf8(unescaped.as_ref()).is_err()
    } else {
        false
    }
}

/// Get the source text for a node
fn source_text(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let loc = node.location();
    ctx.source[loc.start_offset()..loc.end_offset()].to_string()
}

/// Get source text from a location
fn source_text_node_loc(loc: ruby_prism::Location, ctx: &CheckContext) -> String {
    ctx.source[loc.start_offset()..loc.end_offset()].to_string()
}

/// Strip outer quotes from a string source
fn strip_outer_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Compute the autocorrected value for a literal node.
/// This produces the string that should replace the entire #{...} interpolation.
fn autocorrected_value(node: &ruby_prism::Node, ctx: &CheckContext, in_hash: bool) -> String {
    match node {
        ruby_prism::Node::IntegerNode { .. } => {
            // Parse the integer value from source, handling hex, octal, binary, underscores
            let src = source_text(node, ctx);
            parse_integer_literal(&src).to_string()
        }

        ruby_prism::Node::FloatNode { .. } => {
            let src = source_text(node, ctx);
            format_float_literal(&src)
        }

        ruby_prism::Node::NilNode { .. } => {
            if in_hash {
                "nil".to_string()
            } else {
                String::new()
            }
        }

        ruby_prism::Node::TrueNode { .. } => "true".to_string(),
        ruby_prism::Node::FalseNode { .. } => "false".to_string(),

        ruby_prism::Node::StringNode { .. } => {
            if in_hash {
                autocorrected_value_for_string_in_hash(node, ctx)
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

        ruby_prism::Node::ArrayNode { .. } => {
            autocorrected_value_for_array(node, ctx)
        }

        ruby_prism::Node::HashNode { .. } => {
            autocorrected_value_for_hash(node, ctx)
        }

        ruby_prism::Node::CallNode { .. } => {
            // Unary minus: -1, -1.5, etc.
            let call = node.as_call_node().unwrap();
            let method = String::from_utf8_lossy(call.name().as_slice());
            if method == "-@" {
                if let Some(recv) = call.receiver() {
                    let inner = autocorrected_value(&recv, ctx, in_hash);
                    return format!("-{}", inner);
                }
            }
            // Fallback: source with " escaped
            source_text(node, ctx).replace('"', "\\\"")
        }

        ruby_prism::Node::RangeNode { .. } => {
            // Use source text with " escaped
            source_text(node, ctx).replace('"', "\\\"")
        }

        _ => {
            source_text(node, ctx).replace('"', "\\\"")
        }
    }
}

/// For string nodes: determine autocorrected value.
/// Double-quoted strings: use the interpreted (unescaped) value directly.
/// Single-quoted strings: use the interpreted value, then inspect-like escaping.
fn autocorrected_value_for_string(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let str_node = node.as_string_node().unwrap();
    let src = source_text(node, ctx);

    if src.starts_with('\'') || src.starts_with("%q") {
        // Single-quoted string or %q(): get the interpreted value (unescaped),
        // then apply Ruby inspect-like escaping (for embedding in a double-quoted string context)
        let unescaped = String::from_utf8_lossy(str_node.unescaped().as_ref()).to_string();
        escape_for_double_quote_context(&unescaped)
    } else {
        // Double-quoted string or %(), %Q(): use the interpreted (unescaped) value directly.
        // This means "\n" becomes an actual newline character, which is what RuboCop does.
        String::from_utf8_lossy(str_node.unescaped().as_ref()).to_string()
    }
}

/// For string nodes inside hash values
/// Uses the interpreted value, wraps in escaped quotes: \"value\"
/// Double quotes in value become \\\"
fn autocorrected_value_for_string_in_hash(node: &ruby_prism::Node, _ctx: &CheckContext) -> String {
    let str_node = node.as_string_node().unwrap();
    let value = String::from_utf8_lossy(str_node.unescaped().as_ref()).to_string();

    // Escape double quotes: " becomes \\\"
    // The output goes into an interpolated string which is itself inside quotes
    let mut result = String::new();
    result.push('\\');
    result.push('"');
    for ch in value.chars() {
        if ch == '"' {
            result.push('\\');
            result.push('\\');
            result.push('\\');
            result.push('"');
        } else {
            result.push(ch);
        }
    }
    result.push('\\');
    result.push('"');
    result
}

/// For symbol nodes: extract just the name part (without the colon)
fn autocorrected_value_for_symbol(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let sym_node = node.as_symbol_node().unwrap();
    let src = source_text(node, ctx);

    if src.starts_with(":\"") || src.starts_with(":'") {
        // Quoted symbol - extract content between quotes
        if let Some(value_loc) = sym_node.value_loc() {
            let value = &ctx.source[value_loc.start_offset()..value_loc.end_offset()];

            // For :"..." symbols, content is already interpreted
            // For :'...' symbols, content is literal
            if src.starts_with(":'") {
                // Single-quoted symbol: escape for double-quote context
                escape_for_double_quote_context(value)
            } else {
                // Double-quoted symbol: content already interpreted
                value.replace('"', "\\\"")
            }
        } else {
            // Fallback
            src.strip_prefix(':').unwrap_or(&src).to_string()
        }
    } else {
        // Simple symbol :foo - just strip the colon
        src.strip_prefix(':').unwrap_or(&src).to_string()
    }
}

/// For symbol nodes inside hash values
/// RuboCop's `autocorrected_value_in_hash_for_symbol`:
/// If value contains space, " or ' -> :\"escaped_value\" (literal backslash-quote)
/// Otherwise -> :value
fn autocorrected_value_for_symbol_in_hash(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let sym_node = node.as_symbol_node().unwrap();
    let src = source_text(node, ctx);

    if let Some(value_loc) = sym_node.value_loc() {
        let value = &ctx.source[value_loc.start_offset()..value_loc.end_offset()];

        // Check if symbol value contains spaces, quotes, etc. that need special handling
        if value.contains(' ') || value.contains('"') || value.contains('\'') {
            // Build :\"value\" with \\\" for any internal double quotes
            let mut result = String::from(":");
            result.push('\\');
            result.push('"');
            for ch in value.chars() {
                if ch == '"' {
                    result.push('\\');
                    result.push('\\');
                    result.push('\\');
                    result.push('"');
                } else {
                    result.push(ch);
                }
            }
            result.push('\\');
            result.push('"');
            result
        } else {
            format!(":{}", value)
        }
    } else {
        let name = src.strip_prefix(':').unwrap_or(&src);
        format!(":{}", name)
    }
}

/// For array nodes
fn autocorrected_value_for_array(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let arr = node.as_array_node().unwrap();
    let src = source_text(node, ctx);

    // Check if it's a percent literal (%w, %i, %W, %I)
    if src.starts_with("%w") || src.starts_with("%W") {
        // Word array - split by whitespace and format as Ruby array
        let open_loc = arr.opening_loc().unwrap();
        let close_loc = arr.closing_loc().unwrap();
        // Content is between opening delimiter+bracket and closing bracket
        // For %w[...], opening is "%w[" and closing is "]"
        let content_start = open_loc.end_offset();
        let content_end = close_loc.start_offset();
        let content = ctx.source[content_start..content_end].trim();
        if content.is_empty() {
            return "[]".to_string();
        }
        let words: Vec<&str> = content.split_whitespace().collect();
        let formatted: Vec<String> = words.iter().map(|w| format!("\\\"{}\\\"", w)).collect();
        format!("[{}]", formatted.join(", "))
    } else if src.starts_with("%i") || src.starts_with("%I") {
        // Symbol array - split by whitespace
        let open_loc = arr.opening_loc().unwrap();
        let close_loc = arr.closing_loc().unwrap();
        let content_start = open_loc.end_offset();
        let content_end = close_loc.start_offset();
        let content = ctx.source[content_start..content_end].trim();
        if content.is_empty() {
            return "[]".to_string();
        }
        let words: Vec<&str> = content.split_whitespace().collect();
        let formatted: Vec<String> = words.iter().map(|w| format!("\\\"{}\\\"", w)).collect();
        format!("[{}]", formatted.join(", "))
    } else {
        // Regular array literal - escape double quotes in the source representation
        source_text(node, ctx).replace('"', "\\\"")
    }
}

/// For hash nodes
fn autocorrected_value_for_hash(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    let hash = node.as_hash_node().unwrap();
    let pairs: Vec<String> = hash.elements().iter().map(|e| {
        if let ruby_prism::Node::AssocNode { .. } = &e {
            let assoc = e.as_assoc_node().unwrap();
            let key = autocorrected_value_in_hash(&assoc.key(), ctx);
            let value = autocorrected_value_in_hash(&assoc.value(), ctx);
            format!("{}=>{}", key, value)
        } else {
            source_text(&e, ctx)
        }
    }).collect();
    format!("{{{}}}", pairs.join(", "))
}

/// Compute autocorrected value for a node inside a hash (different escaping rules)
fn autocorrected_value_in_hash(node: &ruby_prism::Node, ctx: &CheckContext) -> String {
    match node {
        ruby_prism::Node::IntegerNode { .. } => {
            let src = source_text(node, ctx);
            parse_integer_literal(&src).to_string()
        }
        ruby_prism::Node::FloatNode { .. } => {
            let src = source_text(node, ctx);
            format_float_literal(&src)
        }
        ruby_prism::Node::NilNode { .. } => "nil".to_string(),
        ruby_prism::Node::StringNode { .. } => {
            autocorrected_value_for_string_in_hash(node, ctx)
        }
        ruby_prism::Node::SymbolNode { .. } => {
            autocorrected_value_for_symbol_in_hash(node, ctx)
        }
        ruby_prism::Node::TrueNode { .. } => "true".to_string(),
        ruby_prism::Node::FalseNode { .. } => "false".to_string(),
        ruby_prism::Node::ArrayNode { .. } => {
            autocorrected_value_for_array(node, ctx)
        }
        ruby_prism::Node::HashNode { .. } => {
            autocorrected_value_for_hash(node, ctx)
        }
        _ => {
            source_text(node, ctx).replace('"', "\\\"")
        }
    }
}

/// Parse an integer literal, handling hex (0x), octal (0o), binary (0b), and underscores
fn parse_integer_literal(src: &str) -> i128 {
    let s = src.replace('_', "");
    if s.starts_with("0x") || s.starts_with("0X") {
        i128::from_str_radix(&s[2..], 16).unwrap_or(0)
    } else if s.starts_with("0o") || s.starts_with("0O") {
        i128::from_str_radix(&s[2..], 8).unwrap_or(0)
    } else if s.starts_with("0b") || s.starts_with("0B") {
        i128::from_str_radix(&s[2..], 2).unwrap_or(0)
    } else if s.starts_with("0d") || s.starts_with("0D") {
        s[2..].parse().unwrap_or(0)
    } else {
        s.parse().unwrap_or(0)
    }
}

/// Parse a float literal, handling scientific notation and underscores
/// Returns the formatted string (Ruby's to_f.to_s behavior)
fn format_float_literal(src: &str) -> String {
    let s = src.replace('_', "");
    let val: f64 = s.parse().unwrap_or(0.0);
    // Ruby's Float#to_s always includes a decimal point
    let formatted = val.to_string();
    if !formatted.contains('.') && !formatted.contains('e') && !formatted.contains('E') {
        format!("{}.0", formatted)
    } else {
        formatted
    }
}

/// Escape content for inclusion in a double-quoted string context.
/// This handles single-quoted string content being inlined into a double-quoted outer string.
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

/// Handle special regexp chars: escape forward slashes for slash-delimited regexps
/// Matches RuboCop's logic: `(2 * ((backslash_count + 1) / 4)) + 1` needed backslashes
fn handle_special_regexp_chars(value: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '/' {
            // Count preceding backslashes in result
            let mut backslash_count = 0;
            let mut j = result.len();
            while j > 0 {
                let prev = result.as_bytes()[j - 1];
                if prev == b'\\' {
                    backslash_count += 1;
                    j -= 1;
                } else {
                    break;
                }
            }
            // Remove the existing backslashes
            let new_len = result.len() - backslash_count;
            result.truncate(new_len);

            // Calculate needed backslashes: (2 * ((n + 1) / 4)) + 1
            // Note: integer division -- (n+1)/4 is computed first
            let needed = (2 * ((backslash_count + 1) / 4)) + 1;
            for _ in 0..needed {
                result.push('\\');
            }
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
        let offenses = check(r#""this is #{a} silly""#);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn accepts_xstr_in_interpolation() {
        let offenses = check(r#""this is #{`a`} silly""#);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_string_in_interpolation() {
        let offenses = check(r#""this is the #{"foo"}""#);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_symbol_in_interpolation() {
        let offenses = check(r#""this is the #{:symbol}""#);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_float_in_interpolation() {
        let offenses = check(r#""this is the #{2.0}""#);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_true_in_interpolation() {
        let offenses = check(r#""this is the #{true}""#);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_false_in_interpolation() {
        let offenses = check(r#""this is the #{false}""#);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_nil_in_interpolation() {
        let offenses = check(r#""this is the #{nil}""#);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn accepts_empty_interpolation() {
        let offenses = check(r#""this is #{a} silly""#);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_range_with_literal_endpoints() {
        let offenses = check(r#""this is the #{1..2}""#);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn accepts_range_with_nonliteral_endpoints() {
        let offenses = check(r#""this is an irange: #{var1..var2}""#);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn parse_integer_hex() {
        assert_eq!(parse_integer_literal("0xaabb"), 43707);
    }

    #[test]
    fn parse_integer_octal() {
        assert_eq!(parse_integer_literal("0o377"), 255);
    }

    #[test]
    fn parse_integer_with_underscores() {
        assert_eq!(parse_integer_literal("1_123"), 1123);
    }
}
