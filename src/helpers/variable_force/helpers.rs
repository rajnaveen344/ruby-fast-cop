//! Utility functions for variable force analysis.

use ruby_prism::Visit;
use std::collections::HashSet;

/// Read a ConstantId as String.
pub fn name_str(id: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(id.as_slice()).to_string()
}

/// Extract method parameter names from a DefNode.
pub fn extract_param_names(def: &ruby_prism::DefNode) -> HashSet<String> {
    let mut params = HashSet::new();
    if let Some(parameters) = def.parameters() {
        for p in parameters.requireds().iter() {
            if let Some(rp) = p.as_required_parameter_node() {
                params.insert(name_str(&rp.name()));
            }
        }
        for p in parameters.optionals().iter() {
            if let Some(op) = p.as_optional_parameter_node() {
                params.insert(name_str(&op.name()));
            }
        }
        if let Some(rest) = parameters.rest() {
            if let Some(rp) = rest.as_rest_parameter_node() {
                if let Some(name_loc) = rp.name_loc() {
                    params.insert(String::from_utf8_lossy(name_loc.as_slice()).to_string());
                }
            }
        }
        for p in parameters.keywords().iter() {
            if let Some(kp) = p.as_required_keyword_parameter_node() {
                let name = name_str(&kp.name());
                params.insert(name.trim_end_matches(':').to_string());
            } else if let Some(kp) = p.as_optional_keyword_parameter_node() {
                let name = name_str(&kp.name());
                params.insert(name.trim_end_matches(':').to_string());
            }
        }
        if let Some(kr) = parameters.keyword_rest() {
            if let Some(krp) = kr.as_keyword_rest_parameter_node() {
                if let Some(name_loc) = krp.name_loc() {
                    params.insert(String::from_utf8_lossy(name_loc.as_slice()).to_string());
                }
            }
        }
        if let Some(block_param) = parameters.block() {
            if let Some(name_loc) = block_param.name_loc() {
                params.insert(String::from_utf8_lossy(name_loc.as_slice()).to_string());
            }
        }
        for p in parameters.posts().iter() {
            if let Some(rp) = p.as_required_parameter_node() {
                params.insert(name_str(&rp.name()));
            }
        }
    }
    params
}

/// Check if a begin node has retry in any of its rescue clauses.
pub fn begin_has_retry(begin: &ruby_prism::BeginNode) -> bool {
    let mut checker = RetryChecker { has_retry: false };
    let mut rescue = begin.rescue_clause();
    while let Some(rc) = rescue {
        if let Some(stmts) = rc.statements() {
            for stmt in stmts.body().iter() {
                checker.visit(&stmt);
            }
        }
        if checker.has_retry { return true; }
        rescue = rc.subsequent();
    }
    false
}

struct RetryChecker {
    has_retry: bool,
}

impl Visit<'_> for RetryChecker {
    fn visit_retry_node(&mut self, _node: &ruby_prism::RetryNode) {
        self.has_retry = true;
    }
    fn visit_begin_node(&mut self, _node: &ruby_prism::BeginNode) {}
}

/// Collect all local variable write names in a node tree.
pub fn collect_all_writes_in_node(node: &ruby_prism::Node, writes: &mut HashSet<String>) {
    let mut collector = WriteCollector { writes };
    collector.visit(node);
}

struct WriteCollector<'a> {
    writes: &'a mut HashSet<String>,
}

impl Visit<'_> for WriteCollector<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = name_str(&node.name());
        self.writes.insert(name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        let name = name_str(&node.name());
        self.writes.insert(name);
    }
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}
