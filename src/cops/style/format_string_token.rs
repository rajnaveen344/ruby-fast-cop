//! Style/FormatStringToken cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnforcedStyle {
    Annotated,
    Template,
    Unannotated,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TokenKind {
    Annotated,
    Template,
    Unannotated,
}

#[derive(Debug)]
struct FormatToken {
    byte_offset: usize,
    byte_length: usize,
    kind: TokenKind,
    type_char: char,
}

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

    fn find_format_tokens(&self, content: &str) -> Vec<FormatToken> {
        let mut tokens = Vec::new();
        for cap in self.token_regex.captures_iter(content) {
            let m = cap.get(0).unwrap();
            if m.as_str() == "%%" { continue; }

            let (kind, type_char) = if cap.get(1).is_some() {
                (TokenKind::Annotated, cap.get(2).unwrap().as_str().chars().next().unwrap())
            } else if cap.get(3).is_some() {
                (TokenKind::Template, 's')
            } else if cap.get(4).is_some() {
                (TokenKind::Unannotated, cap.get(4).unwrap().as_str().chars().next().unwrap())
            } else {
                continue;
            };
            tokens.push(FormatToken { byte_offset: m.start(), byte_length: m.len(), kind, type_char });
        }
        tokens
    }

    fn matches_enforced_style(&self, kind: TokenKind) -> bool {
        matches!(
            (self.enforced_style, kind),
            (EnforcedStyle::Annotated, TokenKind::Annotated)
                | (EnforcedStyle::Template, TokenKind::Template)
                | (EnforcedStyle::Unannotated, TokenKind::Unannotated)
        )
    }

    fn correctable_sequence(&self, type_char: char) -> bool {
        match self.enforced_style {
            EnforcedStyle::Template => type_char == 's',
            _ => true,
        }
    }

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

    fn corrected_token(&self, token: &FormatToken, content: &str) -> Option<String> {
        let token_str = &content[token.byte_offset..token.byte_offset + token.byte_length];
        match (token.kind, self.enforced_style) {
            (TokenKind::Template, EnforcedStyle::Annotated) => {
                token_str.strip_prefix("%{").and_then(|s| s.strip_suffix('}')).map(|name| format!("%<{}>s", name))
            }
            (TokenKind::Annotated, EnforcedStyle::Template) => {
                let re = regex::Regex::new(r"^%<(\w+)>[a-zA-Z]$").unwrap();
                re.captures(token_str).map(|caps| format!("%{{{}}}", caps.get(1).unwrap().as_str()))
            }
            _ => None,
        }
    }

    fn is_allowed_method(&self, method_name: &str) -> bool {
        self.allowed_methods.iter().any(|m| m == method_name)
            || self.allowed_patterns.iter().any(|p| p.is_match(method_name))
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

struct CallInfo {
    method_name: String,
    is_format_call: bool,
    is_percent_call: bool,
    first_arg_range: Option<(usize, usize)>,
    receiver_range: Option<(usize, usize)>,
    call_range: (usize, usize),
}

struct FormatTokenVisitor<'a> {
    cop: &'a FormatStringToken,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_xstr_or_regexp: bool,
    call_stack: Vec<CallInfo>,
}

impl<'a> FormatTokenVisitor<'a> {
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

    fn is_in_allowed_method_for_range(&self, str_start: usize, str_end: usize) -> bool {
        for info in self.call_stack.iter().rev() {
            if str_start >= info.call_range.0 && str_end <= info.call_range.1 {
                return self.cop.is_allowed_method(&info.method_name);
            }
        }
        false
    }

    fn check_string_content(
        &mut self,
        content: &str,
        content_start_offset: usize,
        content_end_offset: usize,
    ) {
        if !content.contains('%') { return; }

        let tokens = self.cop.find_format_tokens(content);
        if tokens.is_empty() { return; }

        let in_format_ctx = self.is_in_format_context(content_start_offset, content_end_offset);
        let detections: Vec<&FormatToken> = tokens.iter()
            .filter(|t| !self.cop.matches_enforced_style(t.kind))
            .filter(|t| !((t.kind == TokenKind::Unannotated || self.cop.conservative) && !in_format_ctx))
            .collect();

        if detections.is_empty() { return; }

        if detections.iter().all(|t| t.kind == TokenKind::Unannotated) {
            if detections.len() <= self.cop.max_unannotated_placeholders { return; }
            if detections.iter().any(|t| !self.cop.correctable_sequence(t.type_char)) { return; }
        }

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

    fn process_string_node(&mut self, node: &ruby_prism::StringNode) {
        if self.in_xstr_or_regexp { return; }

        let loc = node.location();
        let node_source = self.ctx.source.get(loc.start_offset()..loc.end_offset()).unwrap_or("");
        if node_source == "__FILE__" { return; }
        if self.is_in_allowed_method_for_range(loc.start_offset(), loc.end_offset()) { return; }

        let content_loc = node.content_loc();
        let content = self.ctx.source.get(content_loc.start_offset()..content_loc.end_offset()).unwrap_or("");
        self.check_string_content(content, content_loc.start_offset(), content_loc.end_offset());
    }

    fn process_interpolated_string_parts(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        if self.in_xstr_or_regexp { return; }

        let node_loc = node.location();
        let (node_start, node_end) = (node_loc.start_offset(), node_loc.end_offset());
        if self.is_in_allowed_method_for_range(node_start, node_end) { return; }

        let in_format_ctx = self.is_in_format_context(node_start, node_end);

        let mut all_tokens: Vec<(FormatToken, usize)> = Vec::new();
        for part in node.parts().iter() {
            if let ruby_prism::Node::StringNode { .. } = &part {
                let str_node = part.as_string_node().unwrap();
                let content_loc = str_node.content_loc();
                let content = self.ctx.source.get(content_loc.start_offset()..content_loc.end_offset()).unwrap_or("");
                if content.contains('%') {
                    for token in self.cop.find_format_tokens(content) {
                        all_tokens.push((token, content_loc.start_offset()));
                    }
                }
            }
        }
        if all_tokens.is_empty() { return; }

        let detections: Vec<(&FormatToken, usize)> = all_tokens.iter()
            .filter(|(t, _)| !self.cop.matches_enforced_style(t.kind))
            .filter(|(t, _)| !((t.kind == TokenKind::Unannotated || self.cop.conservative) && !in_format_ctx))
            .map(|(t, cs)| (t, *cs))
            .collect();

        if detections.is_empty() { return; }

        if detections.iter().all(|(t, _)| t.kind == TokenKind::Unannotated) {
            if detections.len() <= self.cop.max_unannotated_placeholders { return; }
            if detections.iter().any(|(t, _)| !self.cop.correctable_sequence(t.type_char)) { return; }
        }

        for (token, content_start) in &detections {
            let start = content_start + token.byte_offset;
            let end = start + token.byte_length;
            let message = self.cop.message(token.kind);
            let content = self.ctx.source.get(*content_start..).unwrap_or("");
            let mut offense = self.ctx.offense_with_range(self.cop.name(), &message, self.cop.severity(), start, end);
            if let Some(corrected) = self.cop.corrected_token(token, content) {
                offense = offense.with_correction(Correction::replace(start, end, corrected));
            }
            self.offenses.push(offense);
        }
    }
}

impl Visit<'_> for FormatTokenVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = node_name!(node).to_string();

        let is_format_call = matches!(
            method_name.as_str(),
            "format" | "sprintf" | "printf"
        );
        let is_percent_call = method_name == "%";

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
    }

    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        self.process_interpolated_string_parts(node);
        for part in node.parts().iter() {
            if let ruby_prism::Node::EmbeddedStatementsNode { .. } = &part {
                let embedded = part.as_embedded_statements_node().unwrap();
                if let Some(stmts) = embedded.statements() {
                    self.visit_statements_node(&stmts);
                }
            }
        }
    }

    fn visit_x_string_node(&mut self, _node: &ruby_prism::XStringNode) {}

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        let prev = self.in_xstr_or_regexp;
        self.in_xstr_or_regexp = true;
        ruby_prism::visit_interpolated_x_string_node(self, node);
        self.in_xstr_or_regexp = prev;
    }

    fn visit_regular_expression_node(&mut self, _node: &ruby_prism::RegularExpressionNode) {}

    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        let prev = self.in_xstr_or_regexp;
        self.in_xstr_or_regexp = true;
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
        self.in_xstr_or_regexp = prev;
    }
}

crate::register_cop!("Style/FormatStringToken", |cfg| {
    let cop_config = cfg.get_cop_config("Style/FormatStringToken");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "template" => EnforcedStyle::Template,
            "unannotated" => EnforcedStyle::Unannotated,
            _ => EnforcedStyle::Annotated,
        })
        .unwrap_or(EnforcedStyle::Annotated);
    let max_unannotated = cop_config
        .and_then(|c| c.raw.get("MaxUnannotatedPlaceholdersAllowed"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let conservative = cop_config
        .and_then(|c| c.raw.get("Mode"))
        .and_then(|v| v.as_str())
        .map(|s| s == "conservative")
        .unwrap_or(false);
    let allowed_methods = cop_config
        .and_then(|c| c.raw.get("AllowedMethods"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
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
    Some(Box::new(FormatStringToken::with_config(
        style,
        max_unannotated,
        conservative,
        allowed_methods,
        allowed_patterns,
    )))
});
