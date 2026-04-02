//! Visit-based AST collectors for gathering variable information.

use super::helpers::name_str;
use super::types::ScopeInfo;
use ruby_prism::Visit;
use std::collections::HashSet;

// ── ReadCollector: find all local variable reads in a subtree ──

pub struct ReadCollector<'a> {
    pub live: &'a mut HashSet<String>,
}

impl Visit<'_> for ReadCollector<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.live.insert(name);
    }

    // Don't descend into scope-creating nodes
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

// ── AllReadCollector: reads including those across rescue boundaries ──

pub struct AllReadCollector<'a> {
    pub reads: &'a mut HashSet<String>,
}

impl Visit<'_> for AllReadCollector<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
    }
    // Op-assign/and-assign/or-assign also read the variable
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

// ── ScopeInfoCollector ──

pub struct ScopeInfoCollector<'a> {
    pub scope: &'a mut ScopeInfo,
}

impl Visit<'_> for ScopeInfoCollector<'_> {
    fn visit_forwarding_super_node(&mut self, _node: &ruby_prism::ForwardingSuperNode) {
        self.scope.has_bare_super = true;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Track variable-like method calls
        if node.receiver().is_none() {
            if let Some(msg_loc) = node.message_loc() {
                let name = String::from_utf8_lossy(msg_loc.as_slice()).to_string();
                let has_args = if let Some(args) = node.arguments() {
                    args.arguments().len() > 0
                } else {
                    false
                };
                if !has_args && node.block().is_none() {
                    self.scope.method_calls.insert(name);
                }
            }
        }
        if node.is_variable_call() {
            if let Some(msg_loc) = node.message_loc() {
                let name = String::from_utf8_lossy(msg_loc.as_slice()).to_string();
                self.scope.method_calls.insert(name);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.scope.all_reads.insert(name);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name.clone());
        self.scope.all_reads.insert(name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name.clone());
        self.scope.all_reads.insert(name);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name.clone());
        self.scope.all_reads.insert(name);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name);
    }

    // Don't descend into scope-creating nodes or blocks
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
    fn visit_block_node(&mut self, _node: &ruby_prism::BlockNode) {}
    fn visit_lambda_node(&mut self, _node: &ruby_prism::LambdaNode) {}
}

// ── VarRefCollector: collect variable references in blocks ──

pub struct VarRefCollector {
    pub referenced_vars: HashSet<String>,
    pub written_vars: HashSet<String>,
}

impl VarRefCollector {
    pub fn new() -> Self {
        Self {
            referenced_vars: HashSet::new(),
            written_vars: HashSet::new(),
        }
    }
}

impl Visit<'_> for VarRefCollector {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = name_str(&node.name());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        let name = name_str(&node.name());
        self.written_vars.insert(name);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name.clone());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name.clone());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name.clone());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

// ── NestedWriteFinder: find writes nested in expressions ──

pub struct NestedWriteFinder {
    // (offset, name, name_start, name_end)
    pub writes: Vec<(usize, String, usize, usize)>,
    pub in_container: bool,
}

impl NestedWriteFinder {
    pub fn new() -> Self {
        Self { writes: Vec::new(), in_container: false }
    }
}

impl Visit<'_> for NestedWriteFinder {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        // Only record if we're inside a container (array, hash, arguments)
        // not at the top level or in a simple chain
        if self.in_container {
            let name = name_str(&node.name());
            self.writes.push((
                node.location().start_offset(),
                name,
                node.name_loc().start_offset(),
                node.name_loc().end_offset(),
            ));
        }
        // Recurse into the value to find deeper nested writes
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    // Track when we're inside a container
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let was = self.in_container;
        self.in_container = true;
        ruby_prism::visit_array_node(self, node);
        self.in_container = was;
    }

    fn visit_arguments_node(&mut self, node: &ruby_prism::ArgumentsNode) {
        let was = self.in_container;
        self.in_container = true;
        ruby_prism::visit_arguments_node(self, node);
        self.in_container = was;
    }

    // Don't descend into scope-creating nodes
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
    fn visit_block_node(&mut self, _node: &ruby_prism::BlockNode) {}
    fn visit_lambda_node(&mut self, _node: &ruby_prism::LambdaNode) {}
}
