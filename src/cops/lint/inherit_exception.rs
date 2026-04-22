//! Lint/InheritException - Inherit from StandardError instead of Exception.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use serde::Deserialize;

const MSG: &str = "Inherit from `%s` instead of `Exception`.";

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct Cfg {
    enforced_style: String,
}

pub struct InheritException {
    preferred: &'static str,
}

impl InheritException {
    pub fn new(preferred: &'static str) -> Self { Self { preferred } }
}

impl Cop for InheritException {
    fn name(&self) -> &'static str { "Lint/InheritException" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new(), preferred: self.preferred };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    preferred: &'static str,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(parent) = node.superclass() {
            if is_exception_const(&parent) && !self.has_local_exception_sibling(node) {
                let msg = MSG.replace("%s", self.preferred);
                let loc = parent.location();
                let mut offense = self.ctx.offense_with_range(
                    "Lint/InheritException",
                    &msg,
                    Severity::Warning,
                    loc.start_offset(),
                    loc.end_offset(),
                );
                offense = offense.with_correction(Correction::replace(
                    loc.start_offset(),
                    loc.end_offset(),
                    self.preferred.to_string(),
                ));
                self.offenses.push(offense);
            }
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Class.new(Exception)
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method == "new" {
            if let Some(recv) = node.receiver() {
                if let Some(cr) = recv.as_constant_read_node() {
                    let name = String::from_utf8_lossy(cr.name().as_slice());
                    if name == "Class" {
                        if let Some(args) = node.arguments() {
                            let arg_list: Vec<_> = args.arguments().iter().collect();
                            if let Some(first) = arg_list.first() {
                                if is_exception_const(first) {
                                    let msg = MSG.replace("%s", self.preferred);
                                    let loc = first.location();
                                    let mut offense = self.ctx.offense_with_range(
                                        "Lint/InheritException",
                                        &msg,
                                        Severity::Warning,
                                        loc.start_offset(),
                                        loc.end_offset(),
                                    );
                                    offense = offense.with_correction(Correction::replace(
                                        loc.start_offset(),
                                        loc.end_offset(),
                                        self.preferred.to_string(),
                                    ));
                                    self.offenses.push(offense);
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    /// Check if any sibling in the module body defines a class named Exception.
    /// RuboCop's `inherit_exception_class_with_omitted_namespace?` checks left siblings
    /// that have a class identifier named Exception.
    fn has_local_exception_sibling(&self, class_node: &ruby_prism::ClassNode) -> bool {
        // The parent class of the class_node — if Exception has a cbase (::Exception), skip this check
        if let Some(parent) = class_node.superclass() {
            if let Some(cp) = parent.as_constant_path_node() {
                if cp.parent().is_none() {
                    // ::Exception — cbase means explicit global, always flag
                    return false;
                }
            }
        }

        // We need to walk up to find sibling class definitions named Exception.
        // Since we don't have parent access, we use the source: check if the class
        // body's module/class context defines Exception as a class name.
        // Simpler: scan the source text for `class Exception` before current position.
        let superclass = match class_node.superclass() {
            Some(s) => s,
            None => return false,
        };
        let class_start = class_node.location().start_offset();
        let source = self.ctx.source;

        // Look for `class Exception` before this class definition
        let before = &source[..class_start];
        // Find `class Exception` pattern
        let mut pos = 0;
        while pos < before.len() {
            if let Some(idx) = before[pos..].find("class Exception") {
                let abs_idx = pos + idx;
                // Make sure it's not `class Exception` as the current class superclass
                // (i.e. this is a sibling definition)
                let after = &before[abs_idx + 5..]; // after "class"
                if after.starts_with(" Exception") || after.starts_with("\tException") {
                    // Check if superclass is plain Exception (not ::Exception)
                    if let Some(cr) = superclass.as_constant_read_node() {
                        let name = String::from_utf8_lossy(cr.name().as_slice());
                        if name == "Exception" {
                            return true;
                        }
                    }
                }
                pos = abs_idx + 1;
            } else {
                break;
            }
        }
        false
    }
}

fn is_exception_const(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            let cr = node.as_constant_read_node().unwrap();
            let name = String::from_utf8_lossy(cr.name().as_slice());
            name == "Exception"
        }
        Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            if cp.parent().is_some() { return false; }
            if let Some(const_id) = cp.name() {
                let name = String::from_utf8_lossy(const_id.as_slice());
                return name == "Exception";
            }
            false
        }
        _ => false,
    }
}

crate::register_cop!("Lint/InheritException", |cfg| {
    let c: Cfg = cfg.typed("Lint/InheritException");
    let preferred: &'static str = match c.enforced_style.as_str() {
        "runtime_error" => "RuntimeError",
        _ => "StandardError",
    };
    Some(Box::new(InheritException::new(preferred)))
});
