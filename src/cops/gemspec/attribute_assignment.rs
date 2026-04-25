//! Gemspec/AttributeAssignment cop
//!
//! Flags mixed `spec.attr = …` and `spec.attr[k] = …` assignments for the
//! same attribute inside a `Gem::Specification.new` block.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::{HashMap, HashSet};

const MSG: &str = "Use consistent style for Gemspec attributes assignment.";

#[derive(Default)]
pub struct AttributeAssignment;

impl AttributeAssignment {
    pub fn new() -> Self { Self }
}

impl Cop for AttributeAssignment {
    fn name(&self) -> &'static str { "Gemspec/AttributeAssignment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();

        let mut finder = GemSpecBlockVarFinder { var_name: None };
        finder.visit(&tree);
        let var = match finder.var_name { Some(v) => v, None => return vec![] };

        let mut collector = Collector {
            var: &var,
            direct: HashSet::new(),
            indexed: HashMap::new(),
        };
        collector.visit(&tree);

        let mut offenses = Vec::new();
        for (attr, ranges) in &collector.indexed {
            if !collector.direct.contains(attr) { continue; }
            for &(s, e) in ranges {
                offenses.push(ctx.offense_with_range(self.name(), MSG, self.severity(), s, e));
            }
        }
        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

struct Collector<'a> {
    var: &'a str,
    direct: HashSet<String>,
    indexed: HashMap<String, Vec<(usize, usize)>>,
}

impl<'a> Visit<'_> for Collector<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if method.ends_with('=') && method.as_ref() != "[]=" {
            // spec.attr = value
            if let Some(recv) = node.receiver() {
                if is_target_var(&recv, self.var) {
                    let attr = method.as_ref().trim_end_matches('=').to_string();
                    self.direct.insert(attr);
                }
            }
        } else if method.as_ref() == "[]=" {
            // spec.attr[k] = value  =>  receiver is a CallNode spec.attr
            if let Some(recv) = node.receiver() {
                if let Some(inner) = recv.as_call_node() {
                    if let Some(inner_recv) = inner.receiver() {
                        if is_target_var(&inner_recv, self.var) {
                            let attr = node_name!(inner).to_string();
                            let loc = node.location();
                            self.indexed.entry(attr).or_default()
                                .push((loc.start_offset(), loc.end_offset()));
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn is_target_var(node: &Node, var: &str) -> bool {
    if var == "*" { return true; }
    if let Some(local) = node.as_local_variable_read_node() {
        return String::from_utf8_lossy(local.name().as_slice()) == var;
    }
    false
}

/// Find the `Gem::Specification.new do |spec| ... end` block param name.
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
                                                self.var_name = Some(
                                                    String::from_utf8_lossy(rp.name().as_slice()).to_string()
                                                );
                                                return;
                                            }
                                        }
                                    }
                                } else if matches!(params, Node::ItParametersNode { .. }) {
                                    self.var_name = Some("*".to_string()); return;
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

crate::register_cop!("Gemspec/AttributeAssignment", |_cfg| Some(Box::new(AttributeAssignment::new())));
