//! Naming/MethodName - Checks that method names match the configured style (snake_case or camelCase).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/naming/method_name.rb
//!
//! Checks:
//! - `def` / `def self.` method definitions
//! - `attr`, `attr_reader`, `attr_writer`, `attr_accessor` arguments
//! - `alias` statements (first argument)
//! - `alias_method` calls (first argument)
//! - `define_method` / `define_singleton_method` calls
//! - `Struct.new` / `Data.define` symbol/string member arguments
//!
//! Config options:
//! - EnforcedStyle: "snake_case" (default) or "camelCase"
//! - AllowedPatterns: regexes that exempt matching method names
//! - ForbiddenIdentifiers: exact names that are always forbidden
//! - ForbiddenPatterns: regexes that flag matching method names as forbidden

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use ruby_prism::{Node, Visit};

const OPERATOR_METHODS: &[&str] = &[
    "==", "===", "!=", "<=>", "<", ">", "<=", ">=", "=~", "!~", "&", "|", "^", "~", "<<", ">>",
    "+", "-", "*", "/", "%", "**", "+@", "-@", "~@", "!@", "[]", "[]=", "`", "!",
];

const ATTR_METHODS: &[&str] = &["attr", "attr_reader", "attr_writer", "attr_accessor"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MethodNameStyle {
    SnakeCase,
    CamelCase,
}

pub struct MethodName {
    enforced_style: MethodNameStyle,
    allowed_patterns: Vec<String>,
    forbidden_identifiers: Vec<String>,
    forbidden_patterns: Vec<String>,
}

impl MethodName {
    pub fn new() -> Self {
        Self {
            enforced_style: MethodNameStyle::SnakeCase,
            allowed_patterns: vec![],
            forbidden_identifiers: vec!["__id__".to_string(), "__send__".to_string()],
            forbidden_patterns: vec![],
        }
    }

    pub fn with_config(
        enforced_style: MethodNameStyle,
        allowed_patterns: Vec<String>,
        forbidden_identifiers: Vec<String>,
        forbidden_patterns: Vec<String>,
    ) -> Self {
        Self {
            enforced_style,
            allowed_patterns,
            forbidden_identifiers,
            forbidden_patterns,
        }
    }
}

impl Default for MethodName {
    fn default() -> Self {
        Self::new()
    }
}

fn is_operator(name: &str) -> bool {
    OPERATOR_METHODS.contains(&name)
}

fn strip_suffix(name: &str) -> &str {
    name.trim_end_matches(|c| c == '?' || c == '!' || c == '=')
}

fn is_snake_case(name: &str) -> bool {
    let base = strip_suffix(name);
    if base.is_empty() {
        return true;
    }
    // snake_case means no uppercase ASCII letters
    !base.bytes().any(|b| b.is_ascii_uppercase())
}

fn is_camel_case(name: &str) -> bool {
    let base = strip_suffix(name);
    if base.is_empty() {
        return true;
    }
    // camelCase: starts with lowercase, no underscores
    let first = base.as_bytes()[0];
    if first.is_ascii_uppercase() {
        return false;
    }
    !base.contains('_')
}

fn matches_style(name: &str, style: MethodNameStyle) -> bool {
    match style {
        MethodNameStyle::SnakeCase => is_snake_case(name),
        MethodNameStyle::CamelCase => is_camel_case(name),
    }
}

fn style_message(style: MethodNameStyle) -> &'static str {
    match style {
        MethodNameStyle::SnakeCase => "Use snake_case for method names.",
        MethodNameStyle::CamelCase => "Use camelCase for method names.",
    }
}

fn matches_any_pattern(name: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            if re.is_match(name) {
                return true;
            }
        }
    }
    false
}

fn extract_name_from_node(node: &Node) -> Option<String> {
    match node {
        Node::SymbolNode { .. } => {
            let sym = node.as_symbol_node().unwrap();
            let name = String::from_utf8_lossy(sym.unescaped().as_ref()).to_string();
            Some(name)
        }
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            let name = String::from_utf8_lossy(s.unescaped().as_ref()).to_string();
            Some(name)
        }
        Node::InterpolatedSymbolNode { .. } | Node::InterpolatedStringNode { .. } => None,
        _ => None,
    }
}

struct MethodNameVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a MethodName,
    offenses: Vec<Offense>,
    class_names: Vec<String>,
}

impl<'a> MethodNameVisitor<'a> {
    fn check_name(&mut self, name: &str, start_offset: usize, end_offset: usize) {
        if is_operator(name) {
            return;
        }

        // Check forbidden identifiers first (takes priority)
        if self.cop.forbidden_identifiers.contains(&name.to_string()) {
            self.offenses.push(self.ctx.offense_with_range(
                "Naming/MethodName",
                &format!(
                    "`{}` is forbidden, use another method name instead.",
                    name
                ),
                Severity::Convention,
                start_offset,
                end_offset,
            ));
            return;
        }

        // Check forbidden patterns
        if matches_any_pattern(name, &self.cop.forbidden_patterns) {
            self.offenses.push(self.ctx.offense_with_range(
                "Naming/MethodName",
                &format!(
                    "`{}` is forbidden, use another method name instead.",
                    name
                ),
                Severity::Convention,
                start_offset,
                end_offset,
            ));
            return;
        }

        // Check allowed patterns
        if matches_any_pattern(name, &self.cop.allowed_patterns) {
            return;
        }

        // Check style
        if !matches_style(name, self.cop.enforced_style) {
            self.offenses.push(self.ctx.offense_with_range(
                "Naming/MethodName",
                style_message(self.cop.enforced_style),
                Severity::Convention,
                start_offset,
                end_offset,
            ));
        }
    }

    fn is_class_emitter(&self, method_name: &str) -> bool {
        // A "class emitter" method has a name that matches a class in scope
        // Method names are capitalized when they're class emitters
        if method_name.is_empty() || !method_name.as_bytes()[0].is_ascii_uppercase() {
            return false;
        }
        self.class_names.contains(&method_name.to_string())
    }

    fn check_def_node(&mut self, node: &ruby_prism::DefNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        if is_operator(&name) {
            return;
        }

        // For class emitter methods: singleton method whose name matches a class in scope
        if node.receiver().is_some() && self.is_class_emitter(&name) {
            return;
        }

        // Get method name location
        let name_loc = node.name_loc();
        self.check_name(&name, name_loc.start_offset(), name_loc.end_offset());
    }

    fn check_attr_call(&mut self, node: &ruby_prism::CallNode) {
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };

        let arguments: Vec<_> = args.arguments().iter().collect();

        // Check if any argument violates the naming convention
        // RuboCop reports a single offense spanning all arguments if any violate
        let mut has_violation = false;
        let mut forbidden_name: Option<String> = None;

        for arg in &arguments {
            if let Some(name) = extract_name_from_node(arg) {
                if is_operator(&name) {
                    continue;
                }

                // Check forbidden identifiers
                if self.cop.forbidden_identifiers.contains(&name) {
                    forbidden_name = Some(name.clone());
                    has_violation = true;
                    break;
                }

                // Check forbidden patterns
                if matches_any_pattern(&name, &self.cop.forbidden_patterns) {
                    forbidden_name = Some(name.clone());
                    has_violation = true;
                    break;
                }

                // Check allowed patterns
                if matches_any_pattern(&name, &self.cop.allowed_patterns) {
                    continue;
                }

                if !matches_style(&name, self.cop.enforced_style) {
                    has_violation = true;
                }
            }
        }

        if !has_violation {
            return;
        }

        // For forbidden names, report each individually
        if let Some(ref fname) = forbidden_name {
            // Report each forbidden name argument individually
            for arg in &arguments {
                if let Some(name) = extract_name_from_node(arg) {
                    if self.cop.forbidden_identifiers.contains(&name)
                        || matches_any_pattern(&name, &self.cop.forbidden_patterns)
                    {
                        let loc = arg.location();
                        self.offenses.push(self.ctx.offense_with_range(
                            "Naming/MethodName",
                            &format!(
                                "`{}` is forbidden, use another method name instead.",
                                name
                            ),
                            Severity::Convention,
                            loc.start_offset(),
                            loc.end_offset(),
                        ));
                    }
                }
            }
            let _ = fname;
            return;
        }

        // For style violations, report a single offense spanning all arguments
        let first_arg = &arguments[0];
        let last_arg = &arguments[arguments.len() - 1];
        let start = first_arg.location().start_offset();
        let end = last_arg.location().end_offset();

        self.offenses.push(self.ctx.offense_with_range(
            "Naming/MethodName",
            style_message(self.cop.enforced_style),
            Severity::Convention,
            start,
            end,
        ));
    }

    fn check_alias_method_call(&mut self, node: &ruby_prism::CallNode) {
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };

        let arguments: Vec<_> = args.arguments().iter().collect();

        // alias_method expects exactly 2 arguments; if not, skip
        if arguments.len() != 2 {
            return;
        }

        let first_arg = &arguments[0];

        // Only check symbol/string literals, skip variables and splats
        if let Some(name) = extract_name_from_node(first_arg) {
            let loc = first_arg.location();
            self.check_name(&name, loc.start_offset(), loc.end_offset());
        }
    }

    fn check_define_method_call(&mut self, node: &ruby_prism::CallNode) {
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };

        let arguments: Vec<_> = args.arguments().iter().collect();
        if arguments.is_empty() {
            return;
        }

        let first_arg = &arguments[0];

        if let Some(name) = extract_name_from_node(first_arg) {
            // Operator methods defined via define_method are accepted
            if is_operator(&name) {
                return;
            }
            let loc = first_arg.location();
            self.check_name(&name, loc.start_offset(), loc.end_offset());
        }
    }

    fn check_struct_new_or_data_define(&mut self, node: &ruby_prism::CallNode) {
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };

        let method_name = String::from_utf8_lossy(node.name().as_slice());
        let is_struct_new = method_name == "new";

        let arguments: Vec<_> = args.arguments().iter().collect();
        let mut skip_first_string = is_struct_new;

        for arg in &arguments {
            match arg {
                Node::StringNode { .. } if skip_first_string => {
                    // First string arg to Struct.new is the struct name, skip it
                    skip_first_string = false;
                }
                Node::SymbolNode { .. } | Node::StringNode { .. } => {
                    if let Some(name) = extract_name_from_node(arg) {
                        let loc = arg.location();
                        self.check_name(&name, loc.start_offset(), loc.end_offset());
                    }
                    skip_first_string = false;
                }
                _ => {
                    skip_first_string = false;
                }
            }
        }
    }

    fn check_alias_node(&mut self, node: &ruby_prism::AliasMethodNode) {
        let new_name_node = node.new_name();

        if let Some(name) = extract_name_from_node(&new_name_node) {
            if is_operator(&name) {
                return;
            }
            let loc = new_name_node.location();
            self.check_name(&name, loc.start_offset(), loc.end_offset());
        }
    }

    fn is_struct_new_or_data_define(&self, node: &ruby_prism::CallNode) -> bool {
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return false,
        };

        match method_name.as_ref() {
            "new" => {
                if let Some(c) = receiver.as_constant_read_node() {
                    let name = String::from_utf8_lossy(c.name().as_slice());
                    name == "Struct"
                } else if let Some(cp) = receiver.as_constant_path_node() {
                    // ::Struct.new
                    if cp.parent().is_some() {
                        return false;
                    }
                    let child_name = String::from_utf8_lossy(cp.name().unwrap().as_slice());
                    child_name == "Struct"
                } else {
                    false
                }
            }
            "define" => {
                if let Some(c) = receiver.as_constant_read_node() {
                    let name = String::from_utf8_lossy(c.name().as_slice());
                    name == "Data"
                } else if let Some(cp) = receiver.as_constant_path_node() {
                    // ::Data.define
                    if cp.parent().is_some() {
                        return false;
                    }
                    let child_name = String::from_utf8_lossy(cp.name().unwrap().as_slice());
                    child_name == "Data"
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Collect all class names defined in the AST (for class emitter detection)
    fn collect_class_names(&mut self, node: &Node) {
        if let Some(c) = node.as_class_node() {
            let path = c.constant_path();
            if let Some(cr) = path.as_constant_read_node() {
                let name = String::from_utf8_lossy(cr.name().as_slice()).to_string();
                self.class_names.push(name);
            }
            if let Some(body) = c.body() {
                self.collect_class_names_in_body(&body);
            }
        } else if let Some(m) = node.as_module_node() {
            if let Some(body) = m.body() {
                self.collect_class_names_in_body(&body);
            }
        } else if let Some(p) = node.as_program_node() {
            for stmt in p.statements().body().iter() {
                self.collect_class_names(&stmt);
            }
        }
    }

    fn collect_class_names_in_body(&mut self, node: &Node) {
        if let Some(stmts) = node.as_statements_node() {
            for stmt in stmts.body().iter() {
                self.collect_class_names(&stmt);
            }
        } else if let Some(b) = node.as_begin_node() {
            if let Some(stmts) = b.statements() {
                for stmt in stmts.body().iter() {
                    self.collect_class_names(&stmt);
                }
            }
        } else {
            self.collect_class_names(node);
        }
    }
}

impl Visit<'_> for MethodNameVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.check_def_node(node);
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        if node.receiver().is_none() {
            match method_name.as_str() {
                "attr" | "attr_reader" | "attr_writer" | "attr_accessor" => {
                    self.check_attr_call(node);
                }
                "alias_method" => {
                    self.check_alias_method_call(node);
                }
                "define_method" | "define_singleton_method" => {
                    self.check_define_method_call(node);
                }
                _ => {}
            }
        }

        if self.is_struct_new_or_data_define(node) {
            self.check_struct_new_or_data_define(node);
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_alias_method_node(&mut self, node: &ruby_prism::AliasMethodNode) {
        self.check_alias_node(node);
    }
}

impl Cop for MethodName {
    fn name(&self) -> &'static str {
        "Naming/MethodName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = MethodNameVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
            class_names: Vec::new(),
        };

        // First pass: collect class names for class emitter detection
        for stmt in node.statements().body().iter() {
            visitor.collect_class_names(&stmt);
        }

        // Second pass: check method names
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
