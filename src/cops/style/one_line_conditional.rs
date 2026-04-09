use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct OneLineConditional {
    always_multiline: bool,
}

impl OneLineConditional {
    pub fn new() -> Self {
        Self {
            always_multiline: false,
        }
    }

    pub fn with_config(always_multiline: bool) -> Self {
        Self { always_multiline }
    }

    fn is_single_line(source: &str, start: usize, end: usize) -> bool {
        !source[start..end].contains('\n')
    }

    /// Whether the if node starts with "elsif"
    fn is_elsif(node: &ruby_prism::IfNode, source: &str) -> bool {
        node.if_keyword_loc()
            .map_or(false, |loc| source[loc.start_offset()..].starts_with("elsif"))
    }

    /// Whether the if node is a ternary (doesn't start with "if")
    fn is_ternary(node: &ruby_prism::IfNode, source: &str) -> bool {
        !source[node.location().start_offset()..].starts_with("if")
    }

    /// Check if statements have multiple expressions (begin_type equivalent)
    fn has_multiple_stmts(stmts: &Option<ruby_prism::StatementsNode>) -> bool {
        if let Some(s) = stmts {
            let body: Vec<_> = s.body().iter().collect();
            body.len() >= 2
        } else {
            false
        }
    }

    /// Determine if the subsequent node means we must use multiline
    /// Returns (has_else, has_elsif_or_multi_else)
    fn analyze_subsequent(subsequent: &Option<ruby_prism::Node>) -> (bool, bool) {
        match subsequent {
            None => (false, false),
            Some(node) => {
                match node {
                    Node::ElseNode { .. } => {
                        let else_node = node.as_else_node().unwrap();
                        // Empty else = no offense
                        if else_node.statements().is_none() {
                            return (false, false); // treat as no else
                        }
                        // Check if else body has multiple expressions
                        let multi = if let Some(stmts) = else_node.statements() {
                            let body: Vec<_> = stmts.body().iter().collect();
                            body.len() >= 2
                        } else {
                            false
                        };
                        (true, multi)
                    }
                    Node::IfNode { .. } => {
                        // elsif - always multiline
                        (true, true)
                    }
                    _ => (false, false),
                }
            }
        }
    }

    fn message(keyword: &str, multiline: bool) -> String {
        if multiline {
            format!(
                "Favor multi-line `{}` over single-line `{}/then/else/end` constructs.",
                keyword, keyword
            )
        } else {
            format!(
                "Favor the ternary operator (`?:`) over single-line `{}/then/else/end` constructs.",
                keyword
            )
        }
    }
}

impl Cop for OneLineConditional {
    fn name(&self) -> &'static str {
        "Style/OneLineConditional"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_if(&self, node: &ruby_prism::IfNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip elsif nodes, ternaries, and modifier-if (no end keyword)
        if Self::is_elsif(node, ctx.source) {
            return vec![];
        }
        if Self::is_ternary(node, ctx.source) {
            return vec![];
        }
        if node.end_keyword_loc().is_none() {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if !Self::is_single_line(ctx.source, start, end) {
            return vec![];
        }

        let subsequent = node.subsequent();

        let (has_else, cannot_ternary) = Self::analyze_subsequent(&subsequent);
        if !has_else {
            return vec![];
        }

        // Check if the if-branch has multiple statements
        if Self::has_multiple_stmts(&node.statements()) {
            return vec![];
        }

        let multiline = self.always_multiline || cannot_ternary;
        let msg = Self::message("if", multiline);

        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)]
    }

    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip modifier-unless (no end keyword)
        if node.end_keyword_loc().is_none() {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();

        if !Self::is_single_line(ctx.source, start, end) {
            return vec![];
        }

        // UnlessNode uses else_clause() which returns Option<ElseNode>
        let else_clause = node.else_clause();
        match else_clause {
            None => return vec![],
            Some(else_node) => {
                // Empty else = no offense
                if else_node.statements().is_none() {
                    return vec![];
                }
                // Check if else body has multiple expressions
                let else_multi = if let Some(stmts) = else_node.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    body.len() >= 2
                } else {
                    false
                };

                // Check if the if-branch has multiple statements
                if Self::has_multiple_stmts(&node.statements()) {
                    return vec![];
                }

                let multiline = self.always_multiline || else_multi;
                let msg = Self::message("unless", multiline);

                vec![ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)]
            }
        }
    }
}
