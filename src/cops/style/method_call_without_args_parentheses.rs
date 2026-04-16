//! Style/MethodCallWithoutArgsParentheses - Checks for unwanted parens in
//! parameterless method calls.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/method_call_without_args_parentheses.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/MethodCallWithoutArgsParentheses";
const MSG: &str = "Do not use parentheses for method calls with no arguments.";

pub struct MethodCallWithoutArgsParentheses {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl Default for MethodCallWithoutArgsParentheses {
    fn default() -> Self { Self { allowed_methods: Vec::new(), allowed_patterns: Vec::new() } }
}

impl MethodCallWithoutArgsParentheses {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { allowed_methods, allowed_patterns }
    }
}

impl Cop for MethodCallWithoutArgsParentheses {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            ancestors: Vec::new(),
            block_no_delim_depth: 0,
            allowed_methods: &self.allowed_methods,
            allowed_patterns: &self.allowed_patterns,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a, 'pr> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Kinds of ancestors (from outermost to innermost), excluding the current node.
    ancestors: Vec<AncestorKind<'pr>>,
    block_no_delim_depth: usize,
    allowed_methods: &'a [String],
    allowed_patterns: &'a [String],
}

#[derive(Clone, Copy)]
enum AncestorKind<'pr> {
    OptionalParameter,
    /// LocalVariableWrite with the given var name
    LocalAsgn(&'pr [u8]),
    /// MultiWrite — we need to inspect for variable_in_mass_assignment
    MultiWrite,
    /// OrWrite / AndWrite / OperatorWrite where lhs is a CallNode (obj.meth ||= x)
    ///   Means the call in rhs should NOT be skipped by same-name check
    ShorthandOnCall,
    /// OrWrite / AndWrite / OperatorWrite where lhs is a variable with given name
    ShorthandOnVar(&'pr [u8]),
    Other,
}

impl<'a, 'pr> Visitor<'a, 'pr> {
    fn check_call(&mut self, node: &ruby_prism::CallNode<'pr>) {
        // no args AND parens present
        let has_args = node.arguments().is_some();
        let open = node.opening_loc();
        let close = node.closing_loc();
        let (open_loc, close_loc) = match (open, close) {
            (Some(o), Some(c)) => (o, c),
            _ => return,
        };
        // opening must be `(` not `[` or other
        let open_src = &self.ctx.source[open_loc.start_offset()..open_loc.end_offset()];
        if open_src != "(" { return; }
        if has_args { return; }

        let name = node_name!(node);
        let name_str = name.as_ref();

        // ineligible: camel_case_method (name starts with uppercase)
        if name_str.chars().next().map_or(false, |c| c.is_ascii_uppercase()) { return; }

        // implicit_call (`foo.()` — name is "call" but no message_loc)
        if name_str == "call" && node.message_loc().is_none() { return; }

        // prefix_not: `not(x)` — name is `!` and message_loc text is `not`
        if name_str == "!" {
            if let Some(msg_loc) = node.message_loc() {
                let msg_src = &self.ctx.source[msg_loc.start_offset()..msg_loc.end_offset()];
                if msg_src == "not" || msg_src == "!" { return; }
            }
        }

        // default_argument: parent is OptionalParameter
        if matches!(self.ancestors.last(), Some(AncestorKind::OptionalParameter)) { return; }

        // Allowed methods
        if is_method_allowed(self.allowed_methods, self.allowed_patterns, name_str, None) {
            return;
        }

        // same_name_assignment
        if self.same_name_assignment(node, name_str) { return; }

        // parenthesized `it` method in block with empty_and_without_delimiters params
        if self.parenthesized_it_in_bare_block(node, name_str) { return; }

        // register offense at `(` to `)`
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME, MSG, Severity::Convention,
            open_loc.start_offset(), close_loc.end_offset(),
        ));
    }

    fn same_name_assignment(&self, node: &ruby_prism::CallNode<'pr>, name: &str) -> bool {
        // RuboCop: skip if receiver present
        if node.receiver().is_some() { return false; }
        // Walk outwards through ancestors, find any ASSIGNMENT-kind
        for a in self.ancestors.iter().rev() {
            match a {
                AncestorKind::LocalAsgn(var) => {
                    let var_str = std::str::from_utf8(var).unwrap_or("");
                    if var_str == name { return true; }
                }
                AncestorKind::MultiWrite => {
                    // conservative: we handled via targets below; if we can't tell, assume no
                    // handled in visit_multi_write_node by pushing specific LocalAsgn for each target
                }
                AncestorKind::ShorthandOnCall => {
                    // Skip: lhs is call type — RuboCop's `any_assignment?` continues past this
                    continue;
                }
                AncestorKind::ShorthandOnVar(var) => {
                    let var_str = std::str::from_utf8(var).unwrap_or("");
                    if var_str == name { return true; }
                }
                _ => {}
            }
        }
        false
    }

    fn parenthesized_it_in_bare_block(&self, node: &ruby_prism::CallNode<'pr>, name: &str) -> bool {
        if name != "it" { return false; }
        if node.receiver().is_some() { return false; }
        if node.block().is_some() { return false; }
        // Must be in block with empty_and_without_delimiters (no pipes/params)
        self.block_no_delim_depth > 0
    }

    fn push_and_visit(&mut self, kind: AncestorKind<'pr>, node: &Node<'pr>) {
        self.ancestors.push(kind);
        self.visit(node);
        self.ancestors.pop();
    }
}

impl<'a, 'pr> Visit<'pr> for Visitor<'a, 'pr> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.check_call(node);
        // Visit children with this call as ancestor
        self.ancestors.push(AncestorKind::Other);
        ruby_prism::visit_call_node(self, node);
        self.ancestors.pop();
    }

    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode<'pr>) {
        self.ancestors.push(AncestorKind::OptionalParameter);
        ruby_prism::visit_optional_parameter_node(self, node);
        self.ancestors.pop();
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        let name = node.name().as_slice();
        self.ancestors.push(AncestorKind::LocalAsgn(name));
        ruby_prism::visit_local_variable_write_node(self, node);
        self.ancestors.pop();
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        let name = node.name().as_slice();
        self.ancestors.push(AncestorKind::ShorthandOnVar(name));
        ruby_prism::visit_local_variable_or_write_node(self, node);
        self.ancestors.pop();
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        let name = node.name().as_slice();
        self.ancestors.push(AncestorKind::ShorthandOnVar(name));
        ruby_prism::visit_local_variable_and_write_node(self, node);
        self.ancestors.pop();
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        let name = node.name().as_slice();
        self.ancestors.push(AncestorKind::ShorthandOnVar(name));
        ruby_prism::visit_local_variable_operator_write_node(self, node);
        self.ancestors.pop();
    }

    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode<'pr>) {
        // obj.method ||= rhs — lhs is CallNode, we mark ShorthandOnCall to not skip rhs by same-name
        self.ancestors.push(AncestorKind::ShorthandOnCall);
        ruby_prism::visit_call_or_write_node(self, node);
        self.ancestors.pop();
    }

    fn visit_call_and_write_node(&mut self, node: &ruby_prism::CallAndWriteNode<'pr>) {
        self.ancestors.push(AncestorKind::ShorthandOnCall);
        ruby_prism::visit_call_and_write_node(self, node);
        self.ancestors.pop();
    }

    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode<'pr>) {
        self.ancestors.push(AncestorKind::ShorthandOnCall);
        ruby_prism::visit_call_operator_write_node(self, node);
        self.ancestors.pop();
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        // For each lhs target that's a LocalVariableTargetNode, push LocalAsgn during rhs visit.
        // But RuboCop uses `variable_in_mass_assignment` — pushes the name to check.
        // Collect all lhs names.
        let lefts: Vec<Node> = node.lefts().iter().collect();
        let mut var_names: Vec<&'pr [u8]> = Vec::new();
        for l in &lefts {
            if let Some(t) = l.as_local_variable_target_node() {
                var_names.push(t.name().as_slice());
            }
        }
        // Push each as LocalAsgn so same-name check triggers; push all then visit
        for n in &var_names {
            self.ancestors.push(AncestorKind::LocalAsgn(n));
        }
        self.ancestors.push(AncestorKind::MultiWrite);
        ruby_prism::visit_multi_write_node(self, node);
        self.ancestors.pop();
        for _ in &var_names {
            self.ancestors.pop();
        }
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        let is_bare = is_block_empty_without_delimiters(node, self.ctx.source);
        if is_bare { self.block_no_delim_depth += 1; }
        ruby_prism::visit_block_node(self, node);
        if is_bare { self.block_no_delim_depth -= 1; }
    }
}

/// `arguments.empty_and_without_delimiters?` in RuboCop:
/// - no parameters AND no `|...|` delimiter
/// e.g. `0.times { it() }` has no `|...|` at all = bare block
/// `0.times { || it() }` has empty `|...|` = NOT bare
fn is_block_empty_without_delimiters(node: &ruby_prism::BlockNode, source: &str) -> bool {
    // If there are params, definitely false (unless params are zero but delimiters present)
    if node.parameters().is_some() {
        // Check if source text for params starts with `|` — if so, has delimiters
        return false;
    }
    // No parameters at all — check source right after the opening brace/do for `|`
    let open = node.opening_loc();
    let after_open = open.end_offset();
    let bytes = source.as_bytes();
    let mut i = after_open;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'|' { return false; }
    true
}
