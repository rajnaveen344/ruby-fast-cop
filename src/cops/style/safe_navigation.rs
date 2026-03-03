//! Style/SafeNavigation - Converts nil-check patterns to safe navigation (`&.`).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/safe_navigation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::{Node, ProgramNode, Visit};

const MSG: &str = "Use safe navigation (`&.`) instead of checking if an object exists before calling the method.";
const COP_NAME: &str = "Style/SafeNavigation";

pub struct SafeNavigation {
    allowed_methods: Vec<String>,
    convert_code_that_can_start_to_return_nil: bool,
    max_chain_length: usize,
    safe_navigation_chain_enabled: bool,
}

impl SafeNavigation {
    pub fn new() -> Self {
        Self {
            allowed_methods: vec![
                "present?".to_string(),
                "blank?".to_string(),
                "presence".to_string(),
                "try".to_string(),
                "try!".to_string(),
            ],
            convert_code_that_can_start_to_return_nil: false,
            max_chain_length: 2,
            safe_navigation_chain_enabled: true,
        }
    }

    pub fn with_config(
        allowed_methods: Vec<String>,
        convert_code_that_can_start_to_return_nil: bool,
        max_chain_length: usize,
    ) -> Self {
        Self {
            allowed_methods,
            convert_code_that_can_start_to_return_nil,
            max_chain_length,
            safe_navigation_chain_enabled: true,
        }
    }

    pub fn with_full_config(
        allowed_methods: Vec<String>,
        convert_code_that_can_start_to_return_nil: bool,
        max_chain_length: usize,
        safe_navigation_chain_enabled: bool,
    ) -> Self {
        Self {
            allowed_methods,
            convert_code_that_can_start_to_return_nil,
            max_chain_length,
            safe_navigation_chain_enabled,
        }
    }
}

impl Cop for SafeNavigation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 3) {
            return vec![];
        }
        let mut visitor = SafeNavVisitor {
            source: ctx.source,
            filename: ctx.filename,
            allowed_methods: &self.allowed_methods,
            convert_nil: self.convert_code_that_can_start_to_return_nil,
            max_chain_length: self.max_chain_length,
            safe_navigation_chain_enabled: self.safe_navigation_chain_enabled,
            offenses: Vec::new(),
        };
        visitor.visit_statements_node(&node.statements());
        visitor.offenses
    }
}

/// Lightweight span type: (start_offset, end_offset)
type Span = (usize, usize);

/// Get the span of a node
fn node_span(node: &Node) -> Span {
    let loc = node.location();
    (loc.start_offset(), loc.end_offset())
}

/// Get source text for a span
fn span_src<'a>(source: &'a str, s: Span) -> &'a str {
    &source[s.0..s.1]
}

/// Get source text for a node
fn node_src<'a>(source: &'a str, node: &Node) -> &'a str {
    span_src(source, node_span(node))
}

/// Check if node is a simple variable (local var read, constant read, or variable-like call)
fn is_simple_var(node: &Node) -> bool {
    match node {
        Node::LocalVariableReadNode { .. }
        | Node::ConstantReadNode { .. }
        | Node::ConstantPathNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::GlobalVariableReadNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            // A variable_call is `foo` that could be a local variable or bare method
            call.is_variable_call() && call.receiver().is_none() && call.arguments().is_none()
        }
        _ => false,
    }
}

/// Compare two source strings ignoring the difference between `&.` and `.`
fn src_match_ignoring_safe_nav(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    // Normalize `&.` to `.` and compare
    let norm_a = a.replace("&.", ".");
    let norm_b = b.replace("&.", ".");
    norm_a == norm_b
}

/// Check if a byte slice equals a string
fn name_eq(name: &[u8], s: &str) -> bool {
    name == s.as_bytes()
}

/// Check if a call node has a dot or safe-nav operator
fn has_dot(call: &ruby_prism::CallNode) -> bool {
    call.call_operator_loc().is_some()
}

/// Check if a call uses `::` (double colon)
fn is_double_colon(source: &str, call: &ruby_prism::CallNode) -> bool {
    if let Some(op_loc) = call.call_operator_loc() {
        let op = &source[op_loc.start_offset()..op_loc.end_offset()];
        return op == "::";
    }
    false
}

/// Check if method name is an operator
fn is_operator(name: &[u8]) -> bool {
    matches!(
        name,
        b"+" | b"-" | b"*" | b"/" | b"%" | b"**" | b"==" | b"!=" | b"<" | b">" | b"<=" | b">="
            | b"<=>" | b"<<" | b">>" | b"&" | b"|" | b"^" | b"~" | b"+@" | b"-@" | b"=~"
            | b"!~"
    )
}

/// Check if name is a comparison/arithmetic operator
fn is_comparison_or_arith(name: &[u8]) -> bool {
    matches!(
        name,
        b">" | b"<" | b">=" | b"<=" | b"==" | b"!=" | b"<=>" | b"=~" | b"!~" | b"+" | b"-"
            | b"*" | b"/" | b"%" | b"**" | b"<<" | b">>"
    )
}

/// Find the end offset of the first line starting from a given offset
fn end_of_first_line(source: &str, start_offset: usize) -> usize {
    let remaining = &source[start_offset..];
    match remaining.find('\n') {
        Some(pos) => start_offset + pos,
        None => source.len(),
    }
}

struct SafeNavVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    allowed_methods: &'a [String],
    convert_nil: bool,
    max_chain_length: usize,
    safe_navigation_chain_enabled: bool,
    offenses: Vec<Offense>,
}

impl<'a> SafeNavVisitor<'a> {
    fn terminal_method_name(&self, node: &Node) -> Option<Vec<u8>> {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            Some(call.name().as_slice().to_vec())
        } else {
            None
        }
    }

    fn is_allowed_method(&self, node: &Node) -> bool {
        if let Some(name) = self.terminal_method_name(node) {
            self.allowed_methods.iter().any(|m| m.as_bytes() == name.as_slice())
        } else {
            false
        }
    }

    fn ends_with_nil_check(&self, node: &Node) -> bool {
        self.terminal_method_name(node).map_or(false, |n| n == b"nil?")
    }

    fn ends_with_empty(&self, node: &Node) -> bool {
        self.terminal_method_name(node).map_or(false, |n| n == b"empty?")
    }

    fn is_negation(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            name_eq(call.name().as_slice(), "!")
        } else {
            false
        }
    }

    fn is_logic_jump(&self, node: &Node) -> bool {
        matches!(
            node,
            Node::BreakNode { .. } | Node::NextNode { .. } | Node::ReturnNode { .. } | Node::YieldNode { .. }
        ) || {
            if let Node::CallNode { .. } = node {
                let call = node.as_call_node().unwrap();
                let name = call.name();
                call.receiver().is_none()
                    && (name_eq(name.as_slice(), "fail")
                        || name_eq(name.as_slice(), "raise")
                        || name_eq(name.as_slice(), "throw"))
            } else {
                false
            }
        }
    }

    fn is_assignment_call(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            let n = call.name().as_slice();
            n.ends_with(b"=") && n != b"==" && n != b"!="
        } else {
            false
        }
    }

    fn is_operator_call(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            is_comparison_or_arith(call.name().as_slice())
        } else {
            false
        }
    }

    /// Check if node is `!foo.nil?`
    fn is_not_nil_check(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if name_eq(call.name().as_slice(), "!") {
                if let Some(recv) = call.receiver() {
                    if let Node::CallNode { .. } = &recv {
                        let inner = recv.as_call_node().unwrap();
                        if name_eq(inner.name().as_slice(), "nil?") && inner.receiver().is_some() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Extract checked var span from a condition.
    /// Returns (span, is_nil_form).
    /// Only accepts simple variables, nil? checks, and ! negation.
    /// Does NOT accept arbitrary method calls like `foo.bar?` as conditions.
    fn extract_checked_var_span(&self, cond: &Node) -> Option<(Span, bool)> {
        // Simple variable (local var, constant, ivar, cvar, gvar, constant path, or variable-call)
        if is_simple_var(cond) {
            return Some((node_span(cond), false));
        }

        match cond {
            Node::CallNode { .. } => {
                let call = cond.as_call_node().unwrap();
                let name = call.name();

                // foo.nil?
                if name_eq(name.as_slice(), "nil?") {
                    if let Some(recv) = call.receiver() {
                        return Some((node_span(&recv), true));
                    }
                    return None;
                }

                // !foo or !foo.nil?
                if name_eq(name.as_slice(), "!") {
                    if let Some(recv) = call.receiver() {
                        // !foo.nil? -> foo
                        if let Node::CallNode { .. } = &recv {
                            let inner = recv.as_call_node().unwrap();
                            if name_eq(inner.name().as_slice(), "nil?") {
                                if let Some(inner_recv) = inner.receiver() {
                                    return Some((node_span(&inner_recv), true));
                                }
                            }
                        }
                        // !foo -> foo (any expression)
                        return Some((node_span(&recv), false));
                    }
                    return None;
                }

                // For conditions like foo[1] (index access) - the checked var IS the whole thing
                // But NOT for foo.bar? or foo.bar (method call that returns bool/result)
                // RuboCop's pattern `$_` matches ANY node, but the overall pattern
                // `(if $_ nil? $_)` means the condition IS the checked variable
                // For if/unless with a method call condition like `if foo.bar?`,
                // the whole `foo.bar?` is the checked var. But then body must use
                // `foo.bar?.something` which is rare. The practical check is that
                // the body must have a matching receiver.
                // However, RuboCop's pattern does accept any node as the checked variable.
                // The key filter is in `offending_node?` which checks `matching_nodes?`.
                Some((node_span(cond), false))
            }
            _ => None,
        }
    }

    // ---- Chain analysis helpers (source text based) ----

    /// Find matching receiver in a call chain using source text comparison
    fn find_matching_receiver_src(&self, method_chain: &Node, parent_src: &str, checked_var_src: &str) -> bool {
        if let Node::CallNode { .. } = method_chain {
            let call = method_chain.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(parent_src, &recv);
                if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                    return true;
                }
                return self.find_matching_receiver_src(&recv, parent_src, checked_var_src);
            }
        }
        false
    }

    /// Find matching receiver in a call chain using span-based comparison
    fn find_matching_receiver_by_span(&self, method_chain: &Node, checked_var_span: Span) -> bool {
        let checked_var_src = span_src(self.source, checked_var_span);
        if let Node::CallNode { .. } = method_chain {
            let call = method_chain.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(self.source, &recv);
                if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                    return true;
                }
                return self.find_matching_receiver_by_span(&recv, checked_var_span);
            }
        }
        false
    }

    /// Count chain length using source text comparison
    fn chain_length_src(&self, node: &Node, parent_src: &str, checked_var_src: &str) -> usize {
        let mut count = 0;
        self.count_chain_src_recursive(node, parent_src, checked_var_src, &mut count);
        count
    }

    fn count_chain_src_recursive(&self, node: &Node, parent_src: &str, checked_var_src: &str, count: &mut usize) {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                *count += 1;
                let recv_src = node_src(parent_src, &recv);
                if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                    return;
                }
                self.count_chain_src_recursive(&recv, parent_src, checked_var_src, count);
            }
        }
    }

    fn chain_length_by_span(&self, node: &Node, checked_var_span: Span) -> usize {
        let checked_var_src = span_src(self.source, checked_var_span);
        let mut count = 0;
        self.count_chain_by_span_recursive(node, checked_var_src, &mut count);
        count
    }

    fn count_chain_by_span_recursive(&self, node: &Node, checked_var_src: &str, count: &mut usize) {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                *count += 1;
                if src_match_ignoring_safe_nav(node_src(self.source, &recv), checked_var_src) {
                    return;
                }
                self.count_chain_by_span_recursive(&recv, checked_var_src, count);
            }
        }
    }

    /// Check dotless/dcolon using source text comparison (for sub-parsed nodes)
    fn chain_has_dotless_or_dcolon_src(&self, node: &Node, parent_src: &str, checked_var_src: &str) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if !has_dot(&call) {
                let n = call.name();
                if name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") || is_operator(n.as_slice()) {
                    return true;
                }
            }
            if is_double_colon(parent_src, &call) {
                return true;
            }
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(parent_src, &recv);
                if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                    return false;
                }
                return self.chain_has_dotless_or_dcolon_src(&recv, parent_src, checked_var_src);
            }
        }
        false
    }

    fn chain_has_dotless_or_dcolon_by_span(&self, node: &Node, checked_var_span: Span) -> bool {
        let checked_var_src = span_src(self.source, checked_var_span);
        self.check_dotless_dcolon_by_src(node, checked_var_src)
    }

    fn check_dotless_dcolon_by_src(&self, node: &Node, checked_var_src: &str) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if !has_dot(&call) {
                let n = call.name();
                if name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") || is_operator(n.as_slice()) {
                    return true;
                }
            }
            if is_double_colon(self.source, &call) {
                return true;
            }
            if let Some(recv) = call.receiver() {
                if src_match_ignoring_safe_nav(node_src(self.source, &recv), checked_var_src) {
                    return false;
                }
                return self.check_dotless_dcolon_by_src(&recv, checked_var_src);
            }
        }
        false
    }

    fn is_dotless_call_on_var_by_span(&self, body: &Node, checked_var_span: Span) -> bool {
        let checked_var_src = span_src(self.source, checked_var_span);
        if let Node::CallNode { .. } = body {
            let call = body.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                if src_match_ignoring_safe_nav(node_src(self.source, &recv), checked_var_src) && !has_dot(&call) {
                    return true;
                }
            }
        }
        false
    }

    /// Check if operator without dot anywhere in chain (for ternary)
    /// Check if any ancestor in the chain is unsafe
    fn has_unsafe_ancestor(&self, node: &Node, checked_var_src: &str) -> bool {
        // For if/unless bodies, walk up the chain checking for unsafe methods
        // This mirrors RuboCop's `unsafe_method_used?` which checks ancestors
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(self.source, &recv);
                if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                    // Reached the checked var, no more ancestors to check
                    return false;
                }
                // Check if this intermediate call in chain is unsafe
                if !self.safe_navigation_chain_enabled {
                    return true;
                }
                if self.is_negated(&recv) {
                    return true;
                }
                // Recurse into receiver
                return self.has_unsafe_ancestor(&recv, checked_var_src);
            }
        }
        false
    }

    /// Check if a node is negated (method call with !)
    fn is_negated(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if name_eq(call.name().as_slice(), "!") {
                return true;
            }
        }
        false
    }

    // ---- Body skip checks ----

    /// Run body checks where checked_var is identified by span in self.source
    fn should_skip_body_by_src(&self, body: &Node, checked_var_span: Span) -> bool {
        if self.is_logic_jump(body) { return true; }
        if self.is_assignment_call(body) { return true; }
        if self.ends_with_empty(body) { return true; }
        if !self.find_matching_receiver_by_span(body, checked_var_span) { return true; }
        if self.chain_length_by_span(body, checked_var_span) > self.max_chain_length { return true; }
        if self.chain_has_dotless_or_dcolon_by_span(body, checked_var_span) { return true; }
        if self.is_operator_call(body) { return true; }
        if self.is_dotless_call_on_var_by_span(body, checked_var_span) { return true; }
        if self.is_allowed_method(body) { return true; }
        if self.ends_with_nil_check(body) { return true; }
        if self.is_negation(body) { return true; }
        // Check for unsafe ancestors (SafeNavigationChain)
        let checked_var_src = span_src(self.source, checked_var_span);
        if self.has_unsafe_ancestor(body, checked_var_src) { return true; }
        false
    }

    // ---- IfNode handling ----

    fn check_if(&mut self, node: &ruby_prism::IfNode) {
        let node_loc = node.location();
        let node_src_text = &self.source[node_loc.start_offset()..node_loc.end_offset()];

        // Skip elsif nodes (RuboCop: allowed_if_condition? returns true for elsif)
        if node_src_text.starts_with("elsif") {
            return;
        }

        // Detect ternary: not starting with if/unless, has subsequent, contains ?
        let is_ternary = !node_src_text.starts_with("if")
            && !node_src_text.starts_with("unless")
            && node.subsequent().is_some()
            && node_src_text.contains('?');

        if is_ternary {
            self.check_ternary(node);
            return;
        }

        // Skip if/else
        if node.subsequent().is_some() {
            return;
        }

        let condition = node.predicate();
        let is_unless = node_src_text.starts_with("unless");

        let (checked_var_span, is_nil_form) = match self.extract_checked_var_span(&condition) {
            Some(v) => v,
            None => return,
        };

        // unless foo (plain variable) -> don't flag
        if is_unless {
            if is_simple_var(&condition) { return; }
            let cond_src = node_src(self.source, &condition);
            if !is_nil_form && !cond_src.starts_with('!') { return; }
        }

        // if !foo (negated without nil?) -> don't flag
        if !is_unless && !is_nil_form {
            let cond_src = node_src(self.source, &condition);
            if cond_src.starts_with('!') { return; }
        }

        // Get single body statement
        let body_node = match node.statements() {
            Some(stmts) => {
                let body = stmts.body();
                if body.len() != 1 { return; }
                body.iter().next().unwrap()
            }
            None => return,
        };

        let checked_var_src = span_src(self.source, checked_var_span);

        // Run body checks
        if self.should_skip_body_by_src(&body_node, checked_var_span) {
            return;
        }

        // Additional check for dotless operator on the direct method call
        if let Node::CallNode { .. } = &body_node {
            let _call = body_node.as_call_node().unwrap();
            let method_call_receiver = self.find_receiver_matching_var(&body_node, checked_var_src);
            if let Some(method_call_node) = method_call_receiver {
                // Check dotless_operator_call on the method call (not the receiver)
                if self.is_dotless_operator_method(&method_call_node) {
                    return;
                }
                // Check double colon
                if let Node::CallNode { .. } = &method_call_node {
                    if is_double_colon(self.source, &method_call_node.as_call_node().unwrap()) {
                        return;
                    }
                }
            }
        }

        // Compute offense range: use end of first line (matches RuboCop annotation style)
        let offense_end = end_of_first_line(self.source, node_loc.start_offset());
        let location = Location::from_offsets(self.source, node_loc.start_offset(), offense_end);
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        // Build correction
        let body_src = node_src(self.source, &body_node);
        let corrected_body = self.add_safe_nav_all(body_src, checked_var_src);

        // Handle comments inside the if expression
        let full_node_src = &self.source[node_loc.start_offset()..node_loc.end_offset()];
        let corrected = self.build_if_correction(full_node_src, &corrected_body, &body_node, node_loc.start_offset());

        offense = offense.with_correction(Correction::replace(
            node_loc.start_offset(), node_loc.end_offset(), corrected,
        ));

        self.offenses.push(offense);
    }

    /// Find the parent CallNode whose receiver matches checked_var
    fn find_receiver_matching_var<'b>(&self, node: &Node<'b>, checked_var_src: &str) -> Option<Node<'b>> {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(self.source, &recv);
                if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                    // The parent of recv is this call node
                    return Some(call.as_node());
                }
                return self.find_receiver_matching_var(&recv, checked_var_src);
            }
        }
        None
    }

    /// Check if a call is a dotless operator method ([], []=, or operator)
    fn is_dotless_operator_method(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if call.call_operator_loc().is_none() {
                let n = call.name();
                return name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") || is_operator(n.as_slice());
            }
        }
        false
    }

    /// Build correction for if/unless expression, handling comments
    fn build_if_correction(&self, if_src: &str, corrected_body: &str, body_node: &Node, if_start_offset: usize) -> String {
        // Collect comments from inside the if expression
        let mut comments_before = Vec::new();
        let mut trailing_comment = String::new();

        let lines: Vec<&str> = if_src.lines().collect();
        let body_loc = body_node.location();
        let body_start_offset = body_loc.start_offset();
        let body_end_offset = body_loc.end_offset();

        for (i, line) in lines.iter().enumerate() {
            let line_trimmed = line.trim();

            // Skip the first line (if/unless ...) and the last line (end)
            // but extract inline comments from them
            if i == 0 {
                // Check for inline comment on the if/unless line
                if let Some(hash_pos) = find_comment_in_line(line) {
                    let comment = line[hash_pos..].trim();
                    if !comment.is_empty() {
                        comments_before.push(comment.to_string());
                    }
                }
                continue;
            }

            if i == lines.len() - 1 {
                // Last line - check for inline comment on `end`
                if let Some(hash_pos) = find_comment_in_line(line) {
                    trailing_comment = format!(" {}", line[hash_pos..].trim());
                }
                continue;
            }

            // For middle lines, check if they are comment-only lines
            if line_trimmed.starts_with('#') {
                // Check if this comment is inside an inner node (body)
                // by checking line position relative to body
                let line_offset = if_start_offset + if_src[..].lines().take(i).map(|l| l.len() + 1).sum::<usize>();
                if line_offset < body_start_offset || line_offset >= body_end_offset {
                    comments_before.push(line_trimmed.to_string());
                }
                // Comments inside inner nodes (body) stay with the body - they're already handled
                // Actually, in RuboCop's comment handling, comments within inner nodes are NOT moved.
                // But comments between if-line and body, and between body and end, ARE moved.
                // For simplicity, we move all non-inner comments.
                continue;
            }
        }

        let mut result = String::new();
        for comment in &comments_before {
            result.push_str(comment);
            result.push('\n');
        }
        result.push_str(corrected_body);
        result.push_str(&trailing_comment);
        result
    }

    // ---- UnlessNode handling ----

    fn check_unless(&mut self, node: &ruby_prism::UnlessNode) {
        let node_loc = node.location();

        if node.else_clause().is_some() {
            return;
        }

        let condition = node.predicate();

        let (checked_var_span, _is_nil_form) = match self.extract_checked_var_span(&condition) {
            Some(v) => v,
            None => return,
        };

        if is_simple_var(&condition) { return; }

        let cond_src = node_src(self.source, &condition);
        if !_is_nil_form && !cond_src.starts_with('!') { return; }

        let body_node = match node.statements() {
            Some(stmts) => {
                let body = stmts.body();
                if body.len() != 1 { return; }
                body.iter().next().unwrap()
            }
            None => return,
        };

        let checked_var_src = span_src(self.source, checked_var_span);

        if self.should_skip_body_by_src(&body_node, checked_var_span) {
            return;
        }

        // Compute offense range: use end of first line (matches RuboCop annotation style)
        let offense_end = end_of_first_line(self.source, node_loc.start_offset());
        let location = Location::from_offsets(self.source, node_loc.start_offset(), offense_end);
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        let body_src = node_src(self.source, &body_node);
        let corrected_body = self.add_safe_nav_all(body_src, checked_var_src);

        // Handle comments
        let full_node_src = &self.source[node_loc.start_offset()..node_loc.end_offset()];
        let corrected = self.build_if_correction(full_node_src, &corrected_body, &body_node, node_loc.start_offset());

        offense = offense.with_correction(Correction::replace(
            node_loc.start_offset(), node_loc.end_offset(), corrected,
        ));

        self.offenses.push(offense);
    }

    // ---- Ternary handling ----

    fn check_ternary(&mut self, node: &ruby_prism::IfNode) {
        let condition = node.predicate();

        let then_node = node.statements().and_then(|stmts| {
            let body = stmts.body();
            if body.len() == 1 { body.iter().next() } else { None }
        });
        let else_node = node.subsequent().and_then(|sub| {
            if let Node::ElseNode { .. } = &sub {
                let els = sub.as_else_node().unwrap();
                els.statements().and_then(|stmts| {
                    let body = stmts.body();
                    if body.len() == 1 { body.iter().next() } else { None }
                })
            } else {
                None
            }
        });

        let (then_node, else_node) = match (then_node, else_node) {
            (Some(t), Some(e)) => (t, e),
            _ => return,
        };

        let then_is_nil = matches!(then_node, Node::NilNode { .. });
        let else_is_nil = matches!(else_node, Node::NilNode { .. });

        if (!then_is_nil && !else_is_nil) || (then_is_nil && else_is_nil) {
            return;
        }

        // Determine which side has the method call and extract checked var
        let (checked_var_span, _method_call_span) = if else_is_nil {
            // `cond ? method : nil`
            let cond_src = node_src(self.source, &condition);
            match self.extract_checked_var_span(&condition) {
                Some((var_span, is_nil)) => {
                    if is_nil && !cond_src.starts_with('!') { return; }
                    if !is_nil && cond_src.starts_with('!') { return; }
                    (var_span, node_span(&then_node))
                }
                None => return,
            }
        } else {
            // `cond ? nil : method`
            let cond_src = node_src(self.source, &condition);
            match self.extract_checked_var_span(&condition) {
                Some((var_span, is_nil)) => {
                    if !is_nil && !cond_src.starts_with('!') && is_simple_var(&condition) { return; }
                    if is_nil && cond_src.starts_with('!') { return; }
                    (var_span, node_span(&else_node))
                }
                None => return,
            }
        };

        let method_node = if else_is_nil { then_node } else { else_node };
        let checked_var_src = span_src(self.source, checked_var_span);

        // Standard body checks for ternary
        // RuboCop's unsafe_method? returns false for ternary, so assignment/negation checks are skipped
        // But dotless operators in chain and allowed methods still apply
        if self.is_logic_jump(&method_node) { return; }
        if self.ends_with_empty(&method_node) { return; }
        if !self.find_matching_receiver_by_span(&method_node, checked_var_span) { return; }
        if self.chain_length_by_span(&method_node, checked_var_span) > self.max_chain_length { return; }
        if self.chain_has_dotless_or_dcolon_by_span(&method_node, checked_var_span) { return; }
        if self.is_allowed_method(&method_node) { return; }
        if self.ends_with_nil_check(&method_node) { return; }
        if self.is_negation(&method_node) { return; }

        // Additional ternary check: dotless_operator_call? on the direct method call
        if let Some(method_call) = self.find_receiver_matching_var(&method_node, checked_var_src) {
            if self.is_dotless_operator_method(&method_call) {
                return;
            }
            if let Node::CallNode { .. } = &method_call {
                if is_double_colon(self.source, &method_call.as_call_node().unwrap()) {
                    return;
                }
            }
        }

        let node_loc = node.location();
        let location = Location::from_offsets(self.source, node_loc.start_offset(), node_loc.end_offset());
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        let method_src = node_src(self.source, &method_node);
        let var_src = span_src(self.source, checked_var_span);
        let corrected = self.add_safe_nav_all(method_src, var_src);
        offense = offense.with_correction(Correction::replace(
            node_loc.start_offset(), node_loc.end_offset(), corrected,
        ));

        self.offenses.push(offense);
    }

    // ---- AndNode handling ----
    //
    // This follows RuboCop's algorithm for processing && chains:
    // 1. Collect "parts" from ALL descendant AND nodes (each_descendant(:and))
    // 2. Each AND contributes: operator span, possibly LHS, possibly RHS
    // 3. Parts are sorted by position, grouped into slices of 2 (node, operator)
    // 4. Consecutive pairs of slices are checked for safe navigation offenses

    fn check_and(&mut self, node: &ruby_prism::AndNode) {
        let and_node = node.as_node();
        let pairs = self.collect_and_clause_pairs(&and_node);

        // Track the rhs_span of previous offending pairs to detect overlapping corrections
        let mut prev_rhs_end: Option<usize> = None;
        for (lhs_span, lhs_op_span, rhs_span) in &pairs {
            let is_overlap = prev_rhs_end.map_or(false, |end| lhs_span.0 < end);
            if self.check_and_pair_rubocop(lhs_span, lhs_op_span, rhs_span, is_overlap) {
                prev_rhs_end = Some(rhs_span.1);
            }
        }
    }

    /// Represents a collected "part" - either a node span or an operator span
    /// We tag them to distinguish during processing but sort them all by position.
    fn collect_and_clause_pairs(&self, node: &Node) -> Vec<(Span, Span, Span)> {
        // Step 1: Collect all parts from the top-level AND and all descendant AND nodes
        let mut parts: Vec<(Span, bool)> = Vec::new(); // (span, is_operator)

        // and_parts for the top-level node
        self.rubocop_and_parts(node, &mut parts);

        // Find all descendant AND nodes and collect their parts
        self.collect_descendant_and_parts(node, &mut parts);

        // Step 2: Sort by position
        parts.sort_by_key(|&(span, _)| span.0);

        // Step 3: Group into slices of 2 (like RuboCop's each_slice(2))
        // Each slice should be (node_span, operator_span)
        let mut slices: Vec<(Span, Option<Span>)> = Vec::new();
        let mut i = 0;
        while i < parts.len() {
            let (span1, is_op1) = parts[i];
            if i + 1 < parts.len() {
                let (span2, is_op2) = parts[i + 1];
                if !is_op1 && is_op2 {
                    // (node, operator) pair
                    slices.push((span1, Some(span2)));
                    i += 2;
                } else if !is_op1 && !is_op2 {
                    // Two nodes in a row - first one has no operator (last element)
                    slices.push((span1, None));
                    i += 1;
                } else {
                    // Operator without preceding node? Skip it.
                    i += 1;
                }
            } else {
                // Last element - single node without operator
                if !is_op1 {
                    slices.push((span1, None));
                }
                i += 1;
            }
        }

        // Step 4: Take consecutive pairs (each_cons(2))
        let mut result = Vec::new();
        for j in 0..slices.len().saturating_sub(1) {
            let (lhs_span, lhs_op) = slices[j];
            let (rhs_span, _rhs_op) = slices[j + 1];
            if let Some(op_span) = lhs_op {
                result.push((lhs_span, op_span, rhs_span));
            }
        }

        result
    }

    /// RuboCop's `and_parts(node)`: extract operator, maybe rhs, maybe lhs from an AND node
    fn rubocop_and_parts(&self, node: &Node, parts: &mut Vec<(Span, bool)>) {
        if let Node::AndNode { .. } = node {
            let and = node.as_and_node().unwrap();

            // Add operator
            let op_loc = and.operator_loc();
            let op_span = (op_loc.start_offset(), op_loc.end_offset());
            parts.push((op_span, true));

            let rhs = and.right();
            let lhs = and.left();

            // Add RHS unless and_inside_begin?(rhs) - i.e., rhs is/contains a paren wrapping an AND
            if !self.and_inside_begin(&rhs) {
                parts.push((node_span(&rhs), false));
            }

            // Add LHS unless lhs is an AND type or and_inside_begin?(lhs)
            if !matches!(lhs, Node::AndNode { .. }) && !self.and_inside_begin(&lhs) {
                parts.push((node_span(&lhs), false));
            }
        }
    }

    /// Find all descendant AND nodes and collect their parts
    fn collect_descendant_and_parts(&self, node: &Node, parts: &mut Vec<(Span, bool)>) {
        // Walk all descendants looking for AND nodes
        // Skip the top-level node itself (already processed)
        self.walk_descendants_for_and(node, parts, true);
    }

    fn walk_descendants_for_and(&self, node: &Node, parts: &mut Vec<(Span, bool)>, is_top: bool) {
        match node {
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                if !is_top {
                    // Check if this AND is inside a block - if so, skip (RuboCop's concat_nodes check)
                    if self.has_block_ancestor_in_source(node) {
                        return;
                    }
                    // Collect parts from this descendant AND
                    self.rubocop_and_parts(node, parts);
                }
                // Continue walking children to find more ANDs
                self.walk_descendants_for_and(&and.left(), parts, false);
                self.walk_descendants_for_and(&and.right(), parts, false);
            }
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    self.walk_descendants_for_and(&body, parts, false);
                }
            }
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for stmt in stmts.body().iter() {
                    self.walk_descendants_for_and(&stmt, parts, false);
                }
            }
            Node::OrNode { .. } => {
                // Walk into OR nodes to find AND descendants
                let or = node.as_or_node().unwrap();
                self.walk_descendants_for_and(&or.left(), parts, false);
                self.walk_descendants_for_and(&or.right(), parts, false);
            }
            _ => {
                // Don't recurse into other node types (calls, blocks, etc.)
            }
        }
    }

    /// Check if a node is conceptually inside a block (RuboCop: and_node.each_ancestor(:block).any?)
    /// Since we walk the tree top-down, we approximate this by checking the source
    fn has_block_ancestor_in_source(&self, _node: &Node) -> bool {
        // In our tree walking, we don't recurse into BlockNode/LambdaNode,
        // so any AND node we find won't be inside a block.
        false
    }

    /// RuboCop's `and_inside_begin?`: checks if a node contains (in descendants)
    /// a parenthesized expression wrapping an AND node.
    /// Pattern: `(begin and ...)` - a begin/paren node whose child is an AND.
    fn and_inside_begin(&self, node: &Node) -> bool {
        self.check_and_inside_begin_recursive(node)
    }

    fn check_and_inside_begin_recursive(&self, node: &Node) -> bool {
        match node {
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    // Check if body is/contains an AND
                    if self.body_contains_and(&body) {
                        return true;
                    }
                    // Also recurse to check deeper
                    return self.check_and_inside_begin_recursive(&body);
                }
                false
            }
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for stmt in stmts.body().iter() {
                    if self.check_and_inside_begin_recursive(&stmt) {
                        return true;
                    }
                }
                false
            }
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                self.check_and_inside_begin_recursive(&and.left())
                    || self.check_and_inside_begin_recursive(&and.right())
            }
            Node::OrNode { .. } => {
                let or = node.as_or_node().unwrap();
                self.check_and_inside_begin_recursive(&or.left())
                    || self.check_and_inside_begin_recursive(&or.right())
            }
            _ => false,
        }
    }

    fn body_contains_and(&self, node: &Node) -> bool {
        match node {
            Node::AndNode { .. } => true,
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                stmts.body().iter().any(|s| matches!(s, Node::AndNode { .. }))
            }
            _ => false,
        }
    }

    /// Check a pair of and-clauses (RuboCop style with separate lhs, operator, and rhs spans)
    /// `lhs_overlaps_prev_rhs`: true if this pair's LHS overlaps with a previous pair's RHS
    fn check_and_pair_rubocop(
        &mut self,
        lhs_span: &Span,
        lhs_op_span: &Span,
        rhs_span: &Span,
        lhs_overlaps_prev_rhs: bool,
    ) -> bool {
        let lhs_src = span_src(self.source, *lhs_span);
        let rhs_src = span_src(self.source, *rhs_span);

        // Parse both sub-expressions to analyze structure
        let lhs_parsed = ruby_prism::parse(lhs_src.as_bytes());
        let rhs_parsed = ruby_prism::parse(rhs_src.as_bytes());

        let lhs_node = {
            let prog = lhs_parsed.node();
            let prog = prog.as_program_node().unwrap();
            let stmts = prog.statements();
            let body = stmts.body();
            if body.len() != 1 {
                return false;
            }
            body.iter().next().unwrap()
        };
        let rhs_node = {
            let prog = rhs_parsed.node();
            let prog = prog.as_program_node().unwrap();
            let stmts = prog.statements();
            let body = stmts.body();
            if body.len() != 1 {
                return false;
            }
            body.iter().next().unwrap()
        };

        // Check if LHS is !foo.nil?
        let is_not_nil = self.is_not_nil_check(&lhs_node);

        if is_not_nil && !self.convert_nil {
            return false;
        }

        // Extract checked variable source text from LHS
        let checked_var_src: String = if is_not_nil {
            match self.not_nil_receiver_src(lhs_src, &lhs_node) {
                Some(s) => s.to_string(),
                None => return false,
            }
        } else {
            match self.extract_and_lhs_src(lhs_src, &lhs_node) {
                Some(s) => s.to_string(),
                None => return false,
            }
        };

        // Validate LHS is a proper object check
        if !is_not_nil && !self.is_valid_and_lhs(&lhs_node) {
            return false;
        }

        // Check lhs_method_chain == lhs_receiver (RuboCop: find_method_chain check)
        // Ensures LHS is the checked variable itself, not a method chain containing it
        // (already handled by extract_and_lhs_src returning the whole LHS)

        // Unwrap parens on RHS (RuboCop's strip_begin)
        let actual_rhs = self.unwrap_parens_parsed(&rhs_node);
        // Find matching receiver in RHS
        if !self.find_matching_receiver_src(&actual_rhs, rhs_src, &checked_var_src) {
            return false;
        }

        // Standard checks
        if self.is_operator_call(&actual_rhs) { return false; }
        if self.chain_length_src(&actual_rhs, rhs_src, &checked_var_src) > self.max_chain_length { return false; }
        if self.chain_has_dotless_or_dcolon_src(&actual_rhs, rhs_src, &checked_var_src) { return false; }
        if self.is_assignment_call(&actual_rhs) { return false; }
        if self.is_allowed_method(&actual_rhs) { return false; }
        if self.ends_with_nil_check(&actual_rhs) { return false; }
        if self.is_negation(&actual_rhs) { return false; }
        if self.ends_with_empty(&actual_rhs) { return false; }
        if self.is_negation(&rhs_node) { return false; }

        // Check for unsafe ancestors (SafeNavigationChain)
        if self.has_unsafe_ancestor_src(&actual_rhs, rhs_src, &checked_var_src) { return false; }

        // Create offense with range from lhs start to rhs end
        let location = Location::from_offsets(self.source, lhs_span.0, rhs_span.1);
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        // Build correction using granular edits (not for OR-containing RHS).
        // Unlike RuboCop which uses ignore_node to prevent duplicate corrections,
        // we generate granular edits for ALL offenses. The apply_corrections engine
        // handles overlapping edits by skipping them, which produces the correct
        // composed result in a single pass.
        let rhs_contains_or = self.node_contains_or(&rhs_node);
        if !rhs_contains_or {
            let edits = self.build_and_correction_edits(
                lhs_span, lhs_op_span, rhs_span,
                &checked_var_src, rhs_src, &actual_rhs,
                rhs_span.0, lhs_overlaps_prev_rhs,
            );
            if !edits.is_empty() {
                offense = offense.with_correction(Correction { edits });
            }
        }

        self.offenses.push(offense);
        true
    }

    /// Build granular correction edits for an AND-pair offense.
    ///
    /// Two strategies based on whether this pair overlaps with a previous pair:
    ///
    /// **Non-overlapping** (first pair or independent):
    ///   Following RuboCop: delete LHS + operator (lhs_start to op_end+ws),
    ///   leaving the RHS including any parens. Then insert `&` at dots.
    ///   For `foo && foo.bar`: Delete(0,7) + Insert(&,10) = "foo&.bar"
    ///   For `foo && (foo.bar? && ...)`: Delete(0,7) + Insert(&,11) = "(foo&.bar?...)"
    ///
    /// **Overlapping** (LHS is previous pair's RHS):
    ///   For the second pair in `foo && foo.bar && foo.bar.baz`:
    ///   Delete operator+ws, Delete receiver in RHS, Insert `&` at dots.
    fn build_and_correction_edits(
        &self,
        lhs_span: &Span,
        lhs_op_span: &Span,
        _rhs_span: &Span,
        checked_var_src: &str,
        rhs_src: &str,
        actual_rhs: &Node,
        rhs_base_offset: usize,
        lhs_overlaps_prev_rhs: bool,
    ) -> Vec<crate::offense::Edit> {
        use crate::offense::Edit;
        let mut edits = Vec::new();

        // Find dot positions in the chain
        let mut dot_positions: Vec<usize> = Vec::new();
        self.collect_chain_dots(actual_rhs, rhs_src, checked_var_src, rhs_base_offset, &mut dot_positions);

        if dot_positions.is_empty() {
            return edits;
        }

        dot_positions.sort();
        let innermost_dot = dot_positions[0];

        if lhs_overlaps_prev_rhs {
            // Overlapping case: don't delete LHS (it's handled by previous pair's correction).
            // Only delete the operator and the receiver in the RHS.

            // Delete operator + surrounding whitespace
            let op_start_with_ws = self.skip_left_whitespace(lhs_op_span.0);
            let op_end_with_ws = self.skip_right_whitespace(lhs_op_span.1);
            edits.push(Edit {
                start_offset: op_start_with_ws,
                end_offset: op_end_with_ws,
                replacement: String::new(),
            });

            // Delete receiver text in the RHS (from recv_start to innermost_dot)
            let recv_global_offset = self.find_receiver_global_offset(
                actual_rhs, rhs_src, checked_var_src, rhs_base_offset,
            );
            if let Some((recv_start, _)) = recv_global_offset {
                edits.push(Edit {
                    start_offset: recv_start,
                    end_offset: innermost_dot,
                    replacement: String::new(),
                });
            }
        } else {
            // Non-overlapping case: Delete entire LHS + operator + trailing whitespace.
            // This follows RuboCop which removes lhs.source_range (right side space)
            // and operator_range (right side space) separately.
            //
            // For "foo && foo.bar": Delete(0, 7) removes "foo && "
            // For "!foo.nil? && foo.bar": Delete(0, 14) removes "!foo.nil? && "
            // For "foo && (foo.bar? && ...)": Delete(0, 7) removes "foo && "
            //
            // The RHS receiver (foo) stays - only the dots get & inserted.

            let op_end_with_ws = self.skip_right_whitespace(lhs_op_span.1);
            edits.push(Edit {
                start_offset: lhs_span.0,
                end_offset: op_end_with_ws,
                replacement: String::new(),
            });

            // RuboCop: corrector.replace(rhs_receiver, lhs_receiver.source)
            // Replace the receiver in the RHS with checked_var_src if they differ.
            // This handles cases like `foo&.bar && foo.bar.baz` where the RHS receiver
            // is `foo.bar` (without &.) but checked_var is `foo&.bar` (with &.).
            let recv_global_offset = self.find_receiver_global_offset(
                actual_rhs, rhs_src, checked_var_src, rhs_base_offset,
            );
            if let Some((recv_start, recv_end)) = recv_global_offset {
                let recv_text = &self.source[recv_start..recv_end];
                if recv_text != checked_var_src {
                    edits.push(Edit {
                        start_offset: recv_start,
                        end_offset: recv_end,
                        replacement: checked_var_src.to_string(),
                    });
                }
            }
        }

        // Insert `&` before each `.` in the chain
        for &dot_pos in &dot_positions {
            if dot_pos < self.source.len() && self.source.as_bytes()[dot_pos] == b'.' {
                if dot_pos == 0 || self.source.as_bytes()[dot_pos - 1] != b'&' {
                    edits.push(Edit {
                        start_offset: dot_pos,
                        end_offset: dot_pos,
                        replacement: "&".to_string(),
                    });
                }
            }
        }

        edits
    }

    /// Skip whitespace to the left of a position in the source
    fn skip_left_whitespace(&self, pos: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut p = pos;
        while p > 0 && (bytes[p - 1] == b' ' || bytes[p - 1] == b'\t') {
            p -= 1;
        }
        p
    }

    /// Find the global (start, end) offset of the receiver in the RHS that matches checked_var_src
    fn find_receiver_global_offset(
        &self,
        rhs_node: &Node,
        rhs_src: &str,
        checked_var_src: &str,
        rhs_base_offset: usize,
    ) -> Option<Span> {
        if let Node::CallNode { .. } = rhs_node {
            let call = rhs_node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_local_src = node_src(rhs_src, &recv);
                if src_match_ignoring_safe_nav(recv_local_src, checked_var_src) {
                    let recv_loc = recv.location();
                    return Some((
                        rhs_base_offset + recv_loc.start_offset(),
                        rhs_base_offset + recv_loc.end_offset(),
                    ));
                }
                return self.find_receiver_global_offset(&recv, rhs_src, checked_var_src, rhs_base_offset);
            }
        }
        None
    }

    /// Skip whitespace to the right of a position in the source
    fn skip_right_whitespace(&self, pos: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut p = pos;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        p
    }

    /// Collect all dot positions in the chain from receiver to outermost call.
    /// Returns global offsets of each `.` operator in the chain.
    fn collect_chain_dots(
        &self,
        node: &Node,
        rhs_src: &str,
        checked_var_src: &str,
        rhs_base_offset: usize,
        dots: &mut Vec<usize>,
    ) {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if let Some(recv) = call.receiver() {
                    let recv_src = node_src(rhs_src, &recv);

                    // Add this call's dot to the list
                    if let Some(dot_loc) = call.call_operator_loc() {
                        let dot_global = rhs_base_offset + dot_loc.start_offset();
                        let dot_end = rhs_base_offset + dot_loc.end_offset();
                        let dot_src = &self.source[dot_global..dot_end];
                        if dot_src == "." || dot_src == "&." {
                            dots.push(dot_global);
                        }
                    }

                    // If receiver matches checked var, stop recursion
                    if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                        return;
                    }

                    // Recurse into receiver for deeper chain dots
                    self.collect_chain_dots(&recv, rhs_src, checked_var_src, rhs_base_offset, dots);
                }
            }
            _ => {}
        }
    }

    /// Check for unsafe ancestor in a sub-parsed chain
    fn has_unsafe_ancestor_src(&self, node: &Node, parent_src: &str, checked_var_src: &str) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(parent_src, &recv);
                if src_match_ignoring_safe_nav(recv_src, checked_var_src) {
                    return false;
                }
                // If SafeNavigationChain is disabled, any chain is unsafe
                if !self.safe_navigation_chain_enabled {
                    return true;
                }
                if self.is_negated(&recv) {
                    return true;
                }
                return self.has_unsafe_ancestor_src(&recv, parent_src, checked_var_src);
            }
        }
        false
    }

    /// Get the source text of the receiver in `!foo.nil?`
    fn not_nil_receiver_src<'b>(&self, parent_src: &'b str, node: &Node) -> Option<&'b str> {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if name_eq(call.name().as_slice(), "!") {
                if let Some(recv) = call.receiver() {
                    if let Node::CallNode { .. } = &recv {
                        let inner = recv.as_call_node().unwrap();
                        if name_eq(inner.name().as_slice(), "nil?") {
                            if let Some(inner_recv) = inner.receiver() {
                                return Some(node_src(parent_src, &inner_recv));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract checked variable source text from an and-LHS
    fn extract_and_lhs_src<'b>(&self, parent_src: &'b str, lhs: &Node) -> Option<&'b str> {
        if is_simple_var(lhs) {
            return Some(node_src(parent_src, lhs));
        }

        match lhs {
            Node::CallNode { .. } => {
                let call = lhs.as_call_node().unwrap();
                if call.call_operator_loc().is_some() || call.receiver().is_some() {
                    Some(node_src(parent_src, lhs))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Check if LHS is a valid object check for &&
    fn is_valid_and_lhs(&self, lhs: &Node) -> bool {
        if is_simple_var(lhs) {
            return true;
        }

        match lhs {
            Node::CallNode { .. } => {
                let call = lhs.as_call_node().unwrap();
                if call.call_operator_loc().is_some() || call.receiver().is_some() {
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Unwrap parens on a node from a sub-parse
    fn unwrap_parens_parsed<'b>(&self, node: &Node<'b>) -> Node<'b> {
        match node {
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if let Node::StatementsNode { .. } = &body {
                        let stmts = body.as_statements_node().unwrap();
                        let body_list = stmts.body();
                        if body_list.len() == 1 {
                            let first = body_list.iter().next().unwrap();
                            return self.unwrap_parens_parsed(&first);
                        }
                    } else {
                        return self.unwrap_parens_parsed(&body);
                    }
                }
                dup_node(node)
            }
            _ => dup_node(node),
        }
    }

    fn node_contains_or(&self, node: &Node) -> bool {
        match node {
            Node::OrNode { .. } => true,
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if let Node::StatementsNode { .. } = &body {
                        let stmts = body.as_statements_node().unwrap();
                        for stmt in stmts.body().iter() {
                            if self.node_contains_or(&stmt) {
                                return true;
                            }
                        }
                    } else {
                        return self.node_contains_or(&body);
                    }
                }
                false
            }
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                self.node_contains_or(&and.left()) || self.node_contains_or(&and.right())
            }
            _ => false,
        }
    }

    // ---- Correction ----

    /// Add `&.` to ALL method calls in the chain from checked variable to outermost call.
    /// This replaces every `.` with `&.` between the checked variable and the end of the chain.
    fn add_safe_nav_all(&self, method_src: &str, var_src: &str) -> String {
        // Normalize var_src for matching (handle &. vs .)
        let norm_var = var_src.replace("&.", ".");

        // Find the var in method_src
        let (pos, matched_len) = if let Some(p) = method_src.find(var_src) {
            (p, var_src.len())
        } else if let Some(p) = method_src.find(&norm_var) {
            (p, norm_var.len())
        } else {
            return method_src.to_string();
        };

        let before = &method_src[..pos];
        // Use the original var_src (with any &.) for the replacement prefix
        let after = &method_src[pos + matched_len..];

        let mut result = String::new();
        result.push_str(before);
        result.push_str(var_src);

        // Now replace ALL `.` with `&.` in the remaining chain
        // But be careful: don't replace `.` inside strings, blocks, or arguments
        let mut chars = after.chars().peekable();
        let mut depth = 0i32; // Track nesting depth for parens/blocks
        let mut in_string = false;
        let mut string_delim = '"';

        while let Some(ch) = chars.next() {
            match ch {
                '"' | '\'' if !in_string => {
                    in_string = true;
                    string_delim = ch;
                    result.push(ch);
                }
                c if in_string && c == string_delim => {
                    in_string = false;
                    result.push(ch);
                }
                _ if in_string => {
                    result.push(ch);
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    result.push(ch);
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    result.push(ch);
                }
                '.' if depth == 0 => {
                    // Check for `..` or `...` range operator
                    if chars.peek() == Some(&'.') {
                        result.push('.');
                    } else if result.ends_with('&') {
                        // Already has &. prefix
                        result.push('.');
                    } else {
                        result.push_str("&.");
                    }
                }
                '&' if depth == 0 && chars.peek() == Some(&'.') => {
                    // Already a safe navigation call
                    result.push('&');
                }
                _ => {
                    result.push(ch);
                }
            }
        }
        result
    }
}

/// Find position of a `#` comment in a line (not inside a string)
fn find_comment_in_line(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut delim = '"';
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '"' | '\'' if !in_string => {
                in_string = true;
                delim = ch;
            }
            c if in_string && c == delim => {
                in_string = false;
            }
            '#' if !in_string => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Duplicate/reconstruct a Node by going through its typed form's as_node()
fn dup_node<'pr>(node: &Node<'pr>) -> Node<'pr> {
    match node {
        Node::LocalVariableReadNode { .. } => node.as_local_variable_read_node().unwrap().as_node(),
        Node::ConstantReadNode { .. } => node.as_constant_read_node().unwrap().as_node(),
        Node::CallNode { .. } => node.as_call_node().unwrap().as_node(),
        Node::AndNode { .. } => node.as_and_node().unwrap().as_node(),
        Node::OrNode { .. } => node.as_or_node().unwrap().as_node(),
        Node::ParenthesesNode { .. } => node.as_parentheses_node().unwrap().as_node(),
        Node::NilNode { .. } => node.as_nil_node().unwrap().as_node(),
        Node::IfNode { .. } => node.as_if_node().unwrap().as_node(),
        Node::UnlessNode { .. } => node.as_unless_node().unwrap().as_node(),
        Node::ElseNode { .. } => node.as_else_node().unwrap().as_node(),
        Node::StatementsNode { .. } => node.as_statements_node().unwrap().as_node(),
        Node::BreakNode { .. } => node.as_break_node().unwrap().as_node(),
        Node::NextNode { .. } => node.as_next_node().unwrap().as_node(),
        Node::ReturnNode { .. } => node.as_return_node().unwrap().as_node(),
        Node::YieldNode { .. } => node.as_yield_node().unwrap().as_node(),
        Node::IntegerNode { .. } => node.as_integer_node().unwrap().as_node(),
        Node::FloatNode { .. } => node.as_float_node().unwrap().as_node(),
        Node::StringNode { .. } => node.as_string_node().unwrap().as_node(),
        Node::SymbolNode { .. } => node.as_symbol_node().unwrap().as_node(),
        Node::TrueNode { .. } => node.as_true_node().unwrap().as_node(),
        Node::FalseNode { .. } => node.as_false_node().unwrap().as_node(),
        Node::SelfNode { .. } => node.as_self_node().unwrap().as_node(),
        Node::InstanceVariableReadNode { .. } => node.as_instance_variable_read_node().unwrap().as_node(),
        Node::ClassVariableReadNode { .. } => node.as_class_variable_read_node().unwrap().as_node(),
        Node::GlobalVariableReadNode { .. } => node.as_global_variable_read_node().unwrap().as_node(),
        Node::ConstantPathNode { .. } => node.as_constant_path_node().unwrap().as_node(),
        Node::BlockNode { .. } => node.as_block_node().unwrap().as_node(),
        Node::ArrayNode { .. } => node.as_array_node().unwrap().as_node(),
        Node::HashNode { .. } => node.as_hash_node().unwrap().as_node(),
        Node::InterpolatedStringNode { .. } => node.as_interpolated_string_node().unwrap().as_node(),
        Node::RegularExpressionNode { .. } => node.as_regular_expression_node().unwrap().as_node(),
        Node::BeginNode { .. } => node.as_begin_node().unwrap().as_node(),
        Node::RangeNode { .. } => node.as_range_node().unwrap().as_node(),
        Node::LambdaNode { .. } => node.as_lambda_node().unwrap().as_node(),
        Node::DefNode { .. } => node.as_def_node().unwrap().as_node(),
        Node::ClassNode { .. } => node.as_class_node().unwrap().as_node(),
        Node::ModuleNode { .. } => node.as_module_node().unwrap().as_node(),
        Node::LocalVariableWriteNode { .. } => node.as_local_variable_write_node().unwrap().as_node(),
        Node::InstanceVariableWriteNode { .. } => node.as_instance_variable_write_node().unwrap().as_node(),
        Node::MultiWriteNode { .. } => node.as_multi_write_node().unwrap().as_node(),
        Node::SplatNode { .. } => node.as_splat_node().unwrap().as_node(),
        _ => {
            #[cfg(debug_assertions)]
            panic!("dup_node: unhandled node type");
            #[cfg(not(debug_assertions))]
            node.as_nil_node().unwrap().as_node()
        }
    }
}

impl<'a> Visit<'_> for SafeNavVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.check_if(node);
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.check_unless(node);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        self.check_and(node);
        // Don't recurse into children - flatten already handled nested ands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cops;
    use ruby_prism::parse;

    fn check(source: &str) -> Vec<Offense> {
        let cop: Box<dyn Cop> = Box::new(SafeNavigation::new());
        let cops_list = vec![cop];
        let result = parse(source.as_bytes());
        cops::run_cops_with_version(&cops_list, &result, source, "test.rb", 2.7)
    }

    #[test]
    fn test_and_basic() {
        let offenses = check("foo && foo.bar");
        assert_eq!(offenses.len(), 1, "Should detect foo && foo.bar");
    }

    #[test]
    fn test_if_basic() {
        let offenses = check("foo.bar if foo");
        assert_eq!(offenses.len(), 1, "Should detect foo.bar if foo");
    }

    #[test]
    fn test_no_offense_for_non_object_check() {
        let offenses = check("x.foo? && x.bar?");
        assert_eq!(offenses.len(), 0, "Should not flag non-object checks");
    }
}
