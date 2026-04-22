//! Lint/RaiseException - Use StandardError over Exception.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use serde::Deserialize;

const MSG: &str = "Use `StandardError` over `Exception`.";

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct Cfg {
    allowed_implicit_namespaces: Vec<String>,
}

pub struct RaiseException {
    allowed_implicit_namespaces: Vec<String>,
}

impl RaiseException {
    pub fn new(allowed_implicit_namespaces: Vec<String>) -> Self {
        Self { allowed_implicit_namespaces }
    }
}

impl Cop for RaiseException {
    fn name(&self) -> &'static str { "Lint/RaiseException" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
            allowed_implicit_namespaces: &self.allowed_implicit_namespaces,
            module_stack: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    allowed_implicit_namespaces: &'a [String],
    module_stack: Vec<String>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let name = module_name(node);
        self.module_stack.push(name);
        ruby_prism::visit_module_node(self, node);
        self.module_stack.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if (method == "raise" || method == "fail") && node.receiver().is_none() {
            self.check_raise(node);
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn check_raise(&mut self, node: &ruby_prism::CallNode) {
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() { return; }

        let first = &arg_list[0];

        // Pattern 1: raise Exception / raise Exception, msg
        if let Some((start, end, is_cbase)) = self.extract_exception_const(first) {
            self.report_offense(start, end, is_cbase);
            return;
        }

        // Pattern 2: raise Exception.new(...)
        if let Some(call) = first.as_call_node() {
            let mname = String::from_utf8_lossy(call.name().as_slice());
            if mname == "new" {
                if let Some(recv) = call.receiver() {
                    if let Some((start, end, is_cbase)) = self.extract_exception_const(&recv) {
                        self.report_offense(start, end, is_cbase);
                    }
                }
            }
        }
    }

    /// Returns (start, end, is_cbase) if node is bare Exception or ::Exception.
    fn extract_exception_const(&self, node: &Node) -> Option<(usize, usize, bool)> {
        match node {
            Node::ConstantReadNode { .. } => {
                let cr = node.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(cr.name().as_slice());
                if name != "Exception" { return None; }
                if self.in_allowed_implicit_namespace() { return None; }
                let loc = node.location();
                Some((loc.start_offset(), loc.end_offset(), false))
            }
            Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                if cp.parent().is_some() { return None; } // Foo::Exception
                let const_id = cp.name()?;
                let name = String::from_utf8_lossy(const_id.as_slice());
                if name != "Exception" { return None; }
                let loc = node.location();
                Some((loc.start_offset(), loc.end_offset(), true))
            }
            _ => None,
        }
    }

    fn in_allowed_implicit_namespace(&self) -> bool {
        if let Some(top) = self.module_stack.last() {
            return self.allowed_implicit_namespaces.iter().any(|n| n == top);
        }
        false
    }

    fn report_offense(&mut self, start: usize, end: usize, is_cbase: bool) {
        let replacement = if is_cbase { "::StandardError" } else { "StandardError" };
        let mut offense = self.ctx.offense_with_range(
            "Lint/RaiseException",
            MSG,
            Severity::Warning,
            start,
            end,
        );
        offense = offense.with_correction(Correction::replace(start, end, replacement.to_string()));
        self.offenses.push(offense);
    }
}

fn module_name(node: &ruby_prism::ModuleNode) -> String {
    let constant = node.constant_path();
    if let Some(cr) = constant.as_constant_read_node() {
        String::from_utf8_lossy(cr.name().as_slice()).to_string()
    } else {
        String::new()
    }
}

crate::register_cop!("Lint/RaiseException", |cfg| {
    let c: Cfg = cfg.typed("Lint/RaiseException");
    let namespaces = if c.allowed_implicit_namespaces.is_empty() {
        vec!["Gem".to_string()]
    } else {
        c.allowed_implicit_namespaces
    };
    Some(Box::new(RaiseException::new(namespaces)))
});
