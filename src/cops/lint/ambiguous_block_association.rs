//! Lint/AmbiguousBlockAssociation - Detects ambiguous block association in unparenthesized calls.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/ambiguous_block_association.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_TPL: &str = "Parenthesize the param `{param}` to make sure that the block will be associated with the `{method}` method call.";

#[derive(Default)]
pub struct AmbiguousBlockAssociation {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl AmbiguousBlockAssociation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { allowed_methods, allowed_patterns }
    }
}

impl Cop for AmbiguousBlockAssociation {
    fn name(&self) -> &'static str {
        "Lint/AmbiguousBlockAssociation"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    cop: &'a AmbiguousBlockAssociation,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Is `node` a lambda or proc form?
    /// Matches LambdaNode, `proc { }`, `lambda { }`, `Proc.new { }`, or `Proc.new` (no block).
    fn is_lambda_or_proc(node: &Node) -> bool {
        match node {
            Node::LambdaNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = node_name!(call).to_string();

                if let Some(block) = call.block() {
                    if !matches!(block, Node::BlockNode { .. }) {
                        return false;
                    }
                    // `proc { }` / `lambda { }` (bare receiver)
                    if call.receiver().is_none() && (method == "proc" || method == "lambda") {
                        return true;
                    }
                    // `Proc.new { }`
                    if method == "new" && Self::is_proc_const(&call.receiver()) {
                        return true;
                    }
                    false
                } else {
                    // `Proc.new` (no block)
                    method == "new" && Self::is_proc_const(&call.receiver())
                }
            }
            _ => false,
        }
    }

    fn is_proc_const(recv: &Option<Node>) -> bool {
        match recv {
            Some(Node::ConstantReadNode { .. }) => {
                let n = recv.as_ref().unwrap().as_constant_read_node().unwrap();
                String::from_utf8_lossy(n.name().as_slice()) == "Proc"
            }
            Some(Node::ConstantPathNode { .. }) => {
                let path = recv.as_ref().unwrap().as_constant_path_node().unwrap();
                path.name()
                    .map_or(false, |id| String::from_utf8_lossy(id.as_slice()) == "Proc")
            }
            _ => false,
        }
    }

    fn is_operator_method(method: &str) -> bool {
        matches!(method,
            "==" | "!=" | "===" | "<" | ">" | "<=" | ">=" | "<=>"
            | "+" | "-" | "*" | "/" | "%" | "**"
            | "<<" | ">>" | "&" | "|" | "^" | "~"
            | "!" | "+@" | "-@"
            | "=~" | "!~"
        )
    }

    fn is_assignment_call(call: &ruby_prism::CallNode) -> bool {
        // Attribute writers (`foo.bar = x`) are CallNode with equal_loc set and is_attribute_write
        call.is_attribute_write() || call.equal_loc().is_some()
    }

    fn check(&mut self, node: &ruby_prism::CallNode) {
        // Must have arguments
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return;
        }
        let last_arg = &arg_list[arg_list.len() - 1];

        // Skip if call is parenthesized
        if node.opening_loc().is_some() {
            return;
        }

        // Skip if last arg is lambda/proc
        if Self::is_lambda_or_proc(last_arg) {
            return;
        }

        // Skip if call is an assignment, operator method, or `[]`
        let outer_method = node_name!(node).to_string();
        if Self::is_operator_method(&outer_method) || outer_method == "[]" || outer_method == "[]=" {
            return;
        }
        if Self::is_assignment_call(node) {
            return;
        }

        // Last arg must be a CallNode with a block, and that inner call must have no args.
        let last_call = match last_arg {
            Node::CallNode { .. } => last_arg.as_call_node().unwrap(),
            _ => return,
        };
        if last_call.block().is_none() {
            return;
        }
        if !matches!(last_call.block(), Some(Node::BlockNode { .. })) {
            return;
        }
        if last_call.arguments().is_some() {
            return;
        }

        // Check allowed methods/patterns based on inner call's method name + send source
        let inner_method = node_name!(last_call).to_string();
        let inner_send_src = self.ctx.src(
            last_call.location().start_offset(),
            // "send_source" in RuboCop = the source up through the message; for `a` = "a".
            // For `receive(:on_int).twice` (chain), it's the entire pre-block expression.
            // Use the call's range minus the block. Safest: the call source up to block_opening_loc.
            last_call.block().as_ref().map_or(
                last_call.location().end_offset(),
                |b| match b {
                    Node::BlockNode { .. } => b.as_block_node().unwrap().opening_loc().start_offset(),
                    _ => last_call.location().end_offset(),
                },
            ),
        ).trim_end().to_string();

        if is_method_allowed(
            &self.cop.allowed_methods,
            &self.cop.allowed_patterns,
            &inner_method,
            Some(&inner_send_src),
        ) {
            return;
        }

        // Build offense
        let outer_loc = node.location();
        let param_src = self.ctx.src(
            last_arg.location().start_offset(),
            last_arg.location().end_offset(),
        );
        let message = MSG_TPL
            .replace("{param}", param_src)
            .replace("{method}", &inner_send_src);

        // Build correction: remove space between selector and first arg, wrap with parens
        let mut offense = self.ctx.offense_with_range(
            "Lint/AmbiguousBlockAssociation",
            &message,
            Severity::Warning,
            outer_loc.start_offset(),
            outer_loc.end_offset(),
        );

        if let Some(selector_loc) = node.message_loc() {
            let space_start = selector_loc.end_offset();
            let first_arg_start = arg_list[0].location().start_offset();
            let last_arg_end = last_arg.location().end_offset();
            // remove [space_start..first_arg_start), insert "(" at space_start, insert ")" after last_arg_end
            let correction = Correction {
                edits: vec![
                    Edit { start_offset: space_start, end_offset: first_arg_start, replacement: "(".to_string() },
                    Edit { start_offset: last_arg_end, end_offset: last_arg_end, replacement: ")".to_string() },
                ],
            };
            offense = offense.with_correction(correction);
        }

        self.offenses.push(offense);
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/AmbiguousBlockAssociation", |cfg| {
    let cop_config = cfg.get_cop_config("Lint/AmbiguousBlockAssociation");
    let allowed_methods = cop_config
        .and_then(|c| c.raw.get("AllowedMethods"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let allowed_patterns = cop_config
        .and_then(|c| c.raw.get("AllowedPatterns"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    Some(Box::new(AmbiguousBlockAssociation::with_config(allowed_methods, allowed_patterns)))
});
