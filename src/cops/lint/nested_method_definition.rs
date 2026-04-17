//! Lint/NestedMethodDefinition - Checks for nested method definitions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/nested_method_definition.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Method definitions must not be nested. Use `lambda` instead.";

#[derive(Default)]
pub struct NestedMethodDefinition {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl NestedMethodDefinition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self { allowed_methods, allowed_patterns }
    }
}

impl Cop for NestedMethodDefinition {
    fn name(&self) -> &'static str {
        "Lint/NestedMethodDefinition"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            def_depth: 0,
            scoping_depth: 0,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    cop: &'a NestedMethodDefinition,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Number of enclosing DefNodes
    def_depth: usize,
    /// Number of enclosing scoping blocks/sclasses
    scoping_depth: usize,
}

impl<'a> Visitor<'a> {
    /// A def is allowed if its receiver is a variable/constant/call (i.e. defs with dynamic target).
    fn allowed_subject_type(receiver: &Node) -> bool {
        matches!(
            receiver,
            Node::LocalVariableReadNode { .. }
                | Node::InstanceVariableReadNode { .. }
                | Node::ClassVariableReadNode { .. }
                | Node::GlobalVariableReadNode { .. }
                | Node::ConstantReadNode { .. }
                | Node::ConstantPathNode { .. }
                | Node::CallNode { .. }
                | Node::ParenthesesNode { .. }
        )
    }

    /// Is this block's call an eval/exec call (instance_eval, class_eval, etc.)?
    fn is_eval_or_exec_call(&self, call: &ruby_prism::CallNode) -> bool {
        matches!(
            node_name!(call).as_ref(),
            "instance_eval" | "class_eval" | "module_eval"
                | "instance_exec" | "class_exec" | "module_exec"
        )
    }

    /// Class.new / Module.new / Struct.new / Data.define, optionally ::-prefixed.
    fn is_class_constructor(&self, call: &ruby_prism::CallNode) -> bool {
        let method = node_name!(call).to_string();
        let recv = match call.receiver() { Some(r) => r, None => return false };
        let recv_name = match Self::const_name(&recv) { Some(n) => n, None => return false };

        match (recv_name.as_str(), method.as_str()) {
            ("Class", "new") | ("Module", "new") | ("Struct", "new") => true,
            ("Data", "define") => true,
            _ => false,
        }
    }

    /// Get constant name from ConstantReadNode or a trailing ConstantPathNode (e.g. `::Class` → "Class").
    fn const_name(node: &Node) -> Option<String> {
        match node {
            Node::ConstantReadNode { .. } => {
                Some(node_name!(node.as_constant_read_node().unwrap()).to_string())
            }
            Node::ConstantPathNode { .. } => {
                let path = node.as_constant_path_node().unwrap();
                path.name().map(|id| String::from_utf8_lossy(id.as_slice()).to_string())
            }
            _ => None,
        }
    }

    fn allowed_method_name(&self, call: &ruby_prism::CallNode) -> bool {
        let name = node_name!(call).to_string();
        is_method_allowed(
            &self.cop.allowed_methods,
            &self.cop.allowed_patterns,
            &name,
            None,
        )
    }

    /// Is this block a "scoping" block (eval/exec/constructor/allowed)?
    fn is_scoping_block(&self, call: &ruby_prism::CallNode) -> bool {
        self.is_eval_or_exec_call(call)
            || self.is_class_constructor(call)
            || self.allowed_method_name(call)
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Check receiver type: defs (def self.foo / def obj.foo)
        let allowed_subject = node.receiver().map_or(false, |r| Self::allowed_subject_type(&r));

        // Only report if (a) inside another def, (b) not under any scoping block/sclass,
        // and (c) this def isn't a "dynamic subject" defs (var/const/call).
        if self.def_depth > 0 && self.scoping_depth == 0 && !allowed_subject {
            let loc = node.location();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/NestedMethodDefinition",
                MSG,
                Severity::Warning,
                loc.start_offset(),
                loc.end_offset(),
            ));
        }

        // Walk into the body with def_depth incremented.
        self.def_depth += 1;
        ruby_prism::visit_def_node(self, node);
        self.def_depth -= 1;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // The parent call is walked before the block in Prism's visit order — we don't
        // have direct access here. Instead, use the CallNode visitor to push/pop scoping.
        // Here we walk children normally.
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check if this call has a block and whether that block is a scoping one.
        let has_block = node.block().is_some();
        let is_block_node = matches!(node.block(), Some(Node::BlockNode { .. }));
        let scoping = has_block && is_block_node && self.is_scoping_block(node);

        if scoping {
            self.scoping_depth += 1;
            ruby_prism::visit_call_node(self, node);
            self.scoping_depth -= 1;
        } else {
            ruby_prism::visit_call_node(self, node);
        }
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        self.scoping_depth += 1;
        ruby_prism::visit_singleton_class_node(self, node);
        self.scoping_depth -= 1;
    }
}

crate::register_cop!("Lint/NestedMethodDefinition", |cfg| {
    let cop_config = cfg.get_cop_config("Lint/NestedMethodDefinition");
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
    Some(Box::new(NestedMethodDefinition::with_config(allowed_methods, allowed_patterns)))
});
