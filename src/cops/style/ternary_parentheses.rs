use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnforcedStyle {
    RequireNoParentheses,
    RequireParentheses,
    RequireParenthesesWhenComplex,
}

pub struct TernaryParentheses {
    style: EnforcedStyle,
    allow_safe_assignment: bool,
}

impl TernaryParentheses {
    pub fn new(style: EnforcedStyle, allow_safe_assignment: bool) -> Self {
        Self { style, allow_safe_assignment }
    }

    fn is_ternary(node: &ruby_prism::IfNode, source: &str) -> bool {
        let start = node.location().start_offset();
        !source[start..].starts_with("if") && !source[start..].starts_with("elsif")
    }

    /// Check if condition is wrapped in parentheses (ParenthesesNode in Prism)
    fn is_parenthesized(condition: &Node) -> bool {
        matches!(condition, Node::ParenthesesNode { .. })
    }

    /// Check if condition is a safe assignment (assignment wrapped in parens)
    fn is_safe_assignment(condition: &Node) -> bool {
        if let Node::ParenthesesNode { .. } = condition {
            let paren = condition.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Some(stmts) = body.as_statements_node() {
                    let stmts_list: Vec<_> = stmts.body().iter().collect();
                    if stmts_list.len() == 1 {
                        return Self::contains_assignment(&stmts_list[0]);
                    }
                }
            }
        }
        false
    }

    fn contains_assignment(node: &Node) -> bool {
        matches!(
            node,
            Node::LocalVariableWriteNode { .. }
                | Node::InstanceVariableWriteNode { .. }
                | Node::ClassVariableWriteNode { .. }
                | Node::GlobalVariableWriteNode { .. }
                | Node::ConstantWriteNode { .. }
                | Node::ConstantPathWriteNode { .. }
        )
    }

    /// Check if this is a multiline condition where closing paren is on its own line
    fn only_closing_paren_on_last_line(condition: &Node, source: &str) -> bool {
        let loc = condition.location();
        let cond_src = &source[loc.start_offset()..loc.end_offset()];
        if let Some(last_line) = cond_src.split('\n').last() {
            last_line.trim() == ")"
        } else {
            false
        }
    }

    /// Check for one-line pattern matching: `(foo in bar) ? a : b`
    fn is_pattern_matching_condition(condition: &Node) -> bool {
        if let Node::ParenthesesNode { .. } = condition {
            let paren = condition.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Some(stmts) = body.as_statements_node() {
                    let stmts_list: Vec<_> = stmts.body().iter().collect();
                    if stmts_list.len() == 1 {
                        return matches!(
                            stmts_list[0],
                            Node::MatchPredicateNode { .. } | Node::MatchRequiredNode { .. }
                        );
                    }
                }
            }
        }
        false
    }

    /// Determine if condition is "complex" (not a simple variable, const, method call, etc.)
    fn is_complex_condition(condition: &Node) -> bool {
        if let Node::ParenthesesNode { .. } = condition {
            // Unwrap parens and check inner
            let paren = condition.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Some(stmts) = body.as_statements_node() {
                    let stmts_list: Vec<_> = stmts.body().iter().collect();
                    if stmts_list.len() == 1 {
                        return Self::is_complex_condition(&stmts_list[0]);
                    }
                }
            }
            return false;
        }
        !Self::is_non_complex_expression(condition)
    }

    fn is_non_complex_expression(node: &Node) -> bool {
        // Variables, constants, defined?, yield are non-complex
        match node {
            Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::ConstantReadNode { .. }
            | Node::ConstantPathNode { .. }
            | Node::DefinedNode { .. }
            | Node::YieldNode { .. } => true,
            Node::CallNode { .. } => {
                Self::is_non_complex_send(&node.as_call_node().unwrap())
            }
            _ => false,
        }
    }

    /// Non-complex send: not an operator method, or is []
    fn is_non_complex_send(call: &ruby_prism::CallNode) -> bool {
        let name = node_name!(call);
        // If it's an operator method and not [], it's complex
        if Self::is_operator_method(&name) && name != "[]" {
            return false;
        }
        true
    }

    fn is_operator_method(name: &str) -> bool {
        matches!(
            name,
            "+" | "-" | "*" | "/" | "%" | "**"
                | "==" | "!=" | "===" | "<=>" | "<" | ">" | "<=" | ">="
                | "&" | "|" | "^" | "~" | "<<" | ">>"
                | "=~" | "!~"
                | "&&" | "||"
                | "+@" | "-@"
        )
    }

    fn offense_detected(&self, condition: &Node) -> bool {
        if Self::is_safe_assignment(condition) {
            return !self.allow_safe_assignment;
        }

        let parens = Self::is_parenthesized(condition);
        match self.style {
            EnforcedStyle::RequireParenthesesWhenComplex => {
                if Self::is_complex_condition(condition) {
                    !parens
                } else {
                    parens
                }
            }
            EnforcedStyle::RequireParentheses => !parens,
            EnforcedStyle::RequireNoParentheses => parens,
        }
    }

    fn message(&self, condition: &Node) -> String {
        match self.style {
            EnforcedStyle::RequireParenthesesWhenComplex => {
                let command = if Self::is_parenthesized(condition) {
                    "Only use"
                } else {
                    "Use"
                };
                format!("{} parentheses for ternary expressions with complex conditions.", command)
            }
            EnforcedStyle::RequireParentheses => {
                "Use parentheses for ternary conditions.".to_string()
            }
            EnforcedStyle::RequireNoParentheses => {
                "Omit parentheses for ternary conditions.".to_string()
            }
        }
    }

    /// Check if removing parens would be unsafe (children have below-ternary precedence)
    fn unsafe_autocorrect(condition: &Node) -> bool {
        if let Node::ParenthesesNode { .. } = condition {
            let paren = condition.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Some(stmts) = body.as_statements_node() {
                    for stmt in stmts.body().iter() {
                        if Self::below_ternary_precedence(&stmt) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Check if a node has precedence below ternary (keyword and/or/not)
    fn below_ternary_precedence(node: &Node) -> bool {
        match node {
            Node::OrNode { .. } => true,
            Node::AndNode { .. } => true,
            Node::CallNode { .. } => {
                // prefix `not`
                let call = node.as_call_node().unwrap();
                let name = node_name!(call);
                name == "!" && call.receiver().is_some() && call.opening_loc().is_none()
            }
            _ => false,
        }
    }

    /// Build correction for removing parens
    fn correct_parenthesized(condition: &Node, source: &str) -> Option<Correction> {
        if Self::is_safe_assignment(condition) || Self::unsafe_autocorrect(condition) {
            return None;
        }

        let paren = condition.as_parentheses_node()?;
        let open_loc = paren.opening_loc();
        let close_loc = paren.closing_loc();

        let mut edits = vec![];

        // Check if inner call needs arg parenthesization when removing outer parens
        let arg_parens_edits = Self::get_arg_parens_edits_from_paren(condition, source);

        // Remove opening paren
        edits.push(crate::offense::Edit {
            start_offset: open_loc.start_offset(),
            end_offset: open_loc.end_offset(),
            replacement: String::new(),
        });

        // Remove closing paren, add space if needed
        let close_end = close_loc.end_offset();
        let need_space = close_end < source.len()
            && source.as_bytes().get(close_end).map_or(true, |&b| b != b' ' && b != b'\n');

        let replacement = if need_space { " ".to_string() } else { String::new() };
        edits.push(crate::offense::Edit {
            start_offset: close_loc.start_offset(),
            end_offset: close_loc.end_offset(),
            replacement,
        });

        // Add arg parenthesization edits if needed
        edits.extend(arg_parens_edits);

        Some(Correction { edits })
    }

    /// Build correction for adding parens
    fn correct_unparenthesized(condition: &Node) -> Correction {
        let loc = condition.location();
        Correction {
            edits: vec![
                crate::offense::Edit {
                    start_offset: loc.start_offset(),
                    end_offset: loc.start_offset(),
                    replacement: "(".to_string(),
                },
                crate::offense::Edit {
                    start_offset: loc.end_offset(),
                    end_offset: loc.end_offset(),
                    replacement: ")".to_string(),
                },
            ],
        }
    }

    fn get_arg_parens_edits_from_paren(condition: &Node, source: &str) -> Vec<crate::offense::Edit> {
        if let Node::ParenthesesNode { .. } = condition {
            let paren = condition.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                if let Some(stmts) = body.as_statements_node() {
                    let stmts_list: Vec<_> = stmts.body().iter().collect();
                    if stmts_list.len() == 1 && Self::node_args_need_parens(&stmts_list[0]) {
                        if let Some(edits) = Self::parenthesize_args_correction(&stmts_list[0], source) {
                            return edits;
                        }
                    }
                }
            }
        }
        vec![]
    }

    fn node_args_need_parens(node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let has_args = call.arguments().map_or(false, |a| a.arguments().iter().count() > 0);
                if !has_args { return false; }
                if call.opening_loc().is_some() { return false; } // already parenthesized

                // Has dot or safe navigation, or is unparenthesized method call
                let has_dot = call.call_operator_loc().is_some();
                if has_dot { return true; }

                // Check if it's an unparenthesized method call (name starts with letter)
                let name = node_name!(call);
                name.chars().next().map_or(false, |c| c.is_alphabetic())
            }
            Node::DefinedNode { .. } => {
                let def = node.as_defined_node().unwrap();
                // Check if defined? is not using parentheses already
                // In Prism, DefinedNode has lparen and rparen locations
                def.lparen_loc().is_none()
            }
            _ => false,
        }
    }

    fn parenthesize_args_correction(node: &Node, _source: &str) -> Option<Vec<crate::offense::Edit>> {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let args = call.arguments()?;
                let args_list: Vec<_> = args.arguments().iter().collect();
                if args_list.is_empty() { return None; }

                // Find the range between method name end and first arg start
                let selector_end = if let Some(msg_loc) = call.message_loc() {
                    msg_loc.end_offset()
                } else {
                    return None;
                };
                let first_arg_start = args_list[0].location().start_offset();
                let last_arg_end = args_list.last().unwrap().location().end_offset();

                // Replace space between selector and first arg with "("
                Some(vec![
                    crate::offense::Edit {
                        start_offset: selector_end,
                        end_offset: first_arg_start,
                        replacement: "(".to_string(),
                    },
                    crate::offense::Edit {
                        start_offset: last_arg_end,
                        end_offset: last_arg_end,
                        replacement: ")".to_string(),
                    },
                ])
            }
            Node::DefinedNode { .. } => {
                let def = node.as_defined_node().unwrap();
                let keyword_loc = def.keyword_loc();
                let keyword_end = keyword_loc.end_offset();
                // "defined?" ends at keyword_end
                // The value follows after a space
                let value = def.value();
                let value_start = value.location().start_offset();
                let value_end = value.location().end_offset();

                Some(vec![
                    crate::offense::Edit {
                        start_offset: keyword_end,
                        end_offset: value_start,
                        replacement: "(".to_string(),
                    },
                    crate::offense::Edit {
                        start_offset: value_end,
                        end_offset: value_end,
                        replacement: ")".to_string(),
                    },
                ])
            }
            _ => None,
        }
    }
}

impl Default for TernaryParentheses {
    fn default() -> Self {
        Self::new(EnforcedStyle::RequireNoParentheses, true)
    }
}

impl Cop for TernaryParentheses {
    fn name(&self) -> &'static str {
        "Style/TernaryParentheses"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_ternary(node, ctx.source) {
            return vec![];
        }

        let condition = node.predicate();

        // Skip if only closing paren on last line (multiline condition)
        if Self::only_closing_paren_on_last_line(&condition, ctx.source) {
            return vec![];
        }

        // Skip pattern matching conditions
        if Self::is_pattern_matching_condition(&condition) {
            return vec![];
        }

        if !self.offense_detected(&condition) {
            return vec![];
        }

        let msg = self.message(&condition);

        // The offense covers the entire ternary expression (the IfNode)
        // For multiline, cap at end of first line (TOML extraction records first-line extent)
        let loc = node.location();
        let start = loc.start_offset();
        let mut end = loc.end_offset();
        // Find end of first line
        if let Some(nl_pos) = ctx.source[start..end].find('\n') {
            end = start + nl_pos;
        }
        let mut offense = ctx.offense_with_range(
            self.name(),
            &msg,
            self.severity(),
            start,
            end,
        );

        // Build correction
        if Self::is_parenthesized(&condition) {
            if let Some(correction) = Self::correct_parenthesized(&condition, ctx.source) {
                offense = offense.with_correction(correction);
            }
        } else {
            offense = offense.with_correction(Self::correct_unparenthesized(&condition));
        }

        vec![offense]
    }
}
