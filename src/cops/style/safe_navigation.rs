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
        Node::LocalVariableReadNode { .. } | Node::ConstantReadNode { .. } => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            // A variable_call is `foo` that could be a local variable or bare method
            call.is_variable_call() && call.receiver().is_none() && call.arguments().is_none()
        }
        _ => false,
    }
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
        // For any unhandled type, create a sentinel that will never match
        // This is safe since we only use dup_node for comparison/storage
        _ => {
            // Fallback: just match on the same pointer data in the enum
            // We can use unsafe to copy the raw data since Node is just pointers
            // But let's avoid unsafe. Instead, we'll handle this by not needing dup for unknown types.
            // In practice, the types above cover all cases in SafeNavigation.
            // If we hit this, it means we have a node type we haven't handled.
            // Let's panic in debug mode.
            #[cfg(debug_assertions)]
            panic!("dup_node: unhandled node type");
            #[cfg(not(debug_assertions))]
            node.as_nil_node().unwrap().as_node() // will panic anyway
        }
    }
}

struct SafeNavVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    allowed_methods: &'a [String],
    convert_nil: bool,
    max_chain_length: usize,
    offenses: Vec<Offense>,
}

impl<'a> SafeNavVisitor<'a> {
    /// Compare two nodes by source text. For call nodes, compare ignoring dot/safe-nav type.
    fn nodes_match(&self, a: &Node, b: &Node) -> bool {
        match (a, b) {
            (Node::CallNode { .. }, Node::CallNode { .. }) => {
                let ac = a.as_call_node().unwrap();
                let bc = b.as_call_node().unwrap();
                if ac.name().as_slice() != bc.name().as_slice() {
                    return false;
                }
                match (ac.receiver(), bc.receiver()) {
                    (Some(ar), Some(br)) => {
                        if !self.nodes_match(&ar, &br) {
                            return false;
                        }
                        match (ac.arguments(), bc.arguments()) {
                            (None, None) => true,
                            (Some(aa), Some(ba)) => {
                                let aa_loc = aa.location();
                                let ba_loc = ba.location();
                                &self.source[aa_loc.start_offset()..aa_loc.end_offset()]
                                    == &self.source[ba_loc.start_offset()..ba_loc.end_offset()]
                            }
                            _ => false,
                        }
                    }
                    (None, None) => true,
                    _ => false,
                }
            }
            _ => node_src(self.source, a) == node_src(self.source, b),
        }
    }

    /// Find matching receiver in a call chain.
    /// Walk down receivers from method_chain to find one matching checked_var.
    fn find_matching_receiver(&self, method_chain: &Node, checked_var: &Node) -> bool {
        if let Node::CallNode { .. } = method_chain {
            let call = method_chain.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                if self.nodes_match(&recv, checked_var) {
                    return true;
                }
                return self.find_matching_receiver(&recv, checked_var);
            }
        }
        false
    }

    /// Count chain length from method_chain down to checked_var
    fn chain_length(&self, method_chain: &Node, checked_var: &Node) -> usize {
        let mut count = 0;
        self.count_chain_recursive(method_chain, checked_var, &mut count);
        count
    }

    fn count_chain_recursive(&self, node: &Node, checked_var: &Node, count: &mut usize) {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                *count += 1;
                if self.nodes_match(&recv, checked_var) {
                    return;
                }
                self.count_chain_recursive(&recv, checked_var, count);
            }
        }
    }

    /// Check if chain has a dotless operator ([], []=, +, etc.) or double colon
    fn chain_has_dotless_or_dcolon(&self, method_chain: &Node, checked_var: &Node) -> bool {
        self.check_chain_dotless_recursive(method_chain, checked_var)
    }

    fn check_chain_dotless_recursive(&self, node: &Node, checked_var: &Node) -> bool {
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
                if self.nodes_match(&recv, checked_var) {
                    return false;
                }
                return self.check_chain_dotless_recursive(&recv, checked_var);
            }
        }
        false
    }

    /// Get the terminal (outermost) method name
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

    /// Check if node is `!foo.nil?` and return the span of the inner receiver
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

    /// Extract checked var span from `!foo.nil?`: returns foo's span
    fn not_nil_receiver_span(&self, node: &Node) -> Option<Span> {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if name_eq(call.name().as_slice(), "!") {
                if let Some(recv) = call.receiver() {
                    if let Node::CallNode { .. } = &recv {
                        let inner = recv.as_call_node().unwrap();
                        if name_eq(inner.name().as_slice(), "nil?") {
                            if let Some(inner_recv) = inner.receiver() {
                                return Some(node_span(&inner_recv));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract the checked variable span and whether it's a nil-check form from a condition.
    fn extract_checked_var_span(&self, cond: &Node) -> Option<(Span, bool)> {
        // Simple variable (local var, constant, or variable-call)
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
                        // !foo -> foo
                        if is_simple_var(&recv) {
                            return Some((node_span(&recv), false));
                        }
                    }
                    return None;
                }

                None
            }
            _ => None,
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

    fn is_dotless_call_on_var(&self, body: &Node, checked_var: &Node) -> bool {
        if let Node::CallNode { .. } = body {
            let call = body.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                if self.nodes_match(&recv, checked_var) && !has_dot(&call) {
                    return true;
                }
            }
        }
        false
    }

    /// Check if chain has operator without dot anywhere
    fn has_operator_without_dot(&self, node: &Node, checked_var: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if !has_dot(&call) {
                let n = call.name();
                if is_operator(n.as_slice()) || name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") {
                    return true;
                }
            }
            if let Some(recv) = call.receiver() {
                if self.nodes_match(&recv, checked_var) {
                    return false;
                }
                return self.has_operator_without_dot(&recv, checked_var);
            }
        }
        false
    }

    // ---- Common body checks ----

    /// Run all standard body checks. Returns true if the body should be skipped (not flagged).
    fn should_skip_body(&self, body: &Node, checked_var: &Node) -> bool {
        self.is_logic_jump(body)
            || self.is_assignment_call(body)
            || self.ends_with_empty(body)
            || !self.find_matching_receiver(body, checked_var)
            || self.chain_length(body, checked_var) > self.max_chain_length
            || self.chain_has_dotless_or_dcolon(body, checked_var)
            || self.is_operator_call(body)
            || self.is_dotless_call_on_var(body, checked_var)
            || self.is_allowed_method(body)
            || self.ends_with_nil_check(body)
            || self.is_negation(body)
    }

    // ---- IfNode handling ----

    fn check_if(&mut self, node: &ruby_prism::IfNode) {
        let node_loc = node.location();
        let node_src_text = &self.source[node_loc.start_offset()..node_loc.end_offset()];

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

        // Create a "checked_var" node by re-parsing the span from original source
        // Actually, we can use find_matching_receiver with source text comparison.
        // Let's build a temporary checked_var node using the span.
        let checked_var_src = span_src(self.source, checked_var_span);

        // Run body checks using source-text based matching
        if self.should_skip_body_by_src(&body_node, checked_var_span) {
            return;
        }

        // For nil-form checks, only flag if convert_nil is true
        if is_nil_form && !self.convert_nil {
            return;
        }

        let location = Location::from_offsets(self.source, node_loc.start_offset(), node_loc.end_offset());
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        let body_src = node_src(self.source, &body_node);
        let corrected = self.add_safe_nav(body_src, checked_var_src);
        offense = offense.with_correction(Correction::replace(
            node_loc.start_offset(), node_loc.end_offset(), corrected,
        ));

        self.offenses.push(offense);
    }

    // ---- UnlessNode handling ----

    fn check_unless(&mut self, node: &ruby_prism::UnlessNode) {
        let node_loc = node.location();

        if node.else_clause().is_some() {
            return;
        }

        let condition = node.predicate();

        let (checked_var_span, is_nil_form) = match self.extract_checked_var_span(&condition) {
            Some(v) => v,
            None => return,
        };

        if is_simple_var(&condition) { return; }

        let cond_src = node_src(self.source, &condition);
        if !is_nil_form && !cond_src.starts_with('!') { return; }

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

        if is_nil_form && !self.convert_nil {
            return;
        }

        let location = Location::from_offsets(self.source, node_loc.start_offset(), node_loc.end_offset());
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        let body_src = node_src(self.source, &body_node);
        let corrected = self.add_safe_nav(body_src, checked_var_src);
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

        // Re-get the method call node (we need it for body checks)
        let method_node = if else_is_nil { then_node } else { else_node };

        if self.should_skip_body_by_src(&method_node, checked_var_span) {
            return;
        }

        // Additional ternary-specific checks
        if let Node::CallNode { .. } = &method_node {
            let call = method_node.as_call_node().unwrap();
            if !has_dot(&call) {
                let n = call.name();
                if name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") || is_operator(n.as_slice()) {
                    return;
                }
            }
        }
        if self.has_operator_without_dot_by_src(&method_node, checked_var_span) {
            return;
        }

        let node_loc = node.location();
        let location = Location::from_offsets(self.source, node_loc.start_offset(), node_loc.end_offset());
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        let method_src = node_src(self.source, &method_node);
        let var_src = span_src(self.source, checked_var_span);
        let corrected = self.add_safe_nav(method_src, var_src);
        offense = offense.with_correction(Correction::replace(
            node_loc.start_offset(), node_loc.end_offset(), corrected,
        ));

        self.offenses.push(offense);
    }

    // ---- AndNode handling ----

    fn check_and(&mut self, node: &ruby_prism::AndNode) {
        let and_node = node.as_node();
        let clauses = self.collect_and_clauses(&and_node);

        if clauses.len() < 2 {
            return;
        }

        let mut i = 0;
        while i + 1 < clauses.len() {
            if self.check_and_pair(&clauses[i], &clauses[i + 1]) {
                i += 2;
            } else {
                i += 1;
            }
        }
    }

    /// Collect flattened and-clauses as (Node, Span) tuples.
    /// We need to store nodes, but we can't clone them. Instead, store spans
    /// and re-derive nodes when needed.
    fn collect_and_clauses(&self, node: &Node) -> Vec<Span> {
        let mut spans = Vec::new();
        self.flatten_and(node, &mut spans);
        spans
    }

    fn flatten_and(&self, node: &Node, out: &mut Vec<Span>) {
        match node {
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                self.flatten_and(&and.left(), out);
                self.flatten_and(&and.right(), out);
            }
            Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                if let Some(body) = paren.body() {
                    if let Node::StatementsNode { .. } = &body {
                        let stmts = body.as_statements_node().unwrap();
                        let body_list = stmts.body();
                        if body_list.len() == 1 {
                            let first = body_list.iter().next().unwrap();
                            if let Node::AndNode { .. } = &first {
                                self.flatten_and(&first, out);
                                return;
                            }
                        }
                    } else if let Node::AndNode { .. } = &body {
                        self.flatten_and(&body, out);
                        return;
                    }
                }
                out.push(node_span(node));
            }
            _ => {
                out.push(node_span(node));
            }
        }
    }

    /// Check a pair of and-clauses. Since we only have spans, we need to
    /// sub-parse to work with nodes. However, sub-parsing changes offsets.
    /// Instead, let's parse the LHS and RHS independently and compare source text.
    fn check_and_pair(&mut self, lhs_span: &Span, rhs_span: &Span) -> bool {
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
        if !is_not_nil {
            if !self.is_valid_and_lhs(&lhs_node) {
                return false;
            }
        }

        // Unwrap parens on RHS
        let actual_rhs = self.unwrap_parens_parsed(&rhs_node);
        let actual_rhs_src = node_src(rhs_src, &actual_rhs);

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

        // Create offense
        let location = Location::from_offsets(self.source, lhs_span.0, rhs_span.1);
        let mut offense = Offense::new(COP_NAME, MSG, Severity::Convention, location, self.filename);

        // Build correction (not for OR-containing RHS)
        if !self.node_contains_or(&rhs_node) {
            let corrected = self.add_safe_nav(actual_rhs_src, &checked_var_src);
            let final_corrected = if actual_rhs_src != rhs_src {
                rhs_src.replace(actual_rhs_src, &corrected)
            } else {
                corrected
            };
            offense = offense.with_correction(Correction::replace(lhs_span.0, rhs_span.1, final_corrected));
        }

        self.offenses.push(offense);
        true
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
        // Simple variable first
        if is_simple_var(lhs) {
            return Some(node_src(parent_src, lhs));
        }

        match lhs {
            Node::CallNode { .. } => {
                let call = lhs.as_call_node().unwrap();
                if call.call_operator_loc().is_some() || call.receiver().is_some() {
                    // For `foo.bar` or `foo&.bar`, the "checked var" IS the whole LHS
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
        // Simple variable (including variable_call) is always a valid LHS
        if is_simple_var(lhs) {
            return true;
        }

        match lhs {
            Node::CallNode { .. } => {
                let call = lhs.as_call_node().unwrap();
                // Has dot operator (foo.bar or foo&.bar) or has receiver
                if call.call_operator_loc().is_some() || call.receiver().is_some() {
                    true
                } else {
                    // Bare method without receiver: `foo?` -> not an object check
                    false
                }
            }
            _ => false,
        }
    }

    /// Find matching receiver using source text comparison (for sub-parsed nodes)
    fn find_matching_receiver_src(&self, method_chain: &Node, parent_src: &str, checked_var_src: &str) -> bool {
        if let Node::CallNode { .. } = method_chain {
            let call = method_chain.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(parent_src, &recv);
                if recv_src == checked_var_src {
                    return true;
                }
                return self.find_matching_receiver_src(&recv, parent_src, checked_var_src);
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
                if recv_src == checked_var_src {
                    return;
                }
                self.count_chain_src_recursive(&recv, parent_src, checked_var_src, count);
            }
        }
    }

    /// Check dotless/dcolon using source text comparison
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
                if recv_src == checked_var_src {
                    return false;
                }
                return self.chain_has_dotless_or_dcolon_src(&recv, parent_src, checked_var_src);
            }
        }
        false
    }

    /// Check operator without dot using source text
    fn has_operator_without_dot_by_src(&self, node: &Node, checked_var_span: Span) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if !has_dot(&call) {
                let n = call.name();
                if is_operator(n.as_slice()) || name_eq(n.as_slice(), "[]") || name_eq(n.as_slice(), "[]=") {
                    return true;
                }
            }
            if let Some(recv) = call.receiver() {
                let recv_span = node_span(&recv);
                if recv_span == checked_var_span || span_src(self.source, recv_span) == span_src(self.source, checked_var_span) {
                    return false;
                }
                return self.has_operator_without_dot_by_src(&recv, checked_var_span);
            }
        }
        false
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

    // ---- Body checks using span-based checked_var ----

    /// Run body checks where checked_var is identified by span in self.source
    fn should_skip_body_by_src(&self, body: &Node, checked_var_span: Span) -> bool {
        if self.is_logic_jump(body) { return true; }
        if self.is_assignment_call(body) { return true; }
        if self.ends_with_empty(body) { return true; }

        // find_matching_receiver using span
        if !self.find_matching_receiver_by_span(body, checked_var_span) { return true; }

        // chain_length using span
        if self.chain_length_by_span(body, checked_var_span) > self.max_chain_length { return true; }

        // dotless/dcolon using span
        if self.chain_has_dotless_or_dcolon_by_span(body, checked_var_span) { return true; }

        if self.is_operator_call(body) { return true; }

        // dotless call on var using span
        if self.is_dotless_call_on_var_by_span(body, checked_var_span) { return true; }

        if self.is_allowed_method(body) { return true; }
        if self.ends_with_nil_check(body) { return true; }
        if self.is_negation(body) { return true; }

        false
    }

    fn find_matching_receiver_by_span(&self, method_chain: &Node, checked_var_span: Span) -> bool {
        let checked_var_src = span_src(self.source, checked_var_span);
        if let Node::CallNode { .. } = method_chain {
            let call = method_chain.as_call_node().unwrap();
            if let Some(recv) = call.receiver() {
                let recv_src = node_src(self.source, &recv);
                if recv_src == checked_var_src {
                    return true;
                }
                return self.find_matching_receiver_by_span(&recv, checked_var_span);
            }
        }
        false
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
                if node_src(self.source, &recv) == checked_var_src {
                    return;
                }
                self.count_chain_by_span_recursive(&recv, checked_var_src, count);
            }
        }
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
                if node_src(self.source, &recv) == checked_var_src {
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
                if node_src(self.source, &recv) == checked_var_src && !has_dot(&call) {
                    return true;
                }
            }
        }
        false
    }

    // ---- Correction ----

    /// Add `&.` to the method chain, replacing the first `.` after the checked variable
    fn add_safe_nav(&self, method_src: &str, var_src: &str) -> String {
        if let Some(pos) = method_src.find(var_src) {
            let after = pos + var_src.len();
            let rest = &method_src[after..];
            let mut result = method_src[..after].to_string();
            let mut replaced_first = false;

            for ch in rest.chars() {
                match ch {
                    '.' if !replaced_first => {
                        if result.ends_with('&') {
                            result.push('.');
                        } else {
                            result.push_str("&.");
                        }
                        replaced_first = true;
                    }
                    _ => result.push(ch),
                }
            }
            result
        } else {
            method_src.to_string()
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
