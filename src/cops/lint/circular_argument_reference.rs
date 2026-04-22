//! Lint/CircularArgumentReference - Detect circular default argument references.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const MSG: &str = "Circular argument reference - `%s`.";

#[derive(Default)]
pub struct CircularArgumentReference;

impl CircularArgumentReference {
    pub fn new() -> Self { Self }
}

impl Cop for CircularArgumentReference {
    fn name(&self) -> &'static str { "Lint/CircularArgumentReference" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode) {
        let arg_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let value = node.value();
        self.check_value(&arg_name, &value);
        ruby_prism::visit_optional_parameter_node(self, node);
    }

    fn visit_optional_keyword_parameter_node(
        &mut self,
        node: &ruby_prism::OptionalKeywordParameterNode,
    ) {
        let arg_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let value = node.value();
        self.check_value(&arg_name, &value);
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn check_value(&mut self, arg_name: &str, value: &Node) {
        // Direct: `foo = foo`
        if let Some(lv) = value.as_local_variable_read_node() {
            let name = String::from_utf8_lossy(lv.name().as_slice());
            if name == arg_name {
                self.report(value, arg_name);
            }
            return;
        }
        // Assignment chain
        if value.as_local_variable_write_node().is_some() {
            let mut seen = HashSet::new();
            self.check_chain(arg_name, value, &mut seen);
        }
    }

    fn check_chain(&mut self, arg_name: &str, node: &Node, seen: &mut HashSet<String>) {
        if let Some(w) = node.as_local_variable_write_node() {
            let name = String::from_utf8_lossy(w.name().as_slice()).to_string();
            seen.insert(name);
            let val = w.value();
            self.check_chain(arg_name, &val, seen);
        } else if let Some(lv) = node.as_local_variable_read_node() {
            let name = String::from_utf8_lossy(lv.name().as_slice());
            if name == arg_name || seen.contains(name.as_ref()) {
                self.report(node, arg_name);
            }
        }
    }

    fn report(&mut self, node: &Node, arg_name: &str) {
        let loc = node.location();
        self.offenses.push(self.ctx.offense_with_range(
            "Lint/CircularArgumentReference",
            &MSG.replace("%s", arg_name),
            Severity::Warning,
            loc.start_offset(),
            loc.end_offset(),
        ));
    }
}

crate::register_cop!("Lint/CircularArgumentReference", |_cfg| Some(Box::new(CircularArgumentReference::new())));
