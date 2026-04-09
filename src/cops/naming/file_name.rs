//! Naming/FileName cop
//!
//! Makes sure that Ruby source files have snake_case names.
//! Ruby scripts (with a shebang) are ignored by default.
//! Optionally checks that the file defines a class/module matching the filename.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use ruby_prism::Node;
use std::path::Path;

const SNAKE_CASE_RE: &str = r"^[\d\p{Ll}_.?!]+$";

pub struct FileName {
    ignore_executable_scripts: bool,
    expect_matching_definition: bool,
    check_definition_path_hierarchy: bool,
    check_definition_path_hierarchy_roots: Vec<String>,
    regex: Option<String>,
    allowed_acronyms: Vec<String>,
}

impl Default for FileName {
    fn default() -> Self {
        Self {
            ignore_executable_scripts: true,
            expect_matching_definition: false,
            check_definition_path_hierarchy: true,
            check_definition_path_hierarchy_roots: vec![
                "lib".into(), "spec".into(), "test".into(), "src".into(),
            ],
            regex: None,
            allowed_acronyms: vec![],
        }
    }
}

impl FileName {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_full_config(
        ignore_executable_scripts: bool,
        expect_matching_definition: bool,
        check_definition_path_hierarchy: bool,
        check_definition_path_hierarchy_roots: Vec<String>,
        regex: Option<String>,
        allowed_acronyms: Vec<String>,
    ) -> Self {
        Self {
            ignore_executable_scripts,
            expect_matching_definition,
            check_definition_path_hierarchy,
            check_definition_path_hierarchy_roots,
            regex,
            allowed_acronyms,
        }
    }

    fn filename_good(&self, basename: &str) -> bool {
        let mut name = basename.to_string();
        if name.starts_with('.') {
            name = name[1..].to_string();
        }
        if let Some(pos) = name.find('.') {
            name = name[..pos].to_string();
        }
        name = name.replace('+', "_");

        if let Some(ref regex_str) = self.regex {
            if let Ok(re) = Regex::new(regex_str) {
                return re.is_match(&name);
            }
        }

        let snake_re = Regex::new(SNAKE_CASE_RE).unwrap();
        snake_re.is_match(&name)
    }

    fn bad_filename_allowed(&self, source: &str) -> bool {
        self.ignore_executable_scripts && source.starts_with("#!")
    }

    fn to_module_name(basename: &str) -> String {
        let name = if let Some(pos) = basename.find('.') {
            &basename[..pos]
        } else {
            basename
        };
        name.split('_')
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => {
                        let upper: String = c.to_uppercase().collect();
                        upper + &chars.collect::<String>()
                    }
                }
            })
            .collect::<String>()
    }

    fn to_namespace(&self, path: &str) -> Vec<String> {
        let components: Vec<&str> = Path::new(path)
            .components()
            .filter_map(|c| {
                if let std::path::Component::Normal(s) = c {
                    s.to_str()
                } else {
                    None
                }
            })
            .collect();

        let roots = &self.check_definition_path_hierarchy_roots;
        let mut start_index = None;

        for (i, c) in components.iter().rev().enumerate() {
            if roots.iter().any(|r| r == c) {
                start_index = Some(components.len() - i);
                break;
            }
        }

        match start_index {
            None => vec![Self::to_module_name(components.last().unwrap_or(&""))],
            Some(idx) => components[idx..]
                .iter()
                .map(|c| Self::to_module_name(c))
                .collect(),
        }
    }

    fn match_acronym(&self, expected: &str, name: &str) -> bool {
        self.allowed_acronyms.iter().any(|acronym| {
            let capitalized = capitalize_first(acronym);
            expected.replace(&capitalized, acronym) == name
        })
    }

    fn perform_class_and_module_naming_checks(
        &self,
        file_path: &str,
        basename: &str,
        program: &ruby_prism::ProgramNode,
    ) -> Option<String> {
        if !self.expect_matching_definition {
            return None;
        }

        if self.check_definition_path_hierarchy {
            let namespace = self.to_namespace(file_path);
            if !self.find_matching(program, &namespace) {
                return Some(self.no_definition_message(basename, file_path));
            }
        } else if !self.matching_class(program, basename) {
            return Some(self.no_definition_message(basename, basename));
        }

        None
    }

    fn matching_class(&self, program: &ruby_prism::ProgramNode, file_name: &str) -> bool {
        let namespace = self.to_namespace(file_name);
        self.find_matching(program, &namespace)
    }

    fn find_matching(&self, program: &ruby_prism::ProgramNode, namespace: &[String]) -> bool {
        if namespace.is_empty() {
            return true;
        }

        let defs = collect_definitions(program);
        let target_name = namespace.last().unwrap();
        let expected_ns = &namespace[..namespace.len() - 1];

        for (def_name, def_ns) in &defs {
            if def_name == target_name || self.match_acronym(target_name, def_name) {
                if expected_ns.is_empty() {
                    return true;
                }
                if self.namespace_matches(def_ns, expected_ns) {
                    return true;
                }
            }
        }

        false
    }

    fn namespace_matches(&self, actual_ns: &[String], expected_ns: &[String]) -> bool {
        let mut remaining: Vec<&String> = expected_ns.iter().collect();

        for actual in actual_ns.iter().rev() {
            if let Some(last) = remaining.last() {
                if actual == *last || self.match_acronym(last, actual) {
                    remaining.pop();
                }
            }
        }

        remaining.is_empty() || remaining == vec![&"Object".to_string()]
    }

    fn no_definition_message(&self, basename: &str, file_path: &str) -> String {
        let namespace = self.to_namespace(file_path);
        format!(
            "`{}` should define a class or module called `{}`.",
            basename,
            namespace.join("::")
        )
    }

    fn other_message(&self, basename: &str) -> String {
        if let Some(ref regex_str) = self.regex {
            format!("`{}` should match `{}`.", basename, regex_str)
        } else {
            format!(
                "The name of this source file (`{}`) should use snake_case.",
                basename
            )
        }
    }
}

impl Cop for FileName {
    fn name(&self) -> &'static str {
        "Naming/FileName"
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let file_path = ctx.filename;
        let basename = Path::new(file_path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        if basename.is_empty() {
            return vec![];
        }

        let msg = if self.filename_good(&basename) {
            self.perform_class_and_module_naming_checks(file_path, &basename, node)
        } else if self.bad_filename_allowed(ctx.source) {
            None
        } else {
            Some(self.other_message(&basename))
        };

        if let Some(message) = msg {
            let location = crate::offense::Location::new(1, 0, 1, 1);
            vec![Offense::new(
                self.name(),
                &message,
                Severity::Convention,
                location,
                file_path,
            )]
        } else {
            vec![]
        }
    }
}

// --- Definition collection ---

/// Collected definition: (name, namespace_chain)
fn collect_definitions(program: &ruby_prism::ProgramNode) -> Vec<(String, Vec<String>)> {
    let mut defs = Vec::new();
    for stmt in program.statements().body().iter() {
        collect_defs_from_node(&stmt, &[], &mut defs);
    }
    defs
}

fn collect_defs_from_node(node: &Node, namespace: &[String], defs: &mut Vec<(String, Vec<String>)>) {
    match node {
        Node::ClassNode { .. } => {
            let class_node = node.as_class_node().unwrap();
            if let Some(name) = extract_defined_name(&class_node.constant_path()) {
                let (simple_name, const_ns) = split_const_path(&name);
                let mut full_ns = namespace.to_vec();
                full_ns.extend(const_ns);
                defs.push((simple_name.clone(), full_ns.clone()));
                full_ns.push(simple_name);
                if let Some(body) = class_node.body() {
                    collect_defs_from_statements(&body, &full_ns, defs);
                }
            }
        }
        Node::ModuleNode { .. } => {
            let module_node = node.as_module_node().unwrap();
            if let Some(name) = extract_defined_name(&module_node.constant_path()) {
                let (simple_name, const_ns) = split_const_path(&name);
                let mut full_ns = namespace.to_vec();
                full_ns.extend(const_ns);
                defs.push((simple_name.clone(), full_ns.clone()));
                full_ns.push(simple_name);
                if let Some(body) = module_node.body() {
                    collect_defs_from_statements(&body, &full_ns, defs);
                }
            }
        }
        Node::ConstantWriteNode { .. } => {
            let casgn = node.as_constant_write_node().unwrap();
            let name = String::from_utf8_lossy(casgn.name().as_slice()).to_string();
            if is_struct_new(&casgn.value()) {
                defs.push((name, namespace.to_vec()));
            }
        }
        _ => {
            // Recurse into other node types that can contain definitions
            recurse_children(node, namespace, defs);
        }
    }
}

fn collect_defs_from_statements(node: &Node, namespace: &[String], defs: &mut Vec<(String, Vec<String>)>) {
    match node {
        Node::StatementsNode { .. } => {
            let stmts = node.as_statements_node().unwrap();
            for stmt in stmts.body().iter() {
                collect_defs_from_node(&stmt, namespace, defs);
            }
        }
        _ => {
            collect_defs_from_node(node, namespace, defs);
        }
    }
}

fn recurse_children(node: &Node, namespace: &[String], defs: &mut Vec<(String, Vec<String>)>) {
    match node {
        Node::BeginNode { .. } => {
            let begin = node.as_begin_node().unwrap();
            if let Some(stmts) = begin.statements() {
                for stmt in stmts.body().iter() {
                    collect_defs_from_node(&stmt, namespace, defs);
                }
            }
        }
        Node::StatementsNode { .. } => {
            let stmts = node.as_statements_node().unwrap();
            for stmt in stmts.body().iter() {
                collect_defs_from_node(&stmt, namespace, defs);
            }
        }
        _ => {}
    }
}

fn extract_defined_name(node: &Node) -> Option<String> {
    match node {
        Node::ConstantReadNode { .. } => {
            let cr = node.as_constant_read_node().unwrap();
            Some(String::from_utf8_lossy(cr.name().as_slice()).to_string())
        }
        Node::ConstantPathNode { .. } => {
            let mut parts = Vec::new();
            collect_const_path_parts(node, &mut parts);
            Some(parts.join("::"))
        }
        _ => None,
    }
}

fn collect_const_path_parts(node: &Node, parts: &mut Vec<String>) {
    match node {
        Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            if let Some(parent) = cp.parent() {
                collect_const_path_parts(&parent, parts);
            }
            if let Some(name) = cp.name() {
                parts.push(String::from_utf8_lossy(name.as_slice()).to_string());
            }
        }
        Node::ConstantReadNode { .. } => {
            let cr = node.as_constant_read_node().unwrap();
            parts.push(String::from_utf8_lossy(cr.name().as_slice()).to_string());
        }
        _ => {}
    }
}

fn split_const_path(name: &str) -> (String, Vec<String>) {
    let parts: Vec<&str> = name.split("::").collect();
    if parts.len() == 1 {
        (parts[0].to_string(), vec![])
    } else {
        let simple = parts.last().unwrap().to_string();
        let ns = parts[..parts.len() - 1]
            .iter()
            .map(|s| s.to_string())
            .collect();
        (simple, ns)
    }
}

fn is_struct_new(node: &Node) -> bool {
    // Direct call: Struct.new(...)
    if let Some(call) = node.as_call_node() {
        let method = String::from_utf8_lossy(call.name().as_slice());
        if method == "new" {
            if let Some(receiver) = call.receiver() {
                return is_struct_const(&receiver);
            }
        }
        return false;
    }
    // Block: Struct.new(...) do ... end
    // In Prism, block calls may be represented differently
    false
}

fn is_struct_const(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            let cr = node.as_constant_read_node().unwrap();
            String::from_utf8_lossy(cr.name().as_slice()) == "Struct"
        }
        Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            if cp.parent().is_none() {
                if let Some(name) = cp.name() {
                    return String::from_utf8_lossy(name.as_slice()) == "Struct";
                }
            }
            false
        }
        _ => false,
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            upper + &chars.collect::<String>().to_lowercase()
        }
    }
}
