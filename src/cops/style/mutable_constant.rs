//! Style/MutableConstant - Checks whether some constant value isn't a mutable literal.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/mutable_constant.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Freeze mutable objects assigned to constants.";

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Literals,
    Strict,
}

pub struct MutableConstant {
    enforced_style: EnforcedStyle,
}

impl MutableConstant {
    pub fn new(style: EnforcedStyle) -> Self {
        Self {
            enforced_style: style,
        }
    }

    /// Parse a `shareable_constant_value` magic comment value.
    fn parse_shareable_constant_value(line: &str) -> Option<String> {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            return None;
        }
        let content = trimmed[1..].trim();
        if let Some((key, val)) = content.split_once(':') {
            let key_trimmed = key.trim();
            if key_trimmed == "shareable_constant_value" {
                return Some(val.trim().to_string());
            }
        }
        None
    }

    /// Check if a shareable_constant_value is an "enabled" value
    fn shareable_constant_value_enabled(value: &str) -> bool {
        matches!(
            value,
            "literal" | "experimental_everything" | "experimental_copy"
        )
    }

    /// Find the most recent shareable_constant_value magic comment that's in scope
    /// for a given line number. Returns true if the most recent value is enabled.
    fn shareable_constant_value_active(source: &str, line_number: u32, ruby_version: f64) -> bool {
        if ruby_version < 3.0 {
            return false;
        }

        let mut most_recent_value: Option<String> = None;

        for (i, line) in source.lines().enumerate() {
            if (i + 1) as u32 > line_number {
                break;
            }
            if let Some(value) = Self::parse_shareable_constant_value(line) {
                most_recent_value = Some(value);
            }
        }

        match most_recent_value {
            Some(value) => Self::shareable_constant_value_enabled(&value),
            None => false,
        }
    }

    /// Parse the frozen_string_literal comment value from source.
    fn frozen_string_literal_value(source: &str) -> Option<bool> {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !trimmed.starts_with('#') {
                break;
            }
            let content = trimmed[1..].trim();
            // Check emacs-style
            if content.starts_with("-*-") && content.ends_with("-*-") {
                let inner = content[3..content.len() - 3].trim();
                for part in inner.split(';') {
                    let part = part.trim();
                    if let Some((key, val)) = part.split_once(':') {
                        let key_normalized = key.trim().to_lowercase().replace(['-', '_'], "");
                        if key_normalized == "frozenstringliteral" {
                            return Some(val.trim().eq_ignore_ascii_case("true"));
                        }
                    }
                }
                continue;
            }
            // Standard format
            if let Some((key, val)) = content.split_once(':') {
                let key_normalized = key.trim().to_lowercase().replace(['-', '_'], "");
                if key_normalized == "frozenstringliteral" {
                    return Some(val.trim().eq_ignore_ascii_case("true"));
                }
            }
        }
        None
    }

    /// Check if a ParenthesesNode wraps a single RangeNode.
    /// Prism wraps the paren body in a StatementsNode, so we need to unwrap it.
    fn paren_wraps_range(node: &Node) -> bool {
        if let Some(paren) = node.as_parentheses_node() {
            if let Some(body) = paren.body() {
                // Prism wraps the paren body in a StatementsNode
                if let Some(stmts) = body.as_statements_node() {
                    let items: Vec<_> = stmts.body().iter().collect();
                    if items.len() == 1 {
                        return matches!(items[0], Node::RangeNode { .. });
                    }
                }
                // Fallback: body might be the node directly
                return matches!(body, Node::RangeNode { .. });
            }
        }
        false
    }

    /// Check if a value node is a mutable literal (for "literals" style).
    fn is_mutable_literal(node: &Node, ruby_version: f64) -> bool {
        match node {
            Node::ArrayNode { .. } => true,
            Node::HashNode { .. } => true,
            Node::StringNode { .. } => true,
            Node::InterpolatedStringNode { .. } => true,
            Node::XStringNode { .. } => true,
            Node::InterpolatedXStringNode { .. } => true,
            Node::RegularExpressionNode { .. } => ruby_version < 3.0,
            Node::InterpolatedRegularExpressionNode { .. } => ruby_version < 3.0,
            Node::RangeNode { .. } => ruby_version < 3.0,
            Node::ParenthesesNode { .. } => {
                Self::paren_wraps_range(node) && ruby_version < 3.0
            }
            Node::SplatNode { .. } => true,
            _ => false,
        }
    }

    /// Check if a value is an immutable literal.
    fn is_immutable_literal(node: &Node, ruby_version: f64) -> bool {
        match node {
            Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::NilNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. } => true,
            Node::RegularExpressionNode { .. }
            | Node::InterpolatedRegularExpressionNode { .. } => ruby_version >= 3.0,
            Node::RangeNode { .. } => ruby_version >= 3.0,
            Node::ParenthesesNode { .. } => {
                Self::paren_wraps_range(node) && ruby_version >= 3.0
            }
            _ => false,
        }
    }

    /// Check if the value is a node that has `.freeze` called on it.
    fn is_frozen(node: &Node) -> bool {
        if let Some(call) = node.as_call_node() {
            let method_name = String::from_utf8_lossy(call.name().as_slice());
            return method_name == "freeze";
        }
        false
    }

    /// Check if a call node represents an operation that produces an immutable object.
    fn operation_produces_immutable_object(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => true,

            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method_name = String::from_utf8_lossy(call.name().as_slice());

                if method_name == "freeze" {
                    return true;
                }

                // Struct.new(...) or Struct.new(...) do ... end
                // In Prism, the block is part of the CallNode (block field)
                if method_name == "new" {
                    if let Some(receiver) = call.receiver() {
                        if Self::is_struct_const(&receiver) {
                            return true;
                        }
                    }
                }

                if matches!(method_name.as_ref(), "count" | "length" | "size") {
                    return true;
                }

                if matches!(
                    method_name.as_ref(),
                    "+" | "-" | "*" | "**" | "/" | "%" | "<<" | "==" | "===" | "!=" | "<=" | ">=" | "<" | ">"
                ) {
                    if let Some(receiver) = call.receiver() {
                        if Self::is_numeric_literal(&receiver) {
                            return true;
                        }
                    }
                    if let Some(args) = call.arguments() {
                        for arg in args.arguments().iter() {
                            if Self::is_numeric_literal(&arg) {
                                return true;
                            }
                        }
                    }
                    if matches!(
                        method_name.as_ref(),
                        "==" | "===" | "!=" | "<=" | ">=" | "<" | ">"
                    ) {
                        return true;
                    }
                }

                if method_name == "[]" {
                    if let Some(receiver) = call.receiver() {
                        return Self::is_env_const(&receiver);
                    }
                }

                false
            }

            Node::OrNode { .. } => {
                let or_node = node.as_or_node().unwrap();
                let left = or_node.left();
                if let Some(call) = left.as_call_node() {
                    let method_name = String::from_utf8_lossy(call.name().as_slice());
                    if method_name == "[]" {
                        if let Some(receiver) = call.receiver() {
                            return Self::is_env_const(&receiver);
                        }
                    }
                }
                false
            }

            _ => false,
        }
    }

    fn is_struct_const(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let c = node.as_constant_read_node().unwrap();
                String::from_utf8_lossy(c.name().as_slice()) == "Struct"
            }
            Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                let name = cp
                    .name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()).to_string())
                    .unwrap_or_default();
                name == "Struct" && cp.parent().is_none()
            }
            _ => false,
        }
    }

    fn is_env_const(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let c = node.as_constant_read_node().unwrap();
                String::from_utf8_lossy(c.name().as_slice()) == "ENV"
            }
            Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                let name = cp
                    .name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()).to_string())
                    .unwrap_or_default();
                name == "ENV" && cp.parent().is_none()
            }
            _ => false,
        }
    }

    fn is_numeric_literal(node: &Node) -> bool {
        matches!(node, Node::IntegerNode { .. } | Node::FloatNode { .. })
    }

    /// Check if a node has actual interpolation (embedded expressions like `#{...}`).
    /// An `InterpolatedStringNode` can represent simple string concat ('foo' 'bar')
    /// without any real interpolation. We check recursively because multiline string
    /// concat with interpolation (e.g., `"#{foo}" \ 'bar'`) nests the interpolated part
    /// inside another InterpolatedStringNode.
    fn has_real_interpolation(node: &Node) -> bool {
        match node {
            Node::InterpolatedStringNode { .. } => {
                let interp = node.as_interpolated_string_node().unwrap();
                for part in interp.parts().iter() {
                    if matches!(part, Node::EmbeddedStatementsNode { .. }) {
                        return true;
                    }
                    // Recurse into nested InterpolatedStringNode parts
                    if Self::has_real_interpolation(&part) {
                        return true;
                    }
                }
                false
            }
            Node::InterpolatedXStringNode { .. } => {
                let interp = node.as_interpolated_x_string_node().unwrap();
                for part in interp.parts().iter() {
                    if matches!(part, Node::EmbeddedStatementsNode { .. }) {
                        return true;
                    }
                }
                false
            }
            Node::InterpolatedRegularExpressionNode { .. } => {
                let interp = node.as_interpolated_regular_expression_node().unwrap();
                for part in interp.parts().iter() {
                    if matches!(part, Node::EmbeddedStatementsNode { .. }) {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Check if this is a heredoc node
    fn is_heredoc(node: &Node, source: &str) -> bool {
        let start = node_start_offset(node);
        let src_at = &source[start..];
        match node {
            Node::InterpolatedStringNode { .. } | Node::StringNode { .. } => {
                src_at.starts_with("<<")
            }
            _ => false,
        }
    }

    /// Detect multiline string concatenation (adjacent strings).
    fn is_string_concat(node: &Node, source: &str) -> bool {
        let start = node_start_offset(node);
        let end = node_end_offset(node);
        if start >= end || end > source.len() {
            return false;
        }
        let text = &source[start..end];
        match node {
            Node::InterpolatedStringNode { .. } | Node::StringNode { .. } => {
                text.contains("\\\n") || {
                    // Check for adjacent string pattern: 'foo'  'bar'
                    // Look for quote-space-quote patterns
                    let mut found_quote = false;
                    let mut found_space_after_quote = false;
                    for ch in text.chars() {
                        if found_quote {
                            if ch == ' ' || ch == '\t' {
                                found_space_after_quote = true;
                            } else if found_space_after_quote && (ch == '\'' || ch == '"') {
                                return true;
                            } else {
                                found_quote = false;
                                found_space_after_quote = false;
                            }
                        }
                        if ch == '\'' || ch == '"' {
                            found_quote = true;
                            found_space_after_quote = false;
                        }
                    }
                    false
                }
            }
            _ => false,
        }
    }

    /// Check if a value node should be flagged for the "literals" style.
    fn check_literals(&self, value: &Node, ctx: &CheckContext) -> Option<Offense> {
        let ruby_version = ctx.target_ruby_version;

        let is_paren_range = Self::paren_wraps_range(value);

        let is_mutable = Self::is_mutable_literal(value, ruby_version);
        let is_range_in_parens = is_paren_range && ruby_version <= 2.7;

        if !is_mutable && !is_range_in_parens {
            return None;
        }

        // Check frozen_string_literal for string types
        if self.is_frozen_by_magic_comment(value, ctx) {
            return None;
        }

        let start = node_start_offset(value);
        let end = node_end_offset(value);

        // For multiline values, cap the offense end to the end of the first line
        // (RuboCop reports offenses on the first line only for multiline expressions)
        let offense_end = first_line_end(ctx.source, start, end);

        let offense = ctx.offense_with_range(
            "Style/MutableConstant",
            MSG,
            Severity::Convention,
            start,
            offense_end,
        );

        let correction = self.build_correction(value, ctx.source, start, end);
        Some(offense.with_correction(correction))
    }

    /// Check if a value node should be flagged for the "strict" style.
    fn check_strict(&self, value: &Node, ctx: &CheckContext) -> Option<Offense> {
        let ruby_version = ctx.target_ruby_version;

        if Self::is_immutable_literal(value, ruby_version) {
            return None;
        }

        if Self::operation_produces_immutable_object(value) {
            return None;
        }

        if self.is_frozen_by_magic_comment(value, ctx) {
            return None;
        }

        let start = node_start_offset(value);
        let end = node_end_offset(value);

        // For multiline values, cap the offense end to the end of the first line
        let offense_end = first_line_end(ctx.source, start, end);

        let offense = ctx.offense_with_range(
            "Style/MutableConstant",
            MSG,
            Severity::Convention,
            start,
            offense_end,
        );

        let correction = self.build_correction(value, ctx.source, start, end);
        Some(offense.with_correction(correction))
    }

    /// Check if the value is frozen by the frozen_string_literal magic comment.
    fn is_frozen_by_magic_comment(&self, value: &Node, ctx: &CheckContext) -> bool {
        let ruby_version = ctx.target_ruby_version;

        match value {
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => {
                let has_real_interp = Self::has_real_interpolation(value);

                // Heredocs
                if Self::is_heredoc(value, ctx.source) {
                    if ruby_version >= 3.0 && has_real_interp {
                        return false;
                    }
                    return Self::frozen_string_literal_value(ctx.source) == Some(true);
                }

                // Multiline string concat or adjacent strings
                if Self::is_string_concat(value, ctx.source) {
                    if ruby_version >= 3.0 && has_real_interp {
                        return false;
                    }
                    return Self::frozen_string_literal_value(ctx.source) == Some(true);
                }

                // Regular string with real interpolation (#{...})
                if has_real_interp {
                    if ruby_version >= 3.0 {
                        return false;
                    }
                    return Self::frozen_string_literal_value(ctx.source) == Some(true);
                }

                // Plain string (StringNode or InterpolatedStringNode without interpolation)
                // These are frozen by frozen_string_literal: true
                // Note: StringNode is already immutable with fsl:true, but we won't get here
                // for plain StringNode since it's not mutable. InterpolatedStringNode without
                // real interpolation (like adjacent string concat) is handled above.
                false
            }
            _ => false,
        }
    }

    /// Build the autocorrect correction for a mutable constant value.
    fn build_correction(
        &self,
        node: &Node,
        source: &str,
        start: usize,
        end: usize,
    ) -> Correction {
        // Check for splat expansion
        if let Some(splat_correction) = self.correct_splat(node, source, start, end) {
            return splat_correction;
        }

        // Check for unbracketed array
        if let Some(_arr) = node.as_array_node() {
            let src = &source[start..end];
            if !src.starts_with('[') && !src.starts_with('%') {
                let replacement = format!("[{}].freeze", src);
                return Correction::replace(start, end, replacement);
            }
        }

        // Check if range/operator needs parentheses
        if self.requires_parentheses(node, source) {
            let src = &source[start..end];
            let replacement = format!("({}).freeze", src);
            return Correction::replace(start, end, replacement);
        }

        // Standard: append .freeze
        Correction::insert(end, ".freeze")
    }

    /// Correct splat expansion: *1..10 -> (1..10).to_a.freeze
    fn correct_splat(
        &self,
        node: &Node,
        source: &str,
        start: usize,
        end: usize,
    ) -> Option<Correction> {
        if let Some(arr) = node.as_array_node() {
            let elements: Vec<_> = arr.elements().iter().collect();
            if elements.len() == 1 {
                if let Some(splat) = elements[0].as_splat_node() {
                    if let Some(inner) = splat.expression() {
                        // Check if it's a parenthesized range: *(1..10)
                        let is_paren_range = Self::paren_wraps_range(&inner);

                        if matches!(inner, Node::RangeNode { .. }) {
                            // Bare range: *1..10 -> (1..10).to_a.freeze
                            let inner_start = node_start_offset(&inner);
                            let inner_end = node_end_offset(&inner);
                            let inner_src = &source[inner_start..inner_end];
                            let replacement = format!("({}).to_a.freeze", inner_src);
                            return Some(Correction::replace(start, end, replacement));
                        } else if is_paren_range {
                            // Parenthesized range: *(1..10) -> (1..10).to_a.freeze
                            let inner_start = node_start_offset(&inner);
                            let inner_end = node_end_offset(&inner);
                            let inner_src = &source[inner_start..inner_end];
                            let replacement = format!("{}.to_a.freeze", inner_src);
                            return Some(Correction::replace(start, end, replacement));
                        }
                    }
                }
            }
        }
        None
    }

    /// Check if a node requires parentheses when adding .freeze
    fn requires_parentheses(&self, node: &Node, _source: &str) -> bool {
        match node {
            Node::RangeNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method_name = String::from_utf8_lossy(call.name().as_slice());
                if call.call_operator_loc().is_none() {
                    if matches!(
                        method_name.as_ref(),
                        "+" | "-" | "*" | "**" | "/" | "%" | "<<" | ">>"
                    ) {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }
}

/// Get start offset for any Node
fn node_start_offset(node: &Node) -> usize {
    node.location().start_offset()
}

/// Get end offset for any Node
fn node_end_offset(node: &Node) -> usize {
    node.location().end_offset()
}

/// For multiline values, cap the end offset to the end of the first line.
/// RuboCop reports the offense location on the first line only for multiline expressions.
/// For single-line values, returns `end` unchanged.
fn first_line_end(source: &str, start: usize, end: usize) -> usize {
    // Check if there's a newline between start and end
    if let Some(newline_pos) = source[start..end].find('\n') {
        // End at the newline (or the backslash-newline continuation)
        let first_line_end = start + newline_pos;
        // Trim trailing whitespace from the first line portion for the offense range
        let first_line = &source[start..first_line_end];
        let trimmed_len = first_line.trim_end().len();
        start + trimmed_len
    } else {
        end
    }
}

/// Visitor that finds constant assignments
struct MutableConstantVisitor<'a> {
    cop: &'a MutableConstant,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_def: bool,
}

impl<'a> MutableConstantVisitor<'a> {
    fn new(cop: &'a MutableConstant, ctx: &'a CheckContext<'a>) -> Self {
        Self {
            cop,
            ctx,
            offenses: Vec::new(),
            in_def: false,
        }
    }

    fn check_assignment(&mut self, value: &Node, line_number: u32) {
        if self.in_def {
            return;
        }

        if MutableConstant::is_frozen(value) {
            return;
        }

        if MutableConstant::shareable_constant_value_active(
            self.ctx.source,
            line_number,
            self.ctx.target_ruby_version,
        ) {
            return;
        }

        let offense = match &self.cop.enforced_style {
            EnforcedStyle::Literals => self.cop.check_literals(value, self.ctx),
            EnforcedStyle::Strict => self.cop.check_strict(value, self.ctx),
        };

        if let Some(o) = offense {
            self.offenses.push(o);
        }
    }
}

impl Visit<'_> for MutableConstantVisitor<'_> {
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let value = node.value();
        let loc = crate::offense::Location::from_offsets(
            self.ctx.source,
            node.location().start_offset(),
            node.location().end_offset(),
        );
        self.check_assignment(&value, loc.line);
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        let value = node.value();
        let loc = crate::offense::Location::from_offsets(
            self.ctx.source,
            node.location().start_offset(),
            node.location().end_offset(),
        );
        self.check_assignment(&value, loc.line);
        ruby_prism::visit_constant_or_write_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let was_in_def = self.in_def;
        self.in_def = true;
        ruby_prism::visit_def_node(self, node);
        self.in_def = was_in_def;
    }
}

impl Cop for MutableConstant {
    fn name(&self) -> &'static str {
        "Style/MutableConstant"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = MutableConstantVisitor::new(self, ctx);
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
