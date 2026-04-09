//! Lint/OutOfRangeRegexpRef cop
//!
//! Looks for references of Regexp captures that are out of range
//! and thus always return nil.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct OutOfRangeRegexpRef;

impl OutOfRangeRegexpRef {
    pub fn new() -> Self {
        Self
    }
}

// Methods that can appear with regexp as receiver: =~, ===, match
const REGEXP_RECEIVER_METHODS: &[&str] = &["=~", "===", "match"];

// Methods that can appear with regexp as first argument
const REGEXP_ARGUMENT_METHODS: &[&str] = &[
    "=~", "match", "grep", "gsub", "gsub!", "sub", "sub!", "[]", "slice", "slice!", "index",
    "rindex", "scan", "partition", "rpartition", "start_with?", "end_with?",
];

impl Cop for OutOfRangeRegexpRef {
    fn name(&self) -> &'static str {
        "Lint/OutOfRangeRegexpRef"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = OutOfRangeVisitor {
            ctx,
            valid_ref: Some(0), // on_new_investigation sets @valid_ref = 0
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct OutOfRangeVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    /// None means we don't know the regexp context (e.g., after a non-literal regexp call).
    /// Some(n) means the last regexp had n capture groups.
    valid_ref: Option<u32>,
    offenses: Vec<Offense>,
}

impl<'a> OutOfRangeVisitor<'a> {
    /// Count capture groups in a RegularExpressionNode.
    /// Returns None if the regexp has interpolation (we skip those).
    /// Named captures take priority over numbered captures (matching RuboCop behavior).
    fn check_regexp(&mut self, node: &Node) -> Option<u32> {
        if let Some(re) = node.as_regular_expression_node() {
            let content = re.unescaped();
            let content_str = String::from_utf8_lossy(content);
            let named = count_named_captures(&content_str);
            let count = if named > 0 {
                named
            } else {
                count_numbered_captures(&content_str)
            };
            self.valid_ref = Some(count);
            Some(count)
        } else {
            // InterpolatedRegularExpressionNode or non-regexp -- skip
            None
        }
    }

    fn is_regexp_node(node: &Node) -> bool {
        matches!(
            node,
            Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
        )
    }

    /// Check if this call is a regexp-related method.
    fn is_regexp_method(method: &str) -> bool {
        REGEXP_ARGUMENT_METHODS.contains(&method) || REGEXP_RECEIVER_METHODS.contains(&method)
    }

    /// Handle after_send: check if regexp is in receiver or first argument position.
    /// Only called for methods in REGEXP_CAPTURE_METHODS.
    fn handle_send(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());

        // Reset valid_ref to nil -- if we encounter a send with a regexp method,
        // we update it. Otherwise it stays nil (unknown).
        self.valid_ref = None;

        // Check if first argument is a regexp literal
        if REGEXP_ARGUMENT_METHODS.contains(&method.as_ref()) {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if !arg_list.is_empty() && Self::is_regexp_node(&arg_list[0]) {
                    self.check_regexp(&arg_list[0]);
                    return;
                }
            }
        }

        // Check if receiver is a regexp literal
        if REGEXP_RECEIVER_METHODS.contains(&method.as_ref()) {
            if let Some(recv) = node.receiver() {
                if Self::is_regexp_node(&recv) {
                    self.check_regexp(&recv);
                    return;
                }
            }
        }
    }

    /// Collect all regexp nodes from a pattern match pattern (recursively)
    fn collect_regexp_patterns(node: &Node, out: &mut Vec<u32>) {
        match node {
            Node::RegularExpressionNode { .. } => {
                let re = node.as_regular_expression_node().unwrap();
                let content = re.unescaped();
                let content_str = String::from_utf8_lossy(content);
                let named = count_named_captures(&content_str);
                let count = if named > 0 {
                    named
                } else {
                    count_numbered_captures(&content_str)
                };
                out.push(count);
            }
            Node::InterpolatedRegularExpressionNode { .. } => {
                // Skip interpolated regexps (like RuboCop's `interpolation?` check)
            }
            _ => {
                // Recurse into child nodes to find nested regexps
                // (handles arrays, hashes, alternatives, pins, captures, etc.)
                struct RegexpCollector<'b> {
                    out: &'b mut Vec<u32>,
                }
                impl Visit<'_> for RegexpCollector<'_> {
                    fn visit_regular_expression_node(
                        &mut self,
                        node: &ruby_prism::RegularExpressionNode,
                    ) {
                        let content = node.unescaped();
                        let content_str = String::from_utf8_lossy(content);
                        let named = count_named_captures(&content_str);
                        let count = if named > 0 {
                            named
                        } else {
                            count_numbered_captures(&content_str)
                        };
                        self.out.push(count);
                    }
                    // Don't recurse into interpolated regexps
                    fn visit_interpolated_regular_expression_node(
                        &mut self,
                        _node: &ruby_prism::InterpolatedRegularExpressionNode,
                    ) {
                        // skip
                    }
                }
                let mut collector = RegexpCollector { out };
                collector.visit(node);
            }
        }
    }

    fn check_nth_ref(&mut self, node: &ruby_prism::NumberedReferenceReadNode) {
        let backref = node.number();
        if let Some(valid) = self.valid_ref {
            if backref > valid {
                let count_str = if valid == 0 {
                    "no".to_string()
                } else {
                    valid.to_string()
                };
                let group_str = if valid == 1 { "group" } else { "groups" };
                let message = format!(
                    "${} is out of range ({} regexp capture {} detected).",
                    backref, count_str, group_str
                );
                let loc = node.location();
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/OutOfRangeRegexpRef",
                    &message,
                    Severity::Warning,
                    loc.start_offset(),
                    loc.end_offset(),
                ));
            }
        }
    }
}

impl Visit<'_> for OutOfRangeVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        let is_regexp_method = Self::is_regexp_method(&method);

        if is_regexp_method {
            // For regexp-related methods, we need special ordering:
            // 1. Visit receiver and arguments (to handle nested calls like `str.gsub(/ +/, "") =~ /re/`)
            // 2. Call handle_send (to set valid_ref from this call's regexp)
            // 3. Visit the block (so $N refs inside the block see the correct valid_ref)
            //
            // This mirrors RuboCop's AST where blocks wrap sends, so after_send
            // fires between argument processing and block body processing.

            // Visit receiver
            if let Some(recv) = node.receiver() {
                self.visit(&recv);
            }
            // Visit arguments
            if let Some(args) = node.arguments() {
                self.visit_arguments_node(&args);
            }

            // Now process after_send (sets valid_ref)
            self.handle_send(node);

            // Visit block (where $N refs live)
            if let Some(block) = node.block() {
                self.visit(&block);
            }
        } else {
            // For non-regexp methods, just do normal traversal
            // Don't call handle_send -- only regexp-related methods affect valid_ref
            ruby_prism::visit_call_node(self, node);
        }
    }

    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode) {
        // `/(?<foo>FOO)/ =~ str` -- MatchWriteNode wraps a CallNode
        // The call's receiver is the regexp
        let call = node.call();
        if let Some(recv) = call.receiver() {
            self.check_regexp(&recv);
        }
        ruby_prism::visit_match_write_node(self, node);
    }

    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode) {
        // Collect capture counts from all regexp conditions (mirrors RuboCop's on_when)
        let conditions: Vec<_> = node.conditions().iter().collect();
        let mut counts: Vec<u32> = Vec::new();
        for cond in &conditions {
            if let Node::RegularExpressionNode { .. } = cond {
                let re = cond.as_regular_expression_node().unwrap();
                let content = re.unescaped();
                let content_str = String::from_utf8_lossy(content);
                let named = count_named_captures(&content_str);
                let count = if named > 0 {
                    named
                } else {
                    count_numbered_captures(&content_str)
                };
                counts.push(count);
            }
            // Skip InterpolatedRegularExpressionNode and non-regexp conditions
        }
        // Set valid_ref to max of counts, or None if no regexp conditions
        self.valid_ref = counts.iter().max().copied();

        // Visit the body (statements) to check nth refs
        if let Some(stmts) = node.statements() {
            ruby_prism::visit_statements_node(self, &stmts);
        }
    }

    fn visit_in_node(&mut self, node: &ruby_prism::InNode) {
        // Collect regexps from pattern (mirrors RuboCop's on_in_pattern)
        let pattern = node.pattern();
        let mut counts = Vec::new();
        Self::collect_regexp_patterns(&pattern, &mut counts);
        // Set valid_ref to max of counts, or None if no regexp patterns
        self.valid_ref = counts.iter().max().copied();

        // Visit the body
        if let Some(stmts) = node.statements() {
            ruby_prism::visit_statements_node(self, &stmts);
        }
    }

    fn visit_numbered_reference_read_node(
        &mut self,
        node: &ruby_prism::NumberedReferenceReadNode,
    ) {
        self.check_nth_ref(node);
    }
}

/// Count named capture groups `(?<name>...)` in a regexp pattern.
/// This is a simplified parser that handles basic cases.
fn count_named_captures(pattern: &str) -> u32 {
    let bytes = pattern.as_bytes();
    let mut count = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped char
            continue;
        }
        if bytes[i] == b'(' && i + 3 < bytes.len() && bytes[i + 1] == b'?' && bytes[i + 2] == b'<'
        {
            // Check it's a named capture (?<name>...) not a lookbehind (?<=...) or (?<!...)
            if i + 3 < bytes.len() && bytes[i + 3] != b'=' && bytes[i + 3] != b'!' {
                count += 1;
            }
        }
        i += 1;
    }
    count
}

/// Count numbered (unnamed) capture groups in a regexp pattern.
/// Counts `(` that are not followed by `?` (which would be non-capturing or special groups).
fn count_numbered_captures(pattern: &str) -> u32 {
    let bytes = pattern.as_bytes();
    let mut count = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped char
            continue;
        }
        if bytes[i] == b'[' {
            // Skip character class
            i += 1;
            while i < bytes.len() && bytes[i] != b']' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'(' {
            // Check if it's a capturing group (not followed by ?)
            if i + 1 >= bytes.len() || bytes[i + 1] != b'?' {
                count += 1;
            }
        }
        i += 1;
    }
    count
}
