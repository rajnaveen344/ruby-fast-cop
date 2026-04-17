//! Style/EmptyLiteral - Prefer `[]` over `Array.new`, `{}` over `Hash.new`, `''` over `String.new`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/empty_literal.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node, Visit};

pub struct EmptyLiteral {
    prefer_double_quotes: bool,
    /// Whether Style/FrozenStringLiteralComment cop is enabled.
    /// When true and no magic comment, String.new is not flagged.
    frozen_string_cop_enabled: bool,
}

impl EmptyLiteral {
    pub fn new() -> Self {
        Self {
            prefer_double_quotes: false,
            frozen_string_cop_enabled: false,
        }
    }

    pub fn with_config(prefer_double_quotes: bool) -> Self {
        Self {
            prefer_double_quotes,
            frozen_string_cop_enabled: false,
        }
    }

    pub fn with_full_config(prefer_double_quotes: bool, frozen_string_cop_enabled: bool) -> Self {
        Self {
            prefer_double_quotes,
            frozen_string_cop_enabled,
        }
    }

    fn preferred_string_literal(&self) -> &'static str {
        if self.prefer_double_quotes {
            "\"\""
        } else {
            "''"
        }
    }

    fn frozen_string_literals_enabled(source: &str) -> bool {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') {
                if trimmed.contains("frozen_string_literal: true")
                    || trimmed.contains("frozen-string-literal: true")
                {
                    return true;
                }
                continue;
            }
            break;
        }
        false
    }

    fn frozen_string_literals_disabled(source: &str) -> bool {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') {
                if trimmed.contains("frozen_string_literal: false")
                    || trimmed.contains("frozen-string-literal: false")
                {
                    return true;
                }
                continue;
            }
            break;
        }
        false
    }

    /// Mirrors RuboCop's `frozen_strings?` method.
    fn frozen_strings(&self, source: &str) -> bool {
        if Self::frozen_string_literals_enabled(source) {
            return true;
        }
        // If FrozenStringLiteralComment cop is enabled and no explicit `false` comment,
        // strings are expected to be frozen.
        self.frozen_string_cop_enabled
            && !Self::frozen_string_literals_disabled(source)
    }

    /// Get receiver constant name for `.new` or `[]` calls.
    fn receiver_constant_name(node: &CallNode, source: &str) -> Option<&'static str> {
        let receiver = node.receiver()?;

        if let Some(const_node) = receiver.as_constant_read_node() {
            let loc = const_node.location();
            let name = &source[loc.start_offset()..loc.end_offset()];
            return match name {
                "Array" => Some("Array"),
                "Hash" => Some("Hash"),
                "String" => Some("String"),
                _ => None,
            };
        }

        if let Some(const_path) = receiver.as_constant_path_node() {
            if const_path.parent().is_some() {
                return None;
            }
            if let Some(name_const) = const_path.name() {
                let name = String::from_utf8_lossy(name_const.as_slice());
                return match name.as_ref() {
                    "Array" => Some("Array"),
                    "Hash" => Some("Hash"),
                    "String" => Some("String"),
                    _ => None,
                };
            }
        }

        None
    }

    fn has_block(node: &CallNode) -> bool {
        if let Some(block) = node.block() {
            block.as_block_node().is_some()
        } else {
            false
        }
    }

    fn is_kernel_conversion_with_empty_array(node: &CallNode) -> Option<&'static str> {
        let method = String::from_utf8_lossy(node.name().as_slice());
        match method.as_ref() {
            "Array" | "Hash" => {}
            _ => return None,
        }

        if node.receiver().is_some() {
            return None;
        }

        let args = node.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return None;
        }

        if let Some(arr) = arg_list[0].as_array_node() {
            if arr.elements().iter().count() == 0 {
                return match method.as_ref() {
                    "Array" => Some("Array"),
                    "Hash" => Some("Hash"),
                    _ => None,
                };
            }
        }
        None
    }

    fn is_bracket_call_no_args(node: &CallNode, source: &str) -> Option<&'static str> {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method != "[]" {
            return None;
        }

        let const_name = Self::receiver_constant_name(node, source)?;
        if const_name != "Array" && const_name != "Hash" {
            return None;
        }

        if let Some(args) = node.arguments() {
            if args.arguments().iter().count() > 0 {
                return None;
            }
        }

        Some(const_name)
    }
}

struct EmptyLiteralVisitor<'a> {
    cop: &'a EmptyLiteral,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Track parent call nodes for Hash.new unparenthesized arg detection
    parent_call: Option<(usize, usize)>, // (start, end) of parent call
}

impl<'a> EmptyLiteralVisitor<'a> {
    /// Check if a call node is the first unparenthesized argument to a send/super.
    fn is_first_unparenthesized_arg(&self, node: &CallNode) -> bool {
        let node_start = node.location().start_offset();
        if node_start == 0 {
            return false;
        }

        let bytes = self.ctx.source.as_bytes();
        // Walk backwards past spaces
        let mut pos = node_start;
        while pos > 0 && bytes[pos - 1] == b' ' {
            pos -= 1;
        }

        // Must be preceded by identifier char (method name) without parenthesis
        if pos > 0 && bytes[pos - 1] != b'(' && bytes[pos - 1] != b',' {
            if pos > 0
                && (bytes[pos - 1].is_ascii_alphanumeric()
                    || bytes[pos - 1] == b'_'
                    || bytes[pos - 1] == b'!')
            {
                return true;
            }
        }

        false
    }

    /// Get the full arguments range for a parent call that has this node as first unparenthesized arg.
    fn find_all_args_range(&self, node: &CallNode) -> Option<(usize, usize)> {
        // The node is the first arg. Find the end of all args on the same line context.
        // We'll look for the parent call in the source.
        let node_start = node.location().start_offset();
        let bytes = self.ctx.source.as_bytes();

        // Walk backwards to find space before node
        let mut space_start = node_start;
        while space_start > 0 && bytes[space_start - 1] == b' ' {
            space_start -= 1;
        }

        // The parent call's args start after the space
        // Find end: look for newline or end of parent scope
        // For now, return the parent_call range if available
        self.parent_call
    }

    fn check_call(&mut self, node: &CallNode) {
        let source = self.ctx.source;

        // Check for Array([]) or Hash([])
        if let Some(const_name) = EmptyLiteral::is_kernel_conversion_with_empty_array(node) {
            let node_src = self.ctx.src(node.location().start_offset(), node.location().end_offset());
            let (msg, replacement) = match const_name {
                "Array" => (
                    format!("Use array literal `[]` instead of `{}`.", node_src),
                    "[]".to_string(),
                ),
                _ => (
                    format!("Use hash literal `{{}}` instead of `{}`.", node_src),
                    "{}".to_string(),
                ),
            };
            let offense = self.ctx.offense_with_range(
                "Style/EmptyLiteral", &msg, Severity::Convention,
                node.location().start_offset(), node.location().end_offset(),
            );
            let correction = Correction::replace(
                node.location().start_offset(), node.location().end_offset(), &replacement,
            );
            self.offenses.push(offense.with_correction(correction));
            return;
        }

        // Check for Array[] or Hash[]
        if let Some(const_name) = EmptyLiteral::is_bracket_call_no_args(node, source) {
            let node_src = self.ctx.src(node.location().start_offset(), node.location().end_offset());
            let (msg, replacement) = match const_name {
                "Array" => (
                    format!("Use array literal `[]` instead of `{}`.", node_src),
                    "[]".to_string(),
                ),
                _ => (
                    format!("Use hash literal `{{}}` instead of `{}`.", node_src),
                    "{}".to_string(),
                ),
            };
            let offense = self.ctx.offense_with_range(
                "Style/EmptyLiteral", &msg, Severity::Convention,
                node.location().start_offset(), node.location().end_offset(),
            );
            let correction = Correction::replace(
                node.location().start_offset(), node.location().end_offset(), &replacement,
            );
            self.offenses.push(offense.with_correction(correction));
            return;
        }

        // Must be `.new`
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method != "new" {
            return;
        }

        let const_name = match EmptyLiteral::receiver_constant_name(node, source) {
            Some(name) => name,
            None => return,
        };

        // Check arguments
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if !arg_list.is_empty() {
                if const_name == "Array" && arg_list.len() == 1 {
                    if let Some(arr) = arg_list[0].as_array_node() {
                        if arr.elements().iter().count() != 0 {
                            return;
                        }
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        }

        // Check for block
        if EmptyLiteral::has_block(node) {
            return;
        }

        let node_src = self.ctx.src(node.location().start_offset(), node.location().end_offset());

        match const_name {
            "Array" => {
                let msg = format!("Use array literal `[]` instead of `{}`.", node_src);
                let offense = self.ctx.offense_with_range(
                    "Style/EmptyLiteral", &msg, Severity::Convention,
                    node.location().start_offset(), node.location().end_offset(),
                );
                let correction = Correction::replace(
                    node.location().start_offset(), node.location().end_offset(), "[]",
                );
                self.offenses.push(offense.with_correction(correction));
            }
            "Hash" => {
                let msg = format!("Use hash literal `{{}}` instead of `{}`.", node_src);
                let offense = self.ctx.offense_with_range(
                    "Style/EmptyLiteral", &msg, Severity::Convention,
                    node.location().start_offset(), node.location().end_offset(),
                );

                // Check if this is the first unparenthesized argument
                if self.is_first_unparenthesized_arg(node) {
                    // Need to wrap all args in parentheses
                    // Find the space before the node and all remaining args
                    let space_start = {
                        let mut pos = node.location().start_offset();
                        while pos > 0 && self.ctx.bytes()[pos - 1] == b' ' {
                            pos -= 1;
                        }
                        pos
                    };

                    if let Some((_parent_start, parent_end)) = self.parent_call {
                        // Replace from space before Hash.new to end of all args with parens
                        let remaining_args = &self.ctx.source[node.location().end_offset()..parent_end];
                        let replacement = if remaining_args.trim().is_empty() {
                            "({})".to_string()
                        } else {
                            format!("({{}}{})", remaining_args.trim_start_matches(' '))
                        };
                        let correction = Correction::replace(
                            space_start, parent_end, &replacement,
                        );
                        self.offenses.push(offense.with_correction(correction));
                    } else {
                        // Fallback: just replace the node
                        let correction = Correction::replace(
                            node.location().start_offset(), node.location().end_offset(), "{}",
                        );
                        self.offenses.push(offense.with_correction(correction));
                    }
                } else {
                    let correction = Correction::replace(
                        node.location().start_offset(), node.location().end_offset(), "{}",
                    );
                    self.offenses.push(offense.with_correction(correction));
                }
            }
            "String" => {
                if self.cop.frozen_strings(source) {
                    return;
                }

                let prefer = self.cop.preferred_string_literal();
                let msg = format!("Use string literal `{}` instead of `String.new`.", prefer);
                let offense = self.ctx.offense_with_range(
                    "Style/EmptyLiteral", &msg, Severity::Convention,
                    node.location().start_offset(), node.location().end_offset(),
                );
                let correction = Correction::replace(
                    node.location().start_offset(), node.location().end_offset(), prefer,
                );
                self.offenses.push(offense.with_correction(correction));
            }
            _ => {}
        }
    }
}

impl Visit<'_> for EmptyLiteralVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());

        // Track parent call context for unparenthesized arg detection
        // A call without opening_paren_loc that has arguments is unparenthesized
        let old_parent = self.parent_call;

        if node.opening_loc().is_none() {
            if let Some(args) = node.arguments() {
                let args_list: Vec<_> = args.arguments().iter().collect();
                if !args_list.is_empty() {
                    let last_arg = &args_list[args_list.len() - 1];
                    self.parent_call = Some((
                        node.location().start_offset(),
                        last_arg.location().end_offset(),
                    ));
                }
            }
        }

        // Check this node
        self.check_call(node);

        // Visit children
        ruby_prism::visit_call_node(self, node);

        self.parent_call = old_parent;
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        ruby_prism::visit_forwarding_super_node(self, node);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        let old_parent = self.parent_call;

        // super without parentheses
        if node.lparen_loc().is_none() {
            if let Some(args) = node.arguments() {
                let args_list: Vec<_> = args.arguments().iter().collect();
                if !args_list.is_empty() {
                    let last_arg = &args_list[args_list.len() - 1];
                    self.parent_call = Some((
                        node.location().start_offset(),
                        last_arg.location().end_offset(),
                    ));
                }
            }
        }

        ruby_prism::visit_super_node(self, node);

        self.parent_call = old_parent;
    }
}

impl Cop for EmptyLiteral {
    fn name(&self) -> &'static str {
        "Style/EmptyLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = EmptyLiteralVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            parent_call: None,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Style/EmptyLiteral", |cfg| {
    let prefer_double = cfg.get_cop_config("Style/EmptyLiteral")
        .and_then(|c| c.enforced_style.as_ref())
        .or_else(|| cfg.get_cop_config("Style/StringLiterals")
            .and_then(|c| c.enforced_style.as_ref()))
        .map(|s| s == "double_quotes")
        .unwrap_or(false);
    let frozen_cop_enabled = cfg.get_cop_config("Style/FrozenStringLiteralComment")
        .and_then(|c| c.enabled)
        .unwrap_or(false);
    Some(Box::new(EmptyLiteral::with_full_config(prefer_double, frozen_cop_enabled)))
});
