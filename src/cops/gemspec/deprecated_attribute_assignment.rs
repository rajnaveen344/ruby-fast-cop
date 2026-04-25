//! Gemspec/DeprecatedAttributeAssignment cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const DEPRECATED: &[&str] = &["test_files", "date", "specification_version", "rubygems_version"];

#[derive(Default)]
pub struct DeprecatedAttributeAssignment;

impl DeprecatedAttributeAssignment {
    pub fn new() -> Self { Self }
}

impl Cop for DeprecatedAttributeAssignment {
    fn name(&self) -> &'static str { "Gemspec/DeprecatedAttributeAssignment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();

        let mut finder = GemSpecBlockVarFinder { var_name: None };
        finder.visit(&tree);
        let var = match finder.var_name { Some(v) => v, None => return vec![] };

        let mut collector = Collector {
            var: &var,
            source: ctx.source,
            found: None,
        };
        collector.visit(&tree);

        if let Some((attr, start, end)) = collector.found {
            let msg = format!("Do not set `{}` in gemspec.", attr);
            let (line_start, line_end) = whole_line_range(ctx.source, start, end);
            let offense = ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)
                .with_correction(Correction::delete(line_start, line_end));
            return vec![offense];
        }
        vec![]
    }
}

fn whole_line_range(src: &str, start: usize, end: usize) -> (usize, usize) {
    let bytes = src.as_bytes();
    let mut s = start;
    while s > 0 && bytes[s - 1] != b'\n' { s -= 1; }
    let mut e = end;
    while e < bytes.len() && bytes[e] != b'\n' { e += 1; }
    if e < bytes.len() { e += 1; } // include trailing newline
    (s, e)
}

struct Collector<'a> {
    var: &'a str,
    source: &'a str,
    found: Option<(String, usize, usize)>,
}

impl<'a> Collector<'a> {
    fn check_call_is_deprecated_assign(&mut self, node: &ruby_prism::CallNode) -> Option<(String, usize, usize)> {
        let method = node_name!(node);
        // direct `spec.attr = value`
        for &attr in DEPRECATED {
            let expected = format!("{}=", attr);
            if method.as_ref() == expected {
                if let Some(recv) = node.receiver() {
                    if is_target_var(&recv, self.var) {
                        let loc = node.location();
                        return Some((attr.to_string(), loc.start_offset(), loc.end_offset()));
                    }
                }
            }
        }
        None
    }
}

impl<'a> Visit<'_> for Collector<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.found.is_some() { return; }
        if let Some(x) = self.check_call_is_deprecated_assign(node) { self.found = Some(x); return; }
        ruby_prism::visit_call_node(self, node);
    }
    // op_asgn like `spec.test_files += ...`
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode) {
        if self.found.is_some() { return; }
        let method = String::from_utf8_lossy(node.read_name().as_slice()).to_string();
        if DEPRECATED.contains(&method.as_str()) {
            if let Some(recv) = node.receiver() {
                if is_target_var(&recv, self.var) {
                    let loc = node.location();
                    self.found = Some((method, loc.start_offset(), loc.end_offset()));
                    return;
                }
            }
        }
        ruby_prism::visit_call_operator_write_node(self, node);
    }
}

fn is_target_var(node: &Node, var: &str) -> bool {
    if var == "*" { return true; }
    if let Some(l) = node.as_local_variable_read_node() {
        return String::from_utf8_lossy(l.name().as_slice()) == var;
    }
    false
}

struct GemSpecBlockVarFinder { var_name: Option<String> }

impl Visit<'_> for GemSpecBlockVarFinder {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.var_name.is_some() { return; }
        if node_name!(node).as_ref() == "new" {
            if let Some(recv) = node.receiver() {
                if is_gem_specification(&recv) {
                    if let Some(block) = node.block() {
                        if let Node::BlockNode { .. } = block {
                            let bn = block.as_block_node().unwrap();
                            if let Some(params) = bn.parameters() {
                                if let Some(bp) = params.as_block_parameters_node() {
                                    if let Some(p) = bp.parameters() {
                                        if let Some(first) = p.requireds().iter().next() {
                                            if let Some(rp) = first.as_required_parameter_node() {
                                                self.var_name = Some(String::from_utf8_lossy(rp.name().as_slice()).to_string());
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                            self.var_name = Some("*".to_string());
                            return;
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn is_gem_specification(node: &Node) -> bool {
    let c = match node.as_constant_path_node() { Some(c) => c, None => return false };
    let name = String::from_utf8_lossy(match c.name() { Some(n) => n.as_slice(), None => return false }).to_string();
    if name != "Specification" { return false; }
    match c.parent() {
        Some(Node::ConstantReadNode { .. }) => {
            let p = c.parent().unwrap();
            let pr = p.as_constant_read_node().unwrap();
            String::from_utf8_lossy(pr.name().as_slice()) == "Gem"
        }
        _ => false,
    }
}

crate::register_cop!("Gemspec/DeprecatedAttributeAssignment", |_cfg| Some(Box::new(DeprecatedAttributeAssignment::new())));
