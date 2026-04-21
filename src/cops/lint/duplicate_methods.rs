//! Lint/DuplicateMethods - Checks for duplicated instance (or singleton) method definitions.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;
use std::collections::HashMap;

pub struct DuplicateMethods {
    active_support_extensions: bool,
}

impl DuplicateMethods {
    pub fn new() -> Self {
        Self { active_support_extensions: false }
    }

    pub fn with_config(active_support_extensions: bool) -> Self {
        Self { active_support_extensions }
    }
}

impl Default for DuplicateMethods {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct MethodDef {
    line: u32,
    filename: String,
}

impl Cop for DuplicateMethods {
    fn name(&self) -> &'static str { "Lint/DuplicateMethods" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = DuplicateMethodsVisitor {
            ctx, cop: self, offenses: Vec::new(), definitions: HashMap::new(),
            scope_stack: Vec::new(), def_ancestor_stack: Vec::new(),
            inside_if: false, current_rescue_ensure_scope: None,
            rescue_ensure_seen_keys: HashMap::new(), pending_casgn_name: None,
            in_scope_creating_call: false, non_scope_block_depth: 0,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Clone, Debug)]
struct Scope {
    qualified_name: String,
    is_singleton: bool,
    singleton_receiver: Option<String>,
}

struct DuplicateMethodsVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a DuplicateMethods,
    offenses: Vec<Offense>,
    definitions: HashMap<String, MethodDef>,
    scope_stack: Vec<Scope>,
    def_ancestor_stack: Vec<String>,
    inside_if: bool,
    current_rescue_ensure_scope: Option<String>,
    rescue_ensure_seen_keys: HashMap<String, Vec<String>>,
    pending_casgn_name: Option<String>,
    in_scope_creating_call: bool,
    non_scope_block_depth: usize,
}

impl<'a> DuplicateMethodsVisitor<'a> {
    fn current_scope_name(&self) -> String {
        self.scope_stack.last().map_or_else(|| "Object".to_string(), |s| s.qualified_name.clone())
    }

    fn found_method(&mut self, method_display: &str, line: u32, start_offset: usize, end_offset: usize, rescue_ensure_scope: Option<&str>) {
        if self.non_scope_block_depth > 0 { return; }

        let key = if let Some(ancestor_def) = self.def_ancestor_stack.last() {
            format!("{}.{}", ancestor_def, method_display)
        } else {
            method_display.to_string()
        };

        if let Some(existing) = self.definitions.get(&key) {
            if let Some(scope_id) = rescue_ensure_scope {
                let seen = self.rescue_ensure_seen_keys.entry(scope_id.to_string()).or_default();
                if !seen.contains(&key) {
                    seen.push(key.clone());
                    self.definitions.insert(key, MethodDef { line, filename: self.ctx.filename.to_string() });
                    return;
                }
            }

            let message = format!(
                "Method `{}` is defined at both {}:{} and {}:{}.",
                method_display, existing.filename, existing.line, self.ctx.filename, line
            );
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(), &message, self.cop.severity(), start_offset, end_offset,
            ));
        } else {
            self.definitions.insert(key, MethodDef { line, filename: self.ctx.filename.to_string() });
        }
    }

    fn found_instance_method(&mut self, method_name: &str, line: u32, start_offset: usize, end_offset: usize, rescue_ensure_scope: Option<&str>) {
        if let Some(scope) = self.scope_stack.last() {
            if scope.is_singleton {
                let receiver_name = scope.singleton_receiver.clone().unwrap_or_else(|| {
                    if self.scope_stack.len() >= 2 {
                        self.scope_stack[self.scope_stack.len() - 2].qualified_name.clone()
                    } else {
                        "Object".to_string()
                    }
                });
                let display = format!("{}.{}", receiver_name, method_name);
                self.found_method(&display, line, start_offset, end_offset, rescue_ensure_scope);
                return;
            }
        }
        let scope = self.current_scope_name();
        let display = format!("{}#{}", scope, method_name);
        self.found_method(&display, line, start_offset, end_offset, rescue_ensure_scope);
    }

    fn found_class_method(&mut self, method_name: &str, line: u32, start_offset: usize, end_offset: usize) {
        let display = format!("{}.{}", self.current_scope_name(), method_name);
        self.found_method(&display, line, start_offset, end_offset, None);
    }

    fn found_named_receiver_method(&mut self, receiver_name: &str, method_name: &str, line: u32, start_offset: usize, end_offset: usize) {
        if let Some(qualified) = self.lookup_constant(receiver_name) {
            let display = format!("{}.{}", qualified, method_name);
            self.found_method(&display, line, start_offset, end_offset, None);
        }
    }

    fn lookup_constant(&self, const_name: &str) -> Option<String> {
        self.scope_stack.iter().rev().find_map(|scope| {
            let last_segment = scope.qualified_name.rsplit("::").next().unwrap_or(&scope.qualified_name);
            (last_segment == const_name).then(|| scope.qualified_name.clone())
        })
    }

    fn extract_name(&self, node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::SymbolNode { .. } => {
                let loc = node.as_symbol_node().unwrap().value_loc()?;
                self.ctx.source.get(loc.start_offset()..loc.end_offset()).map(|s| s.to_string())
            }
            ruby_prism::Node::StringNode { .. } => {
                let loc = node.as_string_node().unwrap().content_loc();
                self.ctx.source.get(loc.start_offset()..loc.end_offset()).map(|s| s.to_string())
            }
            _ => None,
        }
    }

    fn extract_const_name(&self, node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                Some(node_name!(node.as_constant_read_node().unwrap()).to_string())
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = node.as_constant_path_node().unwrap();
                let name = path_node.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string())?;
                match path_node.parent() {
                    Some(parent) => Some(format!("{}::{}", self.extract_const_name(&parent)?, name)),
                    None => Some(name),
                }
            }
            _ => None,
        }
    }

    fn qualify_name(&self, name: &str) -> String {
        self.scope_stack.last().map_or_else(
            || name.to_string(),
            |parent| format!("{}::{}", parent.qualified_name, name),
        )
    }

    fn extract_receiver_name(&self, node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } | ruby_prism::Node::ConstantPathNode { .. } => {
                self.extract_const_name(node)
            }
            _ => {
                let loc = node.location();
                self.ctx.source.get(loc.start_offset()..loc.end_offset()).map(|s| s.to_string())
            }
        }
    }

    fn check_attr(&mut self, node: &ruby_prism::CallNode) {
        let method_name = node_name!(node).to_string();
        let args = match node.arguments() { Some(args) => args, None => return };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        let line = self.ctx.line_of(node.location().start_offset()) as u32;
        let (start, end) = (node.location().start_offset(), node.location().end_offset());

        match method_name.as_str() {
            "attr_reader" => {
                for arg in &arg_list {
                    if let Some(name) = self.extract_name(arg) {
                        self.found_instance_method(&name, line, start, end, None);
                    }
                }
            }
            "attr_writer" => {
                for arg in &arg_list {
                    if let Some(name) = self.extract_name(arg) {
                        self.found_instance_method(&format!("{}=", name), line, start, end, None);
                    }
                }
            }
            "attr_accessor" => {
                for arg in &arg_list {
                    if let Some(name) = self.extract_name(arg) {
                        self.found_instance_method(&name, line, start, end, None);
                        self.found_instance_method(&format!("{}=", name), line, start, end, None);
                    }
                }
            }
            "attr" => {
                let writable = arg_list.len() == 2 && matches!(&arg_list[1], ruby_prism::Node::TrueNode { .. });
                if let Some(first_arg) = arg_list.first() {
                    if let Some(name) = self.extract_name(first_arg) {
                        self.found_instance_method(&name, line, start, end, None);
                        if writable {
                            self.found_instance_method(&format!("{}=", name), line, start, end, None);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_alias_method_call(&mut self, node: &ruby_prism::CallNode) {
        if self.inside_if { return; }
        let args = match node.arguments() { Some(args) => args, None => return };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() < 2 { return; }

        let new_name = match self.extract_name(&arg_list[0]) { Some(n) => n, None => return };
        let original_name = match self.extract_name(&arg_list[1]) { Some(n) => n, None => return };
        if new_name == original_name { return; }

        let line = self.ctx.line_of(node.location().start_offset()) as u32;
        let rescue_scope = self.current_rescue_ensure_scope.clone();
        self.found_instance_method(&new_name, line, node.location().start_offset(), node.location().end_offset(), rescue_scope.as_deref());
    }

    fn check_delegate(&mut self, node: &ruby_prism::CallNode) {
        if !self.cop.active_support_extensions || self.inside_if { return; }

        let args = match node.arguments() { Some(args) => args, None => return };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() { return; }

        let last_arg = &arg_list[arg_list.len() - 1];
        let kwargs_elements: Vec<ruby_prism::Node> = match last_arg {
            ruby_prism::Node::KeywordHashNode { .. } => last_arg.as_keyword_hash_node().unwrap().elements().iter().collect(),
            _ => return,
        };

        if kwargs_elements.iter().any(|e| matches!(e, ruby_prism::Node::AssocSplatNode { .. })) { return; }

        let to_value = self.find_assoc_value(&kwargs_elements, "to");
        if to_value.is_none() { return; }

        let prefix = self.get_delegate_prefix(&kwargs_elements, &to_value);
        if prefix.as_deref() == Some("__dynamic__") { return; }

        let line = self.ctx.line_of(node.location().start_offset()) as u32;
        let (start, end) = (node.location().start_offset(), node.location().end_offset());

        for i in 0..arg_list.len() - 1 {
            if let Some(name) = self.extract_name(&arg_list[i]) {
                let method_name = prefix.as_ref().map_or(name.clone(), |pfx| format!("{}_{}", pfx, name));
                self.found_instance_method(&method_name, line, start, end, None);
            }
        }
    }

    fn find_assoc_value(&self, elements: &[ruby_prism::Node], key_name: &str) -> Option<String> {
        elements.iter().find_map(|elem| {
            if let ruby_prism::Node::AssocNode { .. } = elem {
                let pair = elem.as_assoc_node().unwrap();
                let name = self.extract_name(&pair.key())?;
                (name == key_name).then(|| self.extract_name(&pair.value())).flatten()
            } else {
                None
            }
        })
    }

    fn get_delegate_prefix(&self, elements: &[ruby_prism::Node], to_value: &Option<String>) -> Option<String> {
        for elem in elements {
            if let ruby_prism::Node::AssocNode { .. } = elem {
                let pair = elem.as_assoc_node().unwrap();
                if self.extract_name(&pair.key()).as_deref() == Some("prefix") {
                    let value = pair.value();
                    return match &value {
                        ruby_prism::Node::TrueNode { .. } => to_value.clone().or(Some("__dynamic__".to_string())),
                        ruby_prism::Node::FalseNode { .. } => None,
                        ruby_prism::Node::SymbolNode { .. } | ruby_prism::Node::StringNode { .. } => self.extract_name(&value),
                        _ => Some("__dynamic__".to_string()),
                    };
                }
            }
        }
        None
    }

    fn check_forwardable_delegator(&mut self, node: &ruby_prism::CallNode) {
        if self.inside_if { return; }
        let method_name = node_name!(node).to_string();
        let args = match node.arguments() { Some(args) => args, None => return };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        let line = self.ctx.line_of(node.location().start_offset()) as u32;
        let (start, end) = (node.location().start_offset(), node.location().end_offset());

        match method_name.as_str() {
            "def_delegator" | "def_instance_delegator" => {
                let idx = if arg_list.len() >= 3 { 2 } else if arg_list.len() >= 2 { 1 } else { return };
                if let Some(name) = self.extract_name(&arg_list[idx]) {
                    self.found_instance_method(&name, line, start, end, None);
                }
            }
            "def_delegators" | "def_instance_delegators" => {
                for i in 1..arg_list.len() {
                    if let Some(name) = self.extract_name(&arg_list[i]) {
                        self.found_instance_method(&name, line, start, end, None);
                    }
                }
            }
            _ => {}
        }
    }

    fn is_class_eval_call(&self, node: &ruby_prism::CallNode) -> bool {
        let name = node_name!(node);
        name == "class_eval" || name == "module_eval"
    }

    fn is_class_or_module_new(&self, node: &ruby_prism::CallNode) -> bool {
        if node_name!(node) != "new" { return false; }
        node.receiver().and_then(|r| self.extract_receiver_name(&r))
            .map_or(false, |name| name == "Class" || name == "Module")
    }

    /// Push a named scope, visit children, pop scope.
    fn with_named_scope<F>(&mut self, name: &str, is_singleton: bool, singleton_receiver: Option<String>, visit: F)
    where F: FnOnce(&mut Self)
    {
        self.scope_stack.push(Scope {
            qualified_name: name.to_string(),
            is_singleton,
            singleton_receiver,
        });
        visit(self);
        self.scope_stack.pop();
    }
}

impl Visit<'_> for DuplicateMethodsVisitor<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        ruby_prism::visit_program_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(name) = self.extract_const_name(&node.constant_path()) {
            let qualified = self.qualify_name(&name);
            self.with_named_scope(&qualified, false, None, |s| ruby_prism::visit_class_node(s, node));
        } else {
            ruby_prism::visit_class_node(self, node);
        }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if let Some(name) = self.extract_const_name(&node.constant_path()) {
            let qualified = self.qualify_name(&name);
            self.with_named_scope(&qualified, false, None, |s| ruby_prism::visit_module_node(s, node));
        } else {
            ruby_prism::visit_module_node(self, node);
        }
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let expr = node.expression();
        let receiver_name = match &expr {
            ruby_prism::Node::SelfNode { .. } => None,
            _ => self.extract_receiver_name(&expr),
        };
        let qname = receiver_name.clone().unwrap_or_else(|| self.current_scope_name());
        self.with_named_scope(&qname, true, receiver_name, |s| ruby_prism::visit_singleton_class_node(s, node));
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let const_name = node_name!(node).to_string();
        let value = node.value();
        if let ruby_prism::Node::CallNode { .. } = &value {
            let call = value.as_call_node().unwrap();
            if self.is_class_or_module_new(&call) {
                self.pending_casgn_name = Some(const_name);
                ruby_prism::visit_constant_write_node(self, node);
                self.pending_casgn_name = None;
                return;
            }
        }
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let method_name = node_name!(node).to_string();

        if self.inside_if {
            self.def_ancestor_stack.push(method_name);
            ruby_prism::visit_def_node(self, node);
            self.def_ancestor_stack.pop();
            return;
        }

        let def_keyword_start = node.def_keyword_loc().start_offset();
        let name_end = node.name_loc().end_offset();
        let line = self.ctx.line_of(def_keyword_start) as u32;

        if let Some(receiver) = node.receiver() {
            match &receiver {
                ruby_prism::Node::SelfNode { .. } => {
                    self.found_class_method(&method_name, line, def_keyword_start, name_end);
                }
                ruby_prism::Node::ConstantReadNode { .. } => {
                    let const_name = node_name!(receiver.as_constant_read_node().unwrap()).to_string();
                    self.found_named_receiver_method(&const_name, &method_name, line, def_keyword_start, name_end);
                }
                _ => {}
            }
        } else {
            self.found_instance_method(&method_name, line, def_keyword_start, name_end, None);
        }

        self.def_ancestor_stack.push(method_name);
        ruby_prism::visit_def_node(self, node);
        self.def_ancestor_stack.pop();
    }

    fn visit_alias_method_node(&mut self, node: &ruby_prism::AliasMethodNode) {
        if self.inside_if { return; }
        let (new_name, old_name) = match (self.extract_name(&node.new_name()), self.extract_name(&node.old_name())) {
            (Some(n), Some(o)) => (n, o),
            _ => return,
        };
        if new_name == old_name { return; }
        let line = self.ctx.line_of(node.location().start_offset()) as u32;
        self.found_instance_method(&new_name, line, node.location().start_offset(), node.location().end_offset(), None);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = node_name!(node).to_string();

        if node.receiver().is_none() {
            match method_name.as_str() {
                "alias_method" => {
                    self.check_alias_method_call(node);
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                "attr_reader" | "attr_writer" | "attr_accessor" | "attr" => {
                    if !self.inside_if { self.check_attr(node); }
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                "delegate" => {
                    if !self.inside_if { self.check_delegate(node); }
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                "def_delegator" | "def_instance_delegator" | "def_delegators" | "def_instance_delegators" => {
                    self.check_forwardable_delegator(node);
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                _ => {}
            }
        }

        if self.is_class_eval_call(node) {
            if let Some(receiver) = node.receiver() {
                if let Some(name) = self.extract_receiver_name(&receiver) {
                    let qualified = self.qualify_name(&name);
                    self.with_named_scope(&qualified, false, None, |s| {
                        s.in_scope_creating_call = true;
                        ruby_prism::visit_call_node(s, node);
                    });
                    return;
                }
            } else {
                self.in_scope_creating_call = true;
                ruby_prism::visit_call_node(self, node);
                return;
            }
        }

        if self.is_class_or_module_new(node) {
            if let Some(name) = self.pending_casgn_name.take() {
                let qualified = self.qualify_name(&name);
                self.with_named_scope(&qualified, false, None, |s| {
                    s.in_scope_creating_call = true;
                    ruby_prism::visit_call_node(s, node);
                });
                return;
            }
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if self.in_scope_creating_call {
            self.in_scope_creating_call = false;
            ruby_prism::visit_block_node(self, node);
        } else {
            self.non_scope_block_depth += 1;
            ruby_prism::visit_block_node(self, node);
            self.non_scope_block_depth -= 1;
        }
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let prev = self.inside_if;
        self.inside_if = true;
        ruby_prism::visit_if_node(self, node);
        self.inside_if = prev;
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let prev = self.inside_if;
        self.inside_if = true;
        ruby_prism::visit_unless_node(self, node);
        self.inside_if = prev;
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        let prev = self.current_rescue_ensure_scope.clone();
        self.current_rescue_ensure_scope = Some(format!("rescue_{}", node.location().start_offset()));
        ruby_prism::visit_rescue_node(self, node);
        self.current_rescue_ensure_scope = prev;
    }

    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode) {
        let prev = self.current_rescue_ensure_scope.clone();
        self.current_rescue_ensure_scope = Some(format!("ensure_{}", node.location().start_offset()));
        ruby_prism::visit_ensure_node(self, node);
        self.current_rescue_ensure_scope = prev;
    }
}

// AllCopsActiveSupportExtensionsEnabled is a fallback alias for ActiveSupportExtensionsEnabled.
#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    active_support_extensions_enabled: Option<bool>,
    #[serde(rename = "AllCopsActiveSupportExtensionsEnabled")]
    all_cops_active_support_extensions_enabled: Option<bool>,
}

crate::register_cop!("Lint/DuplicateMethods", |cfg| {
    let c: Cfg = cfg.typed("Lint/DuplicateMethods");
    let active_support = c.active_support_extensions_enabled
        .or(c.all_cops_active_support_extensions_enabled)
        .unwrap_or(false);
    Some(Box::new(DuplicateMethods::with_config(active_support)))
});
