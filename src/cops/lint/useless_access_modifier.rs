use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

/// Access modifier names
const ACCESS_MODIFIERS: &[&str] = &["private", "protected", "public"];

/// Methods that create new methods (always recognized)
const BUILTIN_ATTR_METHODS: &[&str] = &[
    "attr", "attr_reader", "attr_writer", "attr_accessor",
];

/// Scope-creating block methods
const SCOPE_CREATING_EVAL: &[&str] = &["class_eval", "module_eval", "instance_eval"];
const SCOPE_CREATING_NEW: &[&str] = &["Class", "Module", "Struct"];

pub struct UselessAccessModifier {
    context_creating_methods: Vec<String>,
    method_creating_methods: Vec<String>,
}

impl UselessAccessModifier {
    pub fn new() -> Self {
        Self {
            context_creating_methods: Vec::new(),
            method_creating_methods: Vec::new(),
        }
    }

    pub fn with_config(
        context_creating_methods: Vec<String>,
        method_creating_methods: Vec<String>,
    ) -> Self {
        Self {
            context_creating_methods,
            method_creating_methods,
        }
    }
}

impl Cop for UselessAccessModifier {
    fn name(&self) -> &'static str {
        "Lint/UselessAccessModifier"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut checker = Checker {
            ctx,
            context_creating_methods: &self.context_creating_methods,
            method_creating_methods: &self.method_creating_methods,
            offenses: Vec::new(),
        };

        let stmts: Vec<Node> = node.statements().body().iter().collect();

        // Top-level: flag bare access modifiers as useless, recurse into scopes
        checker.check_toplevel(&stmts);

        // Also recurse into all top-level class/module/sclass/eval_call/included blocks
        checker.recurse_into_scopes(&stmts);

        checker.offenses
    }
}

struct Checker<'a> {
    ctx: &'a CheckContext<'a>,
    context_creating_methods: &'a [String],
    method_creating_methods: &'a [String],
    offenses: Vec<Offense>,
}

impl<'a> Checker<'a> {
    /// Top-level: flag bare access modifiers and recurse into transparent nodes.
    /// RuboCop's on_begin: only checks direct children that are bare access modifiers.
    fn check_toplevel(&mut self, stmts: &[Node]) {
        for stmt in stmts {
            if let Some((name, msg_start, msg_end)) = extract_bare_access_modifier(stmt) {
                // At top level, access modifiers are always useless.
                // RuboCop calls check_send_node(child, child.method_name, true)
                // which results in new_vis == cur_vis -> immediate offense.
                let loc = stmt.location();
                self.add_offense_raw(&name, msg_start, msg_end, loc.start_offset(), loc.end_offset());
            }
        }
    }

    /// Recursively find and check all scopes in the given statements.
    fn recurse_into_scopes(&mut self, stmts: &[Node]) {
        for stmt in stmts {
            self.find_and_check_scopes(stmt);
        }
    }

    /// Walk the tree to find class/module/sclass/eval_call nodes and check them.
    fn find_and_check_scopes(&mut self, node: &Node) {
        match node {
            Node::ClassNode { .. } => {
                // check_node(node.body) -> check_scope if begin_type
                self.check_node_body(node);
            }
            Node::ModuleNode { .. } => {
                self.check_node_body(node);
            }
            Node::SingletonClassNode { .. } => {
                self.check_node_body(node);
            }
            Node::CallNode { .. } => {
                if self.is_eval_call(node) || self.is_included_block(node) {
                    // on_block: eval_call or included_block -> check_node(block.body)
                    let call = node.as_call_node().unwrap();
                    if let Some(block) = call.block() {
                        if let Node::BlockNode { .. } = &block {
                            let blk = block.as_block_node().unwrap();
                            if let Some(body) = blk.body() {
                                self.check_node(&body);
                            }
                        }
                    }
                } else {
                    // Recurse into non-scope-creating calls to find nested scopes
                    let call = node.as_call_node().unwrap();
                    if let Some(block) = call.block() {
                        self.find_and_check_scopes(&block);
                    }
                    if let Some(args) = call.arguments() {
                        for arg in args.arguments().iter() {
                            self.find_and_check_scopes(&arg);
                        }
                    }
                }
            }
            Node::BlockNode { .. } => {
                let blk = node.as_block_node().unwrap();
                if let Some(body) = blk.body() {
                    self.find_and_check_scopes(&body);
                }
            }
            // Recurse through transparent nodes
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for child in stmts.body().iter() {
                    self.find_and_check_scopes(&child);
                }
            }
            Node::BeginNode { .. } => {
                if let Some(stmts) = node.as_begin_node().unwrap().statements() {
                    for child in stmts.body().iter() {
                        self.find_and_check_scopes(&child);
                    }
                }
            }
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                if let Some(stmts) = n.statements() {
                    for child in stmts.body().iter() {
                        self.find_and_check_scopes(&child);
                    }
                }
                if let Some(sub) = n.subsequent() {
                    self.find_and_check_scopes(&sub);
                }
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                if let Some(stmts) = n.statements() {
                    for child in stmts.body().iter() {
                        self.find_and_check_scopes(&child);
                    }
                }
                if let Some(else_clause) = n.else_clause() {
                    if let Some(stmts) = else_clause.statements() {
                        for child in stmts.body().iter() {
                            self.find_and_check_scopes(&child);
                        }
                    }
                }
            }
            Node::ElseNode { .. } => {
                if let Some(stmts) = node.as_else_node().unwrap().statements() {
                    for child in stmts.body().iter() {
                        self.find_and_check_scopes(&child);
                    }
                }
            }
            Node::DefNode { .. } => {
                // Don't recurse into def bodies for scope finding at top level
            }
            _ => {}
        }
    }

    /// RuboCop's check_node: if body is begin_type -> check_scope,
    /// elif single bare modifier -> flag it.
    fn check_node(&mut self, body: &Node) {
        match body {
            Node::BeginNode { .. } | Node::StatementsNode { .. } => {
                self.check_scope(body);
            }
            _ => {
                // Single statement body - if it's a bare access modifier, flag it
                if let Some((name, msg_start, msg_end)) = extract_bare_access_modifier(body) {
                    let loc = body.location();
                    self.add_offense_raw(&name, msg_start, msg_end, loc.start_offset(), loc.end_offset());
                }
            }
        }
    }

    /// Match RuboCop's check_node for class/module/sclass bodies
    fn check_node_body(&mut self, node: &Node) {
        let body = match node {
            Node::ClassNode { .. } => node.as_class_node().unwrap().body(),
            Node::ModuleNode { .. } => node.as_module_node().unwrap().body(),
            Node::SingletonClassNode { .. } => node.as_singleton_class_node().unwrap().body(),
            _ => None,
        };
        if let Some(body) = body {
            self.check_node(&body);
        }
    }

    /// RuboCop's check_scope: check_child_nodes then flag leftover unused modifier.
    fn check_scope(&mut self, body: &Node) {
        let (_cur_vis, unused) = self.check_child_nodes(body, None, "public");
        if let Some((name, msg_start, msg_end, node_start, node_end)) = unused {
            self.add_offense_raw(&name, msg_start, msg_end, node_start, node_end);
        }
    }

    /// RuboCop's check_child_nodes: recursively process children,
    /// threading cur_vis and unused modifier state.
    fn check_child_nodes(
        &mut self,
        node: &Node,
        unused: Option<(String, usize, usize, usize, usize)>,
        cur_vis: &str,
    ) -> (String, Option<(String, usize, usize, usize, usize)>) {
        let mut cur_vis = cur_vis.to_string();
        let mut unused = unused;

        let children = get_child_statements(node);

        for child in &children {
            if let Some((name, msg_start, msg_end)) = extract_bare_access_modifier(child) {
                // Access modifier: check if redundant or flag previous as unused
                let result = self.check_send_node(child, &name, msg_start, msg_end, &cur_vis, unused);
                cur_vis = result.0;
                unused = result.1;
            } else if is_private_class_method_no_args(child) {
                // private_class_method without args - always useless
                let call = child.as_call_node().unwrap();
                if let Some(msg_loc) = call.message_loc() {
                    let loc = child.location();
                    self.add_offense_raw(
                        "private_class_method",
                        msg_loc.start_offset(),
                        msg_loc.end_offset(),
                        loc.start_offset(),
                        loc.end_offset(),
                    );
                }
                // Don't change cur_vis or unused
            } else if self.is_included_block(child) {
                // Skip included blocks (ActiveSupport extensions enabled)
                continue;
            } else if self.is_method_definition(child) {
                // Method definition makes the pending modifier useful
                unused = None;
            } else if self.is_start_of_new_scope(child) {
                // New scope - check it independently, don't affect parent state
                self.check_new_scope(child);
            } else if !is_defs_type(child) {
                // Recurse into transparent nodes (begin, blocks, if/unless, etc.)
                let result = self.check_child_nodes(child, unused, &cur_vis);
                cur_vis = result.0;
                unused = result.1;
            }
            // defs_type (def self.foo) is ignored - doesn't affect visibility state
        }

        (cur_vis, unused)
    }

    /// RuboCop's check_send_node + check_new_visibility
    fn check_send_node(
        &mut self,
        node: &Node,
        new_vis: &str,
        msg_start: usize,
        msg_end: usize,
        cur_vis: &str,
        unused: Option<(String, usize, usize, usize, usize)>,
    ) -> (String, Option<(String, usize, usize, usize, usize)>) {
        let loc = node.location();
        let node_start = loc.start_offset();
        let node_end = loc.end_offset();

        if new_vis == cur_vis {
            // Same visibility as current - immediately flag as useless
            self.add_offense_raw(new_vis, msg_start, msg_end, node_start, node_end);
            (cur_vis.to_string(), unused)
        } else {
            // Different visibility - flag previous unused modifier if any
            if let Some((ref uname, us, ue, uns, une)) = unused {
                self.add_offense_raw(uname, us, ue, uns, une);
            }
            // This modifier becomes the new pending unused one
            let new_unused = Some((
                new_vis.to_string(),
                msg_start,
                msg_end,
                node_start,
                node_end,
            ));
            (new_vis.to_string(), new_unused)
        }
    }

    /// Check if a node is a method definition that makes access modifiers useful.
    fn is_method_definition(&self, node: &Node) -> bool {
        match node {
            Node::DefNode { .. } => {
                // Only instance methods (no receiver). `def self.foo` is defs_type.
                let def = node.as_def_node().unwrap();
                def.receiver().is_none()
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if call.receiver().is_some() {
                    return false;
                }
                let method_name = call_method_name(&call);

                // static: attr, attr_reader, attr_writer, attr_accessor
                if BUILTIN_ATTR_METHODS.contains(&method_name.as_str()) {
                    return true;
                }

                // dynamic: define_method
                if method_name == "define_method" {
                    return true;
                }

                // MethodCreatingMethods config (excluding 'included')
                if method_name != "included"
                    && self.method_creating_methods.iter().any(|m| m == &method_name)
                {
                    // RuboCop pattern: {def (send nil? :method_name ...)}
                    // This means either a def node OR a call to method_name
                    // For a call node, just having the name match is enough
                    return true;
                }

                // Also check: call with a DefNode argument (e.g., `helper_method def some_method`)
                if let Some(args) = call.arguments() {
                    for arg in args.arguments().iter() {
                        if let Node::DefNode { .. } = &arg {
                            let def = arg.as_def_node().unwrap();
                            if def.receiver().is_none() {
                                return true;
                            }
                        }
                    }
                }

                false
            }
            _ => false,
        }
    }

    /// Check if a node starts a new scope.
    fn is_start_of_new_scope(&self, node: &Node) -> bool {
        match node {
            Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if call.block().is_none() {
                    return false;
                }
                self.is_eval_call(node)
            }
            _ => false,
        }
    }

    /// Check if a call node is an `included` block (ActiveSupport).
    /// In RuboCop, this returns true only when active_support_extensions_enabled?
    /// We treat `included` as scope-creating when it appears in ContextCreatingMethods config.
    fn is_included_block(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if call.block().is_none() {
                return false;
            }
            if call.receiver().is_some() {
                return false;
            }
            let method_name = call_method_name(&call);
            // 'included' is special: only treated as scope-creating when explicitly
            // in ContextCreatingMethods (simulating active_support_extensions_enabled?)
            if method_name == "included" {
                return self.context_creating_methods.iter().any(|m| m == "included");
            }
        }
        false
    }

    /// Check if a call is an eval/constructor/context-creating call.
    fn is_eval_call(&self, node: &Node) -> bool {
        if let Node::CallNode { .. } = node {
            let call = node.as_call_node().unwrap();
            if call.block().is_none() {
                return false;
            }
            let method_name = call_method_name(&call);

            // class_eval, module_eval, instance_eval (with any receiver)
            if SCOPE_CREATING_EVAL.contains(&method_name.as_str()) {
                return true;
            }

            // Class.new, Module.new, Struct.new
            if let Some(recv) = call.receiver() {
                if method_name == "new" && is_scope_creating_new_receiver(&recv) {
                    return true;
                }
                // Data.define
                if method_name == "define" && is_data_receiver(&recv) {
                    return true;
                }
            }

            // ContextCreatingMethods (excluding 'included')
            if call.receiver().is_none()
                && method_name != "included"
                && self.context_creating_methods.iter().any(|m| m == &method_name)
            {
                return true;
            }
        }
        false
    }

    /// Check a new scope node (class/module/sclass/eval block).
    fn check_new_scope(&mut self, node: &Node) {
        match node {
            Node::ClassNode { .. } => {
                self.check_node_body(node);
            }
            Node::ModuleNode { .. } => {
                self.check_node_body(node);
            }
            Node::SingletonClassNode { .. } => {
                self.check_node_body(node);
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if let Some(block) = call.block() {
                    if let Node::BlockNode { .. } = &block {
                        let blk = block.as_block_node().unwrap();
                        if let Some(body) = blk.body() {
                            self.check_node(&body);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn add_offense_raw(
        &mut self,
        name: &str,
        msg_start: usize,
        msg_end: usize,
        node_start: usize,
        node_end: usize,
    ) {
        let message = format!("Useless `{}` access modifier.", name);
        let offense = self.ctx.offense_with_range(
            "Lint/UselessAccessModifier",
            &message,
            Severity::Warning,
            msg_start,
            msg_end,
        );
        let correction = make_correction(self.ctx.source, node_start, node_end);
        self.offenses.push(offense.with_correction(correction));
    }
}

/// Extract bare access modifier (private/protected/public without args).
fn extract_bare_access_modifier(node: &Node) -> Option<(String, usize, usize)> {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() {
            return None;
        }
        let method_name = call_method_name(&call);
        if !ACCESS_MODIFIERS.contains(&method_name.as_str()) {
            return None;
        }
        let has_args = call.arguments().map_or(false, |args| {
            args.arguments().iter().next().is_some()
        });
        if has_args || call.block().is_some() {
            return None;
        }
        let msg_loc = call.message_loc()?;
        Some((method_name, msg_loc.start_offset(), msg_loc.end_offset()))
    } else {
        None
    }
}

/// Check if a node is `def self.foo` or `def SomeClass.foo`.
fn is_defs_type(node: &Node) -> bool {
    if let Node::DefNode { .. } = node {
        let def = node.as_def_node().unwrap();
        def.receiver().is_some()
    } else {
        false
    }
}

/// Check if `private_class_method` without arguments.
fn is_private_class_method_no_args(node: &Node) -> bool {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() { return false; }
        let method_name = call_method_name(&call);
        if method_name != "private_class_method" { return false; }
        let has_args = call.arguments().map_or(false, |a| a.arguments().iter().next().is_some());
        !has_args && call.block().is_none()
    } else {
        false
    }
}

/// Get child statements from a node for check_child_nodes traversal.
fn get_child_statements<'b>(node: &'b Node<'b>) -> Vec<Node<'b>> {
    match node {
        Node::StatementsNode { .. } => {
            node.as_statements_node().unwrap().body().iter().collect()
        }
        Node::BeginNode { .. } => {
            if let Some(stmts) = node.as_begin_node().unwrap().statements() {
                stmts.body().iter().collect()
            } else {
                vec![]
            }
        }
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            let mut children = Vec::new();
            if let Some(stmts) = n.statements() {
                children.extend(stmts.body().iter());
            }
            if let Some(sub) = n.subsequent() {
                children.push(sub);
            }
            children
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            let mut children = Vec::new();
            if let Some(stmts) = n.statements() {
                children.extend(stmts.body().iter());
            }
            if let Some(else_clause) = n.else_clause() {
                if let Some(else_stmts) = else_clause.statements() {
                    children.extend(else_stmts.body().iter());
                }
            }
            children
        }
        Node::ElseNode { .. } => {
            let n = node.as_else_node().unwrap();
            if let Some(stmts) = n.statements() {
                stmts.body().iter().collect()
            } else {
                vec![]
            }
        }
        Node::BlockNode { .. } => {
            let n = node.as_block_node().unwrap();
            if let Some(body) = n.body() {
                vec![body]
            } else {
                vec![]
            }
        }
        Node::CallNode { .. } => {
            // For a call node that we're recursing into (transparent), check its block
            let call = node.as_call_node().unwrap();
            if let Some(block) = call.block() {
                if let Node::BlockNode { .. } = &block {
                    let blk = block.as_block_node().unwrap();
                    if let Some(body) = blk.body() {
                        return vec![body];
                    }
                }
            }
            vec![]
        }
        _ => vec![],
    }
}

fn call_method_name(call: &ruby_prism::CallNode) -> String {
    String::from_utf8_lossy(call.name().as_slice()).to_string()
}

fn is_scope_creating_new_receiver(recv: &Node) -> bool {
    match recv {
        Node::ConstantReadNode { .. } => {
            let name = String::from_utf8_lossy(
                recv.as_constant_read_node().unwrap().name().as_slice(),
            );
            SCOPE_CREATING_NEW.contains(&name.as_ref())
        }
        Node::ConstantPathNode { .. } => {
            let path = recv.as_constant_path_node().unwrap();
            if path.parent().is_none() {
                if let Some(child_name) = path.name() {
                    let name = String::from_utf8_lossy(child_name.as_slice());
                    return SCOPE_CREATING_NEW.contains(&name.as_ref());
                }
            }
            false
        }
        _ => false,
    }
}

fn is_data_receiver(recv: &Node) -> bool {
    match recv {
        Node::ConstantReadNode { .. } => {
            let name = String::from_utf8_lossy(
                recv.as_constant_read_node().unwrap().name().as_slice(),
            );
            name.as_ref() == "Data"
        }
        Node::ConstantPathNode { .. } => {
            let path = recv.as_constant_path_node().unwrap();
            if path.parent().is_none() {
                if let Some(child_name) = path.name() {
                    let name = String::from_utf8_lossy(child_name.as_slice());
                    return name.as_ref() == "Data";
                }
            }
            false
        }
        _ => false,
    }
}

fn make_correction(source: &str, node_start: usize, node_end: usize) -> Correction {
    let bytes = source.as_bytes();

    // Remove from start of line (whitespace before) through trailing newline
    let mut remove_start = node_start;
    while remove_start > 0 && matches!(bytes[remove_start - 1], b' ' | b'\t') {
        remove_start -= 1;
    }

    let mut remove_end = node_end;
    while remove_end < bytes.len() && matches!(bytes[remove_end], b' ' | b'\t') {
        remove_end += 1;
    }
    if remove_end < bytes.len() && bytes[remove_end] == b'\r' {
        remove_end += 1;
    }
    if remove_end < bytes.len() && bytes[remove_end] == b'\n' {
        remove_end += 1;
    }

    Correction::delete(remove_start, remove_end)
}
