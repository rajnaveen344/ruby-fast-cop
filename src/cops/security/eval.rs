//! Security/Eval cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct Eval;

impl Eval {
    pub fn new() -> Self { Self }

    /// Check if arg is safe (literal string, or interpolated string with only literal expressions)
    /// Mirrors RuboCop's pattern `$!str` with `code.dstr_type? && code.recursive_literal?` check
    fn is_safe_arg(node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::StringNode { .. } => true,
            ruby_prism::Node::InterpolatedStringNode { .. } => {
                let n = node.as_interpolated_string_node().unwrap();
                n.parts().iter().all(|p| Self::is_recursive_literal(&p))
            }
            _ => false,
        }
    }

    /// Check if a node is a "recursive literal" (all parts are literals, no variables)
    fn is_recursive_literal(node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::StringNode { .. } => true,
            ruby_prism::Node::EmbeddedStatementsNode { .. } => {
                let es = node.as_embedded_statements_node().unwrap();
                match es.statements() {
                    Some(stmts) => {
                        let body: Vec<_> = stmts.body().iter().collect();
                        body.len() == 1 && Self::is_literal_value(&body[0])
                    }
                    None => true,
                }
            }
            _ => Self::is_literal_value(node),
        }
    }

    fn is_literal_value(node: &ruby_prism::Node) -> bool {
        matches!(node,
            ruby_prism::Node::IntegerNode { .. } |
            ruby_prism::Node::FloatNode { .. } |
            ruby_prism::Node::RationalNode { .. } |
            ruby_prism::Node::ImaginaryNode { .. } |
            ruby_prism::Node::TrueNode { .. } |
            ruby_prism::Node::FalseNode { .. } |
            ruby_prism::Node::NilNode { .. } |
            ruby_prism::Node::StringNode { .. } |
            ruby_prism::Node::SymbolNode { .. }
        )
    }
}

impl Cop for Eval {
    fn name(&self) -> &'static str { "Security/Eval" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "eval" {
            return vec![];
        }

        let receiver = node.receiver();

        // Allowed receivers: nil (bare eval), binding, Kernel, ::Kernel
        // "something.eval" is NOT flagged (arbitrary receiver)
        let ok_receiver = match &receiver {
            None => true, // bare eval(...)
            Some(r) => match r {
                ruby_prism::Node::LocalVariableReadNode { .. } => {
                    let name = String::from_utf8_lossy(
                        r.as_local_variable_read_node().unwrap().name().as_slice()
                    );
                    name == "binding"
                }
                ruby_prism::Node::CallNode { .. } => {
                    // `binding` is sometimes parsed as a variable-call (CallNode with is_variable_call)
                    let call = r.as_call_node().unwrap();
                    let cname = String::from_utf8_lossy(call.name().as_slice());
                    cname == "binding" && call.receiver().is_none() && call.arguments().is_none()
                }
                ruby_prism::Node::ConstantReadNode { .. } => {
                    let name = node_name!(r.as_constant_read_node().unwrap());
                    name == "Kernel"
                }
                ruby_prism::Node::ConstantPathNode { .. } => {
                    let cp = r.as_constant_path_node().unwrap();
                    // ::Kernel — parent is None (cbase), child is Kernel
                    if cp.parent().is_none() {
                        let child_name = String::from_utf8_lossy(cp.name_loc().as_slice());
                        child_name == "Kernel"
                    } else {
                        false
                    }
                }
                _ => false, // e.g. something.eval — skip
            }
        };

        if !ok_receiver {
            return vec![];
        }

        // No arguments → safe (eval with no args)
        let args = node.arguments();
        if args.is_none() {
            return vec![];
        }
        let first_arg = args.unwrap().arguments().iter().next();
        if first_arg.is_none() {
            return vec![];
        }

        // If first arg is safe (literal or interpolation with only literals) → skip
        if Self::is_safe_arg(&first_arg.unwrap()) {
            return vec![];
        }

        vec![ctx.offense(self.name(), "The use of `eval` is a serious security risk.", self.severity(), &node.message_loc().unwrap())]
    }
}

crate::register_cop!("Security/Eval", |_cfg| Some(Box::new(Eval::new())));
