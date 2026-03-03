//! Style/FormatStringToken - Checks format string tokens.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/format_string_token.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;

/// Enforced style for format string tokens
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnforcedStyle {
    /// Requires `%<name>s` style tokens
    Annotated,
    /// Requires `%{name}` style tokens
    Template,
    /// Requires `%s` style tokens (positional)
    Unannotated,
}

/// Token kind detected in a format string
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TokenKind {
    Annotated,
    Template,
    Unannotated,
}

/// A single format token found in a string
#[derive(Debug)]
struct FormatToken {
    /// Byte offset from start of the string content
    byte_offset: usize,
    /// Length in bytes
    byte_length: usize,
    /// What kind of token this is
    kind: TokenKind,
    /// The format type character (e.g., 's', 'd', 'f')
    type_char: char,
}

/// Checks format string tokens style.
pub struct FormatStringToken {
    enforced_style: EnforcedStyle,
    max_unannotated_placeholders: usize,
    conservative: bool,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<Regex>,
    token_regex: Regex,
}

impl FormatStringToken {
    pub fn new(enforced_style: EnforcedStyle) -> Self {
        Self::with_config(enforced_style, 0, false, vec![], vec![])
    }

    pub fn with_config(
        enforced_style: EnforcedStyle,
        max_unannotated_placeholders: usize,
        conservative: bool,
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        let compiled_patterns = allowed_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        // Single regex that matches all format token types:
        // Group 1: %% (escaped percent)
        // Group 2: annotated %<name>X
        // Group 3: template %{name}
        // Group 4: unannotated %s, %-20d, %1$s etc.
        //
        // Valid format type chars: [diouxXeEfgGaAcps]
        // Flags: [#0 +-]
        // Width: \d+ or *
        // Precision: .\d+ or .*
        let token_regex = Regex::new(
            r"(?x)
            (?:%%)|                                                             # escaped percent
            (?:%<(\w+)>[\ \#0+\-]*(?:\*|\d+)?(?:\.(?:\*|\d+))?([bBdiouxXeEfgGaAcps]))|  # annotated
            (?:%\{(\w+)\})|                                                     # template
            (?:%(?:\d+\$)?[\ \#0+\-]*(?:\*|\d+)?(?:\.(?:\*|\d+))?([bBdiouxXeEfgGaAcps]))  # unannotated
            "
        ).unwrap();

        Self {
            enforced_style,
            max_unannotated_placeholders,
            conservative,
            allowed_methods,
            allowed_patterns: compiled_patterns,
            token_regex,
        }
    }

    /// Find all format tokens in a string content
    fn find_format_tokens(&self, content: &str) -> Vec<FormatToken> {
        let mut tokens = Vec::new();

        for cap in self.token_regex.captures_iter(content) {
            let m = cap.get(0).unwrap();

            // Skip %% (escaped percent)
            if m.as_str() == "%%" {
                continue;
            }

            // Check which group matched
            if cap.get(1).is_some() {
                // Annotated: %<name>X - group 1 is name, group 2 is type char
                let type_char = cap.get(2).unwrap().as_str().chars().next().unwrap();
                tokens.push(FormatToken {
                    byte_offset: m.start(),
                    byte_length: m.len(),
                    kind: TokenKind::Annotated,
                    type_char,
                });
            } else if cap.get(3).is_some() {
                // Template: %{name} - group 3 is name
                tokens.push(FormatToken {
                    byte_offset: m.start(),
                    byte_length: m.len(),
                    kind: TokenKind::Template,
                    type_char: 's', // templates are always string type
                });
            } else if cap.get(4).is_some() {
                // Unannotated: %s, %-20d, etc. - group 4 is type char
                let type_char = cap.get(4).unwrap().as_str().chars().next().unwrap();
                tokens.push(FormatToken {
                    byte_offset: m.start(),
                    byte_length: m.len(),
                    kind: TokenKind::Unannotated,
                    type_char,
                });
            }
        }

        tokens
    }

    /// Check if a token kind matches the enforced style
    fn matches_enforced_style(&self, kind: TokenKind) -> bool {
        matches!(
            (self.enforced_style, kind),
            (EnforcedStyle::Annotated, TokenKind::Annotated)
                | (EnforcedStyle::Template, TokenKind::Template)
                | (EnforcedStyle::Unannotated, TokenKind::Unannotated)
        )
    }

    /// Check if a format sequence type is correctable to the target style
    fn correctable_sequence(&self, type_char: char) -> bool {
        match self.enforced_style {
            EnforcedStyle::Template => type_char == 's',
            _ => true,
        }
    }

    /// Generate the offense message for a detected token style
    fn message(&self, detected_kind: TokenKind) -> String {
        format!(
            "Prefer {} over {}.",
            Self::message_text_for_style(self.enforced_style),
            Self::message_text_for_kind(detected_kind)
        )
    }

    fn message_text_for_style(style: EnforcedStyle) -> &'static str {
        match style {
            EnforcedStyle::Annotated => "annotated tokens (like `%<foo>s`)",
            EnforcedStyle::Template => "template tokens (like `%{foo}`)",
            EnforcedStyle::Unannotated => "unannotated tokens (like `%s`)",
        }
    }

    fn message_text_for_kind(kind: TokenKind) -> &'static str {
        match kind {
            TokenKind::Annotated => "annotated tokens (like `%<foo>s`)",
            TokenKind::Template => "template tokens (like `%{foo}`)",
            TokenKind::Unannotated => "unannotated tokens (like `%s`)",
        }
    }

    /// Compute the corrected form of a format token.
    /// Returns None if correction is not possible.
    fn corrected_token(&self, token: &FormatToken, content: &str) -> Option<String> {
        let token_str = &content[token.byte_offset..token.byte_offset + token.byte_length];
        match (token.kind, self.enforced_style) {
            (TokenKind::Template, EnforcedStyle::Annotated) => {
                // %{name} → %<name>s
                // Extract the name from %{name}
                if let Some(name) = token_str.strip_prefix("%{").and_then(|s| s.strip_suffix('}')) {
                    Some(format!("%<{}>s", name))
                } else {
                    None
                }
            }
            (TokenKind::Annotated, EnforcedStyle::Template) => {
                // %<name>s → %{name} (only when type is 's')
                // Extract name from %<name>X
                let re = regex::Regex::new(r"^%<(\w+)>[a-zA-Z]$").unwrap();
                if let Some(caps) = re.captures(token_str) {
                    let name = caps.get(1).unwrap().as_str();
                    Some(format!("%{{{}}}", name))
                } else {
                    None
                }
            }
            (TokenKind::Unannotated, EnforcedStyle::Annotated) => {
                // Can't convert unannotated to annotated (no name available)
                None
            }
            (TokenKind::Unannotated, EnforcedStyle::Template) => {
                // Can't convert unannotated to template (no name available)
                None
            }
            _ => None,
        }
    }

    /// Check if a method name is in the allowed list
    fn is_allowed_method(&self, method_name: &str) -> bool {
        if self.allowed_methods.iter().any(|m| m == method_name) {
            return true;
        }
        self.allowed_patterns.iter().any(|p| p.is_match(method_name))
    }
}

impl Default for FormatStringToken {
    fn default() -> Self {
        Self::new(EnforcedStyle::Annotated)
    }
}

impl Cop for FormatStringToken {
    fn name(&self) -> &'static str {
        "Style/FormatStringToken"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = FormatTokenVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            in_xstr_or_regexp: false,
            call_stack: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Info about a call node in the call stack
struct CallInfo {
    method_name: String,
    /// Whether this call is format/sprintf/printf
    is_format_call: bool,
    /// Whether this call uses the % operator on a string
    is_percent_call: bool,
    /// Byte range of the first argument (the format string)
    first_arg_range: Option<(usize, usize)>,
    /// Byte range of the receiver (for % operator)
    receiver_range: Option<(usize, usize)>,
    /// Byte range of the entire call node (for determining containment)
    call_range: (usize, usize),
}

/// Visitor that walks the AST collecting format token offenses
struct FormatTokenVisitor<'a> {
    cop: &'a FormatStringToken,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_xstr_or_regexp: bool,
    call_stack: Vec<CallInfo>,
}

impl<'a> FormatTokenVisitor<'a> {
    /// Check if a string node (by its byte range) is in a "format context"
    /// i.e., it's the first arg to format/sprintf/printf or the receiver of %
    fn is_in_format_context(&self, str_start: usize, str_end: usize) -> bool {
        for info in self.call_stack.iter().rev() {
            if info.is_format_call {
                if let Some((arg_start, arg_end)) = info.first_arg_range {
                    if str_start >= arg_start && str_end <= arg_end {
                        return true;
                    }
                }
            }
            if info.is_percent_call {
                if let Some((recv_start, recv_end)) = info.receiver_range {
                    if str_start >= recv_start && str_end <= recv_end {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if the nearest ancestor call is in AllowedMethods/AllowedPatterns.
    /// RuboCop checks only the first (nearest) :send ancestor.
    fn is_in_allowed_method_for_range(&self, str_start: usize, str_end: usize) -> bool {
        // Find the nearest call that contains this string range
        for info in self.call_stack.iter().rev() {
            if str_start >= info.call_range.0 && str_end <= info.call_range.1 {
                return self.cop.is_allowed_method(&info.method_name);
            }
        }
        false
    }

    /// Process a string's content for format tokens
    fn check_string_content(
        &mut self,
        content: &str,
        content_start_offset: usize,
        content_end_offset: usize,
    ) {
        if !content.contains('%') {
            return;
        }

        // Find all format tokens
        let tokens = self.cop.find_format_tokens(content);
        if tokens.is_empty() {
            return;
        }

        // Filter tokens: remove those matching enforced style, and apply allowed_string? check
        let in_format_ctx = self.is_in_format_context(content_start_offset, content_end_offset);

        let mut detections: Vec<&FormatToken> = Vec::new();
        for token in &tokens {
            // Skip tokens that already match the enforced style
            if self.cop.matches_enforced_style(token.kind) {
                continue;
            }

            // allowed_string? check: skip unannotated (or all in conservative mode)
            // if not in format context
            let is_allowed = (token.kind == TokenKind::Unannotated || self.cop.conservative)
                && !in_format_ctx;
            if is_allowed {
                continue;
            }

            detections.push(token);
        }

        if detections.is_empty() {
            return;
        }

        // allowed_unannotated? check
        if detections.iter().all(|t| t.kind == TokenKind::Unannotated) {
            if detections.len() <= self.cop.max_unannotated_placeholders {
                return;
            }
            // Also skip if any token has a non-correctable type
            if detections
                .iter()
                .any(|t| !self.cop.correctable_sequence(t.type_char))
            {
                return;
            }
        }

        // Generate per-token offenses
        for token in &detections {
            let start = content_start_offset + token.byte_offset;
            let end = start + token.byte_length;
            let message = self.cop.message(token.kind);

            let mut offense = self.ctx.offense_with_range(
                self.cop.name(),
                &message,
                self.cop.severity(),
                start,
                end,
            );
            if let Some(corrected) = self.cop.corrected_token(token, content) {
                offense = offense.with_correction(Correction::replace(start, end, corrected));
            }
            self.offenses.push(offense);
        }
    }

    /// Process a StringNode
    fn process_string_node(&mut self, node: &ruby_prism::StringNode) {
        if self.in_xstr_or_regexp {
            return;
        }

        // Skip __FILE__
        let loc = node.location();
        let node_source = self
            .ctx
            .source
            .get(loc.start_offset()..loc.end_offset())
            .unwrap_or("");
        if node_source == "__FILE__" {
            return;
        }

        // Skip if nearest ancestor call is in AllowedMethods
        let node_start = loc.start_offset();
        let node_end = loc.end_offset();
        if self.is_in_allowed_method_for_range(node_start, node_end) {
            return;
        }

        let content_loc = node.content_loc();
        let content_start = content_loc.start_offset();
        let content_end = content_loc.end_offset();
        let content = self
            .ctx
            .source
            .get(content_start..content_end)
            .unwrap_or("");

        self.check_string_content(content, content_start, content_end);
    }

    /// Process an InterpolatedStringNode's parts
    fn process_interpolated_string_parts(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if self.in_xstr_or_regexp {
            return;
        }

        // Check if the interpolated string itself is in format context
        let node_loc = node.location();
        let node_start = node_loc.start_offset();
        let node_end = node_loc.end_offset();

        // Skip if nearest ancestor call is in AllowedMethods
        if self.is_in_allowed_method_for_range(node_start, node_end) {
            return;
        }

        let in_format_ctx = self.is_in_format_context(node_start, node_end);

        // Collect all tokens from all string parts first, then apply filtering
        let mut all_tokens: Vec<(FormatToken, usize)> = Vec::new(); // (token, content_start_offset)

        for part in node.parts().iter() {
            if let ruby_prism::Node::StringNode { .. } = &part {
                let str_node = part.as_string_node().unwrap();
                let content_loc = str_node.content_loc();
                let content_start = content_loc.start_offset();
                let content_end = content_loc.end_offset();
                let content = self
                    .ctx
                    .source
                    .get(content_start..content_end)
                    .unwrap_or("");

                if content.contains('%') {
                    let tokens = self.cop.find_format_tokens(content);
                    for token in tokens {
                        all_tokens.push((token, content_start));
                    }
                }
            }
        }

        if all_tokens.is_empty() {
            return;
        }

        // Filter tokens
        let mut detections: Vec<(&FormatToken, usize)> = Vec::new();
        for (token, content_start) in &all_tokens {
            if self.cop.matches_enforced_style(token.kind) {
                continue;
            }

            let is_allowed = (token.kind == TokenKind::Unannotated || self.cop.conservative)
                && !in_format_ctx;
            if is_allowed {
                continue;
            }

            detections.push((token, *content_start));
        }

        if detections.is_empty() {
            return;
        }

        // allowed_unannotated? check across all parts
        if detections.iter().all(|(t, _)| t.kind == TokenKind::Unannotated) {
            if detections.len() <= self.cop.max_unannotated_placeholders {
                return;
            }
            if detections
                .iter()
                .any(|(t, _)| !self.cop.correctable_sequence(t.type_char))
            {
                return;
            }
        }

        // Generate per-token offenses
        for (token, content_start) in &detections {
            let start = content_start + token.byte_offset;
            let end = start + token.byte_length;
            let message = self.cop.message(token.kind);

            // Get the content slice for this token's string part
            let content = self.ctx.source.get(*content_start..).unwrap_or("");
            let mut offense = self.ctx.offense_with_range(
                self.cop.name(),
                &message,
                self.cop.severity(),
                start,
                end,
            );
            if let Some(corrected) = self.cop.corrected_token(token, content) {
                offense = offense.with_correction(Correction::replace(start, end, corrected));
            }
            self.offenses.push(offense);
        }
    }
}

impl Visit<'_> for FormatTokenVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        let is_format_call = matches!(
            method_name.as_str(),
            "format" | "sprintf" | "printf"
        );
        let is_percent_call = method_name == "%";

        // Get first argument range for format calls
        let first_arg_range = if is_format_call {
            node.arguments().and_then(|args| {
                let arg_list = args.arguments();
                if !arg_list.is_empty() {
                    let first_arg = arg_list.iter().next().unwrap();
                    let loc = first_arg.location();
                    Some((loc.start_offset(), loc.end_offset()))
                } else {
                    None
                }
            })
        } else {
            None
        };

        // Get receiver range for % operator
        let receiver_range = if is_percent_call {
            node.receiver().map(|recv| {
                let loc = recv.location();
                (loc.start_offset(), loc.end_offset())
            })
        } else {
            None
        };

        let call_loc = node.location();
        let info = CallInfo {
            method_name,
            is_format_call,
            is_percent_call,
            first_arg_range,
            receiver_range,
            call_range: (call_loc.start_offset(), call_loc.end_offset()),
        };

        self.call_stack.push(info);
        ruby_prism::visit_call_node(self, node);
        self.call_stack.pop();
    }

    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        self.process_string_node(node);
        // Don't recurse into string nodes (they have no children)
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        // Process the interpolated string parts ourselves
        self.process_interpolated_string_parts(node);

        // Still need to visit embedded statement parts for nested calls
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                let embedded = part.as_embedded_statements_node().unwrap();
                if let Some(stmts) = embedded.statements() {
                    self.visit_statements_node(&stmts);
                }
            }
        }
    }

    fn visit_x_string_node(&mut self, _node: &ruby_prism::XStringNode) {
        // Don't process xstr content - skip entirely
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        // Don't process xstr content, but still visit embedded statements
        // for any nested code that might contain format strings
        let prev = self.in_xstr_or_regexp;
        self.in_xstr_or_regexp = true;
        ruby_prism::visit_interpolated_x_string_node(self, node);
        self.in_xstr_or_regexp = prev;
    }

    fn visit_regular_expression_node(&mut self, _node: &ruby_prism::RegularExpressionNode) {
        // Don't process regexp content - skip entirely
    }

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        // Don't process regexp content, but still visit embedded statements
        let prev = self.in_xstr_or_regexp;
        self.in_xstr_or_regexp = true;
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
        self.in_xstr_or_regexp = prev;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check_with_style(source: &str, style: EnforcedStyle) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(FormatStringToken::new(style));
        let cops = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops(&cops, &result, source, "test.rb")
    }

    fn check(source: &str) -> Vec<Offense> {
        check_with_style(source, EnforcedStyle::Annotated)
    }

    #[test]
    fn annotated_allows_annotated_tokens() {
        let offenses = check("format('%<greeting>s', greeting: 'Hello')");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn annotated_flags_template_tokens() {
        let offenses = check("format('%{greeting}', greeting: 'Hello')");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("annotated"));
    }

    #[test]
    fn annotated_flags_unannotated_in_format_context() {
        let offenses = check("format('%s', 'Hello')");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn template_allows_template_tokens() {
        let offenses = check_with_style(
            "format('%{greeting}', greeting: 'Hello')",
            EnforcedStyle::Template,
        );
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn skips_percent_escape() {
        let offenses = check("format('%<hit_rate>6.2f%%', hit_rate: 12.34)");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_strings_without_format_tokens() {
        let offenses = check("'hello world'");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn skips_xstr() {
        let offenses = check("`echo \"%s %<annotated>s %{template}\"`");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn skips_regexp() {
        let offenses = check("/foo bar %u/");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn per_token_offenses() {
        let offenses = check("format('%-20s %-30s', 'foo', 'bar')");
        assert_eq!(offenses.len(), 2);
    }
}
