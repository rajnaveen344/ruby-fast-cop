//! Gemspec/DuplicatedAssignment cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;
use std::collections::HashMap;

#[derive(Default)]
pub struct DuplicatedAssignment;

impl DuplicatedAssignment {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicatedAssignment {
    fn name(&self) -> &'static str { "Gemspec/DuplicatedAssignment" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();

        // Step 1: find the gemspec block variable name
        let mut block_var_finder = GemSpecBlockVarFinder { var_name: None };
        block_var_finder.visit(&tree);
        let block_var = match block_var_finder.var_name {
            Some(v) => v,
            None => return vec![],
        };

        // Step 2: collect assignment calls on that variable
        let mut collector = AssignmentCollector {
            block_var: &block_var,
            source: ctx.source,
            assignments: Vec::new(),
            indexed_assignments: Vec::new(),
        };
        collector.visit(&tree);

        let mut offenses = Vec::new();

        // Process regular assignment methods (name=, version=, etc.)
        let mut by_method: HashMap<String, Vec<AssignmentInfo>> = HashMap::new();
        for a in collector.assignments {
            by_method.entry(a.method_name.clone()).or_default().push(a);
        }
        for (_method, mut calls) in by_method {
            if calls.len() < 2 { continue; }
            calls.sort_by_key(|c| c.line);
            let first_line = calls[0].line;
            for dup in &calls[1..] {
                let msg = format!(
                    "`{}` method calls already given on line {} of the gemspec.",
                    dup.method_name, first_line
                );
                offenses.push(ctx.offense_with_range(
                    self.name(), &msg, self.severity(),
                    dup.node_start, dup.node_end,
                ));
            }
        }

        // Process indexed assignments (metadata['key']=)
        let mut by_indexed: HashMap<String, Vec<IndexedAssignmentInfo>> = HashMap::new();
        for a in collector.indexed_assignments {
            let key = format!("{}.{}", a.receiver_method, a.key_repr);
            by_indexed.entry(key).or_default().push(a);
        }
        for (_key, mut calls) in by_indexed {
            if calls.len() < 2 { continue; }
            calls.sort_by_key(|c| c.line);
            let first_line = calls[0].line;
            for dup in &calls[1..] {
                let msg = format!(
                    "`{}[{}]=` method calls already given on line {} of the gemspec.",
                    dup.receiver_method, dup.key_repr, first_line
                );
                offenses.push(ctx.offense_with_range(
                    self.name(), &msg, self.severity(),
                    dup.node_start, dup.node_end,
                ));
            }
        }

        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

#[derive(Debug)]
struct AssignmentInfo {
    method_name: String,
    line: usize,
    node_start: usize,
    node_end: usize,
}

#[derive(Debug)]
struct IndexedAssignmentInfo {
    receiver_method: String,
    key_repr: String,
    line: usize,
    node_start: usize,
    node_end: usize,
}

/// Find the Gem::Specification.new block variable name
/// Strategy: find `Gem::Specification.new` call with a block that has a param
struct GemSpecBlockVarFinder {
    var_name: Option<String>,
}

impl Visit<'_> for GemSpecBlockVarFinder {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.var_name.is_some() { return; }

        if node_name!(node) == "new" {
            if let Some(receiver) = node.receiver() {
                if is_gem_specification_const(&receiver) {
                    // Check if there's a block with a param
                    if let Some(block_node) = node.block() {
                        if let ruby_prism::Node::BlockNode { .. } = block_node {
                            let block = block_node.as_block_node().unwrap();
                            if let Some(params) = block.parameters() {
                                match params {
                                    ruby_prism::Node::BlockParametersNode { .. } => {
                                        let bparams = params.as_block_parameters_node().unwrap();
                                        if let Some(params_node) = bparams.parameters() {
                                            let param_list: Vec<_> = params_node.requireds().iter().collect();
                                            if let Some(first) = param_list.first() {
                                                if let ruby_prism::Node::RequiredParameterNode { .. } = first {
                                                    let param = first.as_required_parameter_node().unwrap();
                                                    let name = String::from_utf8_lossy(param.name().as_slice()).to_string();
                                                    self.var_name = Some(name);
                                                    return;
                                                }
                                            }
                                        }
                                    }
                                    ruby_prism::Node::ItParametersNode { .. } => {
                                        // Ruby 3.4 `it` implicit param — use "*" sentinel
                                        self.var_name = Some("*".to_string());
                                        return;
                                    }
                                    _ => {}
                                }
                            }
                            // No explicit param (numbered params _1)
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

fn is_gem_specification_const(node: &ruby_prism::Node) -> bool {
    match node {
        ruby_prism::Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            let child = match cp.name() {
                Some(id) => String::from_utf8_lossy(id.as_slice()).to_string(),
                None => return false,
            };
            if child != "Specification" { return false; }
            match cp.parent() {
                Some(parent) => match parent {
                    ruby_prism::Node::ConstantReadNode { .. } => {
                        let name = node_name!(parent.as_constant_read_node().unwrap());
                        name == "Gem"
                    }
                    ruby_prism::Node::ConstantPathNode { .. } => {
                        let parent_cp = parent.as_constant_path_node().unwrap();
                        let parent_name = match parent_cp.name() {
                            Some(id) => String::from_utf8_lossy(id.as_slice()).to_string(),
                            None => return false,
                        };
                        parent_cp.parent().is_none() && parent_name == "Gem"
                    }
                    _ => false,
                },
                None => false,
            }
        }
        _ => false,
    }
}

struct AssignmentCollector<'a> {
    block_var: &'a str,
    source: &'a str,
    assignments: Vec<AssignmentInfo>,
    indexed_assignments: Vec<IndexedAssignmentInfo>,
}

impl Visit<'_> for AssignmentCollector<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);

        let recv = match node.receiver() {
            Some(r) => r,
            None => {
                ruby_prism::visit_call_node(self, node);
                return;
            }
        };

        // Direct assignment method: ends with '=' and not []=
        if method.ends_with('=') && method != "[]=" && self.is_block_var_receiver(&recv) {
            let loc = node.location();
            let line = 1 + self.source[..loc.start_offset()].bytes().filter(|&b| b == b'\n').count();
            self.assignments.push(AssignmentInfo {
                method_name: method.to_string(),
                line,
                node_start: loc.start_offset(),
                node_end: loc.end_offset(),
            });
        } else if method == "[]=" {
            // spec.metadata['key'] = val → (call (call spec :metadata) :[]= 'key' val)
            if let ruby_prism::Node::CallNode { .. } = recv {
                let inner_call = recv.as_call_node().unwrap();
                let inner_method = node_name!(inner_call);
                if let Some(inner_recv) = inner_call.receiver() {
                    if self.is_block_var_receiver(&inner_recv) {
                        if let Some(args) = node.arguments() {
                            let arg_list: Vec<_> = args.arguments().iter().collect();
                            if arg_list.len() == 2 {
                                if let Some(key_repr) = extract_literal_key(&arg_list[0]) {
                                    let loc = node.location();
                                    let line = 1 + self.source[..loc.start_offset()].bytes().filter(|&b| b == b'\n').count();
                                    self.indexed_assignments.push(IndexedAssignmentInfo {
                                        receiver_method: inner_method.to_string(),
                                        key_repr,
                                        line,
                                        node_start: loc.start_offset(),
                                        node_end: loc.end_offset(),
                                    });
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

impl AssignmentCollector<'_> {
    fn is_block_var_receiver(&self, node: &ruby_prism::Node) -> bool {
        match node {
            ruby_prism::Node::LocalVariableReadNode { .. } => {
                let name = String::from_utf8_lossy(
                    node.as_local_variable_read_node().unwrap().name().as_slice()
                );
                if self.block_var == "*" {
                    name == "_1"
                } else {
                    name == self.block_var || name == "_1"
                }
            }
            ruby_prism::Node::ItLocalVariableReadNode { .. } => {
                // Ruby 3.4 `it` implicit block param
                true
            }
            _ => false,
        }
    }
}

fn extract_literal_key(node: &ruby_prism::Node) -> Option<String> {
    match node {
        ruby_prism::Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            let val = String::from_utf8_lossy(s.unescaped());
            Some(format!("'{val}'"))
        }
        ruby_prism::Node::SymbolNode { .. } => {
            let sym = node.as_symbol_node().unwrap();
            let val = String::from_utf8_lossy(sym.unescaped());
            Some(format!(":{val}"))
        }
        _ => None,
    }
}

crate::register_cop!("Gemspec/DuplicatedAssignment", |_cfg| Some(Box::new(DuplicatedAssignment::new())));
