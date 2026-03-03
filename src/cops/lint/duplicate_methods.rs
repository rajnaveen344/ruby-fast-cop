//! Lint/DuplicateMethods - Checks for duplicated instance (or singleton) method definitions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_methods.rb
//!
//! Key behavior from RuboCop: methods defined inside regular blocks (not class_eval,
//! not Class.new/Module.new) are invisible to the cop. This is because RuboCop's
//! `parent_module_name` returns nil when encountering a regular block ancestor, causing
//! `found_instance_method` to skip the method entirely.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;
use std::collections::HashMap;

pub struct DuplicateMethods {
    active_support_extensions: bool,
}

impl DuplicateMethods {
    pub fn new() -> Self {
        Self {
            active_support_extensions: true,
        }
    }

    pub fn with_config(active_support_extensions: bool) -> Self {
        Self {
            active_support_extensions,
        }
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
    fn name(&self) -> &'static str {
        "Lint/DuplicateMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = DuplicateMethodsVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            definitions: HashMap::new(),
            scope_stack: Vec::new(),
            def_ancestor_stack: Vec::new(),
            inside_if: false,
            current_rescue_ensure_scope: None,
            rescue_ensure_seen_keys: HashMap::new(),
            pending_casgn_name: None,
            in_scope_creating_call: false,
            non_scope_block_depth: 0,
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
    /// Flag set in visit_call_node to indicate the block child is a scope-creating block.
    /// This prevents visit_block_node from treating it as a non-scope block.
    in_scope_creating_call: bool,
    /// Depth counter for non-scope blocks. When > 0, all method definitions are invisible
    /// (matching RuboCop behavior where parent_module_name returns nil for regular blocks).
    non_scope_block_depth: usize,
}

impl<'a> DuplicateMethodsVisitor<'a> {
    fn current_scope_name(&self) -> String {
        if self.scope_stack.is_empty() {
            "Object".to_string()
        } else {
            self.scope_stack.last().unwrap().qualified_name.clone()
        }
    }

    fn line_from_offset(&self, offset: usize) -> u32 {
        let mut line = 1u32;
        for (i, ch) in self.ctx.source.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
            }
        }
        line
    }

    /// Returns true if methods should be tracked (we're not inside a non-scope block).
    fn tracking_enabled(&self) -> bool {
        self.non_scope_block_depth == 0
    }

    fn found_method(
        &mut self,
        method_display: &str,
        line: u32,
        start_offset: usize,
        end_offset: usize,
        rescue_ensure_scope: Option<&str>,
    ) {
        if !self.tracking_enabled() {
            return;
        }

        let key = self.build_storage_key(method_display);

        if let Some(existing) = self.definitions.get(&key) {
            if let Some(scope_id) = rescue_ensure_scope {
                let seen = self
                    .rescue_ensure_seen_keys
                    .entry(scope_id.to_string())
                    .or_default();
                if !seen.contains(&key) {
                    seen.push(key.clone());
                    self.definitions.insert(
                        key,
                        MethodDef {
                            line,
                            filename: self.ctx.filename.to_string(),
                        },
                    );
                    return;
                }
            }

            let message = format!(
                "Method `{}` is defined at both {}:{} and {}:{}.",
                method_display,
                existing.filename,
                existing.line,
                self.ctx.filename,
                line
            );
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(),
                &message,
                self.cop.severity(),
                start_offset,
                end_offset,
            ));
        } else {
            self.definitions.insert(
                key,
                MethodDef {
                    line,
                    filename: self.ctx.filename.to_string(),
                },
            );
        }
    }

    fn build_storage_key(&self, method_display: &str) -> String {
        if let Some(ancestor_def) = self.def_ancestor_stack.last() {
            format!("{}.{}", ancestor_def, method_display)
        } else {
            method_display.to_string()
        }
    }

    fn found_instance_method(
        &mut self,
        method_name: &str,
        line: u32,
        start_offset: usize,
        end_offset: usize,
        rescue_ensure_scope: Option<&str>,
    ) {
        if let Some(scope) = self.scope_stack.last() {
            if scope.is_singleton {
                let receiver_name = if let Some(ref recv) = scope.singleton_receiver {
                    recv.clone()
                } else if self.scope_stack.len() >= 2 {
                    self.scope_stack[self.scope_stack.len() - 2]
                        .qualified_name
                        .clone()
                } else {
                    "Object".to_string()
                };
                let method_display = format!("{}.{}", receiver_name, method_name);
                self.found_method(
                    &method_display,
                    line,
                    start_offset,
                    end_offset,
                    rescue_ensure_scope,
                );
                return;
            }
        }

        let scope = self.current_scope_name();
        let method_display = format!("{}#{}", scope, method_name);
        self.found_method(
            &method_display,
            line,
            start_offset,
            end_offset,
            rescue_ensure_scope,
        );
    }

    fn found_class_method(
        &mut self,
        method_name: &str,
        line: u32,
        start_offset: usize,
        end_offset: usize,
    ) {
        let scope = self.current_scope_name();
        let method_display = format!("{}.{}", scope, method_name);
        self.found_method(&method_display, line, start_offset, end_offset, None);
    }

    fn found_named_receiver_method(
        &mut self,
        receiver_name: &str,
        method_name: &str,
        line: u32,
        start_offset: usize,
        end_offset: usize,
    ) {
        let qualified = self.lookup_constant(receiver_name);
        if let Some(qualified) = qualified {
            let method_display = format!("{}.{}", qualified, method_name);
            self.found_method(&method_display, line, start_offset, end_offset, None);
        }
    }

    fn lookup_constant(&self, const_name: &str) -> Option<String> {
        for scope in self.scope_stack.iter().rev() {
            let name = &scope.qualified_name;
            let last_segment = name.rsplit("::").next().unwrap_or(name);
            if last_segment == const_name {
                return Some(name.clone());
            }
        }
        None
    }

    fn extract_name(&self, node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::SymbolNode { .. } => {
                let sym = node.as_symbol_node().unwrap();
                let loc = sym.value_loc()?;
                self.ctx
                    .source
                    .get(loc.start_offset()..loc.end_offset())
                    .map(|s| s.to_string())
            }
            ruby_prism::Node::StringNode { .. } => {
                let str_node = node.as_string_node().unwrap();
                let loc = str_node.content_loc();
                self.ctx
                    .source
                    .get(loc.start_offset()..loc.end_offset())
                    .map(|s| s.to_string())
            }
            _ => None,
        }
    }

    fn extract_const_name(&self, node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = node.as_constant_read_node().unwrap();
                Some(String::from_utf8_lossy(const_node.name().as_slice()).to_string())
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = node.as_constant_path_node().unwrap();
                let name = path_node
                    .name()
                    .map(|n| String::from_utf8_lossy(n.as_slice()).to_string())?;
                if let Some(parent) = path_node.parent() {
                    let parent_name = self.extract_const_name(&parent)?;
                    Some(format!("{}::{}", parent_name, name))
                } else {
                    Some(name)
                }
            }
            _ => None,
        }
    }

    fn qualify_name(&self, name: &str) -> String {
        if self.scope_stack.is_empty() {
            name.to_string()
        } else {
            let parent = &self.scope_stack.last().unwrap().qualified_name;
            format!("{}::{}", parent, name)
        }
    }

    fn extract_receiver_name(&self, node: &ruby_prism::Node) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = node.as_constant_read_node().unwrap();
                Some(String::from_utf8_lossy(const_node.name().as_slice()).to_string())
            }
            ruby_prism::Node::ConstantPathNode { .. } => self.extract_const_name(node),
            _ => {
                let loc = node.location();
                self.ctx
                    .source
                    .get(loc.start_offset()..loc.end_offset())
                    .map(|s| s.to_string())
            }
        }
    }

    fn check_attr(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let args = match node.arguments() {
            Some(args) => args,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        let line = self.line_from_offset(node.location().start_offset());
        let start = node.location().start_offset();
        let end = node.location().end_offset();

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
                        let writer_name = format!("{}=", name);
                        self.found_instance_method(&writer_name, line, start, end, None);
                    }
                }
            }
            "attr_accessor" => {
                for arg in &arg_list {
                    if let Some(name) = self.extract_name(arg) {
                        self.found_instance_method(&name, line, start, end, None);
                        let writer_name = format!("{}=", name);
                        self.found_instance_method(&writer_name, line, start, end, None);
                    }
                }
            }
            "attr" => {
                let writable = arg_list.len() == 2
                    && matches!(&arg_list[1], ruby_prism::Node::TrueNode { .. });
                if let Some(first_arg) = arg_list.first() {
                    if let Some(name) = self.extract_name(first_arg) {
                        self.found_instance_method(&name, line, start, end, None);
                        if writable {
                            let writer_name = format!("{}=", name);
                            self.found_instance_method(&writer_name, line, start, end, None);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_alias_method_call(&mut self, node: &ruby_prism::CallNode) {
        if self.inside_if {
            return;
        }
        let args = match node.arguments() {
            Some(args) => args,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() < 2 {
            return;
        }

        let new_name = match self.extract_name(&arg_list[0]) {
            Some(n) => n,
            None => return,
        };

        let original_name = self.extract_name(&arg_list[1]);
        if let Some(ref orig) = original_name {
            if &new_name == orig {
                return;
            }
        } else {
            return;
        }

        let line = self.line_from_offset(node.location().start_offset());
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        let rescue_scope = self.current_rescue_ensure_scope.clone();
        self.found_instance_method(&new_name, line, start, end, rescue_scope.as_deref());
    }

    fn check_delegate(&mut self, node: &ruby_prism::CallNode) {
        if !self.cop.active_support_extensions || self.inside_if {
            return;
        }

        let args = match node.arguments() {
            Some(args) => args,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return;
        }

        // The last argument should be a keyword hash containing `to:`
        let last_arg = &arg_list[arg_list.len() - 1];
        let kwargs_elements: Vec<ruby_prism::Node> = match last_arg {
            ruby_prism::Node::KeywordHashNode { .. } => {
                let kh = last_arg.as_keyword_hash_node().unwrap();
                kh.elements().iter().collect()
            }
            _ => return,
        };

        // Check for splat keyword args (**options)
        for elem in &kwargs_elements {
            if matches!(elem, ruby_prism::Node::AssocSplatNode { .. }) {
                return;
            }
        }

        // Find `to:` value
        let to_value = self.find_assoc_value(&kwargs_elements, "to");
        if to_value.is_none() {
            return;
        }

        // Get prefix
        let prefix = self.get_delegate_prefix(&kwargs_elements, &to_value);
        if prefix.as_deref() == Some("__dynamic__") {
            return;
        }

        let positional_count = arg_list.len() - 1;
        let line = self.line_from_offset(node.location().start_offset());
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        for i in 0..positional_count {
            if let Some(name) = self.extract_name(&arg_list[i]) {
                let method_name = if let Some(ref pfx) = prefix {
                    format!("{}_{}", pfx, name)
                } else {
                    name
                };
                self.found_instance_method(&method_name, line, start, end, None);
            }
        }
    }

    fn find_assoc_value(&self, elements: &[ruby_prism::Node], key_name: &str) -> Option<String> {
        for elem in elements {
            if let ruby_prism::Node::AssocNode { .. } = elem {
                let pair = elem.as_assoc_node().unwrap();
                let key = pair.key();
                if let Some(name) = self.extract_name(&key) {
                    if name == key_name {
                        return self.extract_name(&pair.value());
                    }
                }
            }
        }
        None
    }

    fn get_delegate_prefix(
        &self,
        elements: &[ruby_prism::Node],
        to_value: &Option<String>,
    ) -> Option<String> {
        for elem in elements {
            if let ruby_prism::Node::AssocNode { .. } = elem {
                let pair = elem.as_assoc_node().unwrap();
                let key = pair.key();
                if let Some(name) = self.extract_name(&key) {
                    if name == "prefix" {
                        let value = pair.value();
                        return match &value {
                            ruby_prism::Node::TrueNode { .. } => {
                                if to_value.is_some() {
                                    to_value.clone()
                                } else {
                                    Some("__dynamic__".to_string())
                                }
                            }
                            ruby_prism::Node::FalseNode { .. } => None,
                            ruby_prism::Node::SymbolNode { .. }
                            | ruby_prism::Node::StringNode { .. } => self.extract_name(&value),
                            _ => Some("__dynamic__".to_string()),
                        };
                    }
                }
            }
        }
        None
    }

    fn check_forwardable_delegator(&mut self, node: &ruby_prism::CallNode) {
        if self.inside_if {
            return;
        }

        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let args = match node.arguments() {
            Some(args) => args,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        let line = self.line_from_offset(node.location().start_offset());
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        match method_name.as_str() {
            "def_delegator" | "def_instance_delegator" => {
                if arg_list.len() >= 3 {
                    if let Some(name) = self.extract_name(&arg_list[2]) {
                        self.found_instance_method(&name, line, start, end, None);
                    }
                } else if arg_list.len() >= 2 {
                    if let Some(name) = self.extract_name(&arg_list[1]) {
                        self.found_instance_method(&name, line, start, end, None);
                    }
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
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        method_name == "class_eval" || method_name == "module_eval"
    }

    fn is_class_or_module_new(&self, node: &ruby_prism::CallNode) -> bool {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if method_name != "new" {
            return false;
        }
        if let Some(recv) = node.receiver() {
            if let Some(name) = self.extract_receiver_name(&recv) {
                return name == "Class" || name == "Module";
            }
        }
        false
    }
}

impl Visit<'_> for DuplicateMethodsVisitor<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        ruby_prism::visit_program_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let const_name = self.extract_const_name(&node.constant_path());
        if let Some(name) = const_name {
            let qualified = self.qualify_name(&name);
            self.scope_stack.push(Scope {
                qualified_name: qualified,
                is_singleton: false,
                singleton_receiver: None,
            });
            ruby_prism::visit_class_node(self, node);
            self.scope_stack.pop();
        } else {
            ruby_prism::visit_class_node(self, node);
        }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let const_name = self.extract_const_name(&node.constant_path());
        if let Some(name) = const_name {
            let qualified = self.qualify_name(&name);
            self.scope_stack.push(Scope {
                qualified_name: qualified,
                is_singleton: false,
                singleton_receiver: None,
            });
            ruby_prism::visit_module_node(self, node);
            self.scope_stack.pop();
        } else {
            ruby_prism::visit_module_node(self, node);
        }
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let expr = node.expression();
        let receiver_name = match &expr {
            ruby_prism::Node::SelfNode { .. } => None,
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = expr.as_constant_read_node().unwrap();
                Some(String::from_utf8_lossy(const_node.name().as_slice()).to_string())
            }
            _ => {
                let loc = expr.location();
                self.ctx
                    .source
                    .get(loc.start_offset()..loc.end_offset())
                    .map(|s| s.to_string())
            }
        };

        self.scope_stack.push(Scope {
            qualified_name: if let Some(ref recv) = receiver_name {
                recv.clone()
            } else {
                self.current_scope_name()
            },
            is_singleton: true,
            singleton_receiver: receiver_name,
        });
        ruby_prism::visit_singleton_class_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let const_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
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
        if self.inside_if {
            let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
            self.def_ancestor_stack.push(method_name);
            ruby_prism::visit_def_node(self, node);
            self.def_ancestor_stack.pop();
            return;
        }

        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let def_keyword_start = node.def_keyword_loc().start_offset();
        let name_end = node.name_loc().end_offset();
        let line = self.line_from_offset(def_keyword_start);

        if let Some(receiver) = node.receiver() {
            match &receiver {
                ruby_prism::Node::SelfNode { .. } => {
                    self.found_class_method(&method_name, line, def_keyword_start, name_end);
                }
                ruby_prism::Node::ConstantReadNode { .. } => {
                    let const_node = receiver.as_constant_read_node().unwrap();
                    let const_name =
                        String::from_utf8_lossy(const_node.name().as_slice()).to_string();
                    self.found_named_receiver_method(
                        &const_name,
                        &method_name,
                        line,
                        def_keyword_start,
                        name_end,
                    );
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
        if self.inside_if {
            return;
        }

        let new_name_node = node.new_name();
        let old_name_node = node.old_name();

        let new_name = self.extract_name(&new_name_node);
        let old_name = self.extract_name(&old_name_node);

        if let (Some(new_name), Some(old_name)) = (new_name, old_name) {
            if new_name == old_name {
                return;
            }
            let line = self.line_from_offset(node.location().start_offset());
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            self.found_instance_method(&new_name, line, start, end, None);
        }
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        if node.receiver().is_none() {
            match method_name.as_str() {
                "alias_method" => {
                    self.check_alias_method_call(node);
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                "attr_reader" | "attr_writer" | "attr_accessor" | "attr" => {
                    if !self.inside_if {
                        self.check_attr(node);
                    }
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                "delegate" => {
                    if !self.inside_if {
                        self.check_delegate(node);
                    }
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                "def_delegator" | "def_instance_delegator" | "def_delegators"
                | "def_instance_delegators" => {
                    self.check_forwardable_delegator(node);
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
                _ => {}
            }
        }

        // Handle X.class_eval do ... end (or class_eval without receiver)
        if self.is_class_eval_call(node) {
            if let Some(receiver) = node.receiver() {
                let recv_name = self.extract_receiver_name(&receiver);
                if let Some(name) = recv_name {
                    let qualified = self.qualify_name(&name);
                    self.scope_stack.push(Scope {
                        qualified_name: qualified,
                        is_singleton: false,
                        singleton_receiver: None,
                    });
                    self.in_scope_creating_call = true;
                    ruby_prism::visit_call_node(self, node);
                    self.scope_stack.pop();
                    return;
                }
            } else {
                // class_eval/module_eval without a receiver (implicit self) -
                // uses the current scope. Don't push a new scope, just mark as scope-creating.
                self.in_scope_creating_call = true;
                ruby_prism::visit_call_node(self, node);
                return;
            }
        }

        // Handle Class.new/Module.new with a pending casgn
        if self.is_class_or_module_new(node) {
            if let Some(name) = self.pending_casgn_name.take() {
                let qualified = self.qualify_name(&name);
                self.scope_stack.push(Scope {
                    qualified_name: qualified,
                    is_singleton: false,
                    singleton_receiver: None,
                });
                self.in_scope_creating_call = true;
                ruby_prism::visit_call_node(self, node);
                self.scope_stack.pop();
                return;
            }
            // Class.new/Module.new WITHOUT constant assignment -> treated as regular block
            // (don't set in_scope_creating_call, so visit_block_node will make it non-scope)
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if self.in_scope_creating_call {
            // This block is for a scope-creating call (class_eval, Class.new with casgn).
            // The scope was already pushed in visit_call_node. Just reset the flag and continue.
            self.in_scope_creating_call = false;
            ruby_prism::visit_block_node(self, node);
        } else {
            // Regular block (dsl_like, describe, etc.) - methods inside are invisible.
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
        self.current_rescue_ensure_scope =
            Some(format!("rescue_{}", node.location().start_offset()));
        ruby_prism::visit_rescue_node(self, node);
        self.current_rescue_ensure_scope = prev;
    }

    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode) {
        let prev = self.current_rescue_ensure_scope.clone();
        self.current_rescue_ensure_scope =
            Some(format!("ensure_{}", node.location().start_offset()));
        ruby_prism::visit_ensure_node(self, node);
        self.current_rescue_ensure_scope = prev;
    }
}
