//! Layout/ClassStructure - Enforces class element ordering.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/class_structure.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

const COP_NAME: &str = "Layout/ClassStructure";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visibility {
    Public,
    Protected,
    Private,
}

pub struct ClassStructure {
    expected_order: Vec<String>,
    /// Maps method/macro name → category key (e.g. "attr_accessor" → "attribute_macros")
    name_to_category: HashMap<String, String>,
}

impl Default for ClassStructure {
    fn default() -> Self {
        Self {
            expected_order: Vec::new(),
            name_to_category: HashMap::new(),
        }
    }
}

impl ClassStructure {
    pub fn new(expected_order: Vec<String>, categories: HashMap<String, Vec<String>>) -> Self {
        let mut name_to_category = HashMap::new();
        for (cat, names) in categories {
            for n in names {
                name_to_category.insert(n, cat.clone());
            }
        }
        Self {
            expected_order,
            name_to_category,
        }
    }

    fn find_category(&self, name: &str) -> Option<&str> {
        self.name_to_category.get(name).map(|s| s.as_str())
    }

    /// Returns Some(category) for nodes that should be classified, None for skip-only nodes
    /// (visibility modifiers, private_constant, dynamic constants, etc.).
    fn classify(
        &self,
        node: &Node,
        visibility: Visibility,
        private_constants: &std::collections::HashSet<String>,
        private_named: &std::collections::HashSet<String>,
        protected_named: &std::collections::HashSet<String>,
    ) -> Option<String> {
        match node {
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                let raw_name = def.name();
                let name = String::from_utf8_lossy(raw_name.as_slice()).into_owned();
                // self.x → public_class_methods
                if def.receiver().is_some() {
                    return Some("public_class_methods".to_string());
                }
                if name == "initialize" {
                    return Some("initializer".to_string());
                }
                let v = if private_named.contains(&name) {
                    Visibility::Private
                } else if protected_named.contains(&name) {
                    Visibility::Protected
                } else {
                    visibility
                };
                Some(format!("{}_methods", visibility_str(v)))
            }
            Node::ConstantWriteNode { .. } => {
                let cw = node.as_constant_write_node().unwrap();
                let raw = cw.name();
                let const_name = String::from_utf8_lossy(raw.as_slice()).into_owned();
                if private_constants.contains(&const_name) {
                    return None;
                }
                // Check categories override for "constants" key
                if let Some(cat) = self.find_category("constants") {
                    return Some(cat.to_string());
                }
                Some("constants".to_string())
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if call.receiver().is_some() {
                    return None;
                }
                let raw_name = node_name!(call);
                let name: String = raw_name.as_ref().to_string();

                // visibility modifier (no args) — skip; caller handles toggling.
                if matches!(name.as_str(), "private" | "public" | "protected") {
                    let arg_count = call
                        .arguments()
                        .map(|a| a.arguments().iter().count())
                        .unwrap_or(0);
                    if arg_count == 0 {
                        return None;
                    }
                    // private :foo / private def foo
                    // def_modifier? → call has a single DefNode arg
                    if let Some(args) = call.arguments() {
                        let arg_list: Vec<_> = args.arguments().iter().collect();
                        if arg_list.len() == 1 {
                            if matches!(arg_list[0], Node::DefNode { .. }) {
                                return Some(format!("{}_methods", name));
                            }
                        }
                    }
                    // private :foo, :bar → not classified (acts as visibility marker for prior defs)
                    return None;
                }

                // private_constant marker
                if name == "private_constant" {
                    return None;
                }

                let category = self.find_category(&name);
                let key = category.unwrap_or(&name).to_string();
                let visibility_key = format!("{}_{}", visibility_str(visibility), key);
                if self.expected_order.iter().any(|e| e == &visibility_key) {
                    Some(visibility_key)
                } else {
                    Some(key)
                }
            }
            _ => None,
        }
    }

    fn check_body_children<'a>(
        &self,
        children: &[Node<'a>],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        // First pass: collect private_constants list and private/protected method names
        let mut private_constants: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut private_named: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut protected_named: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for child in children {
            if let Node::CallNode { .. } = child {
                let call = child.as_call_node().unwrap();
                if call.receiver().is_some() {
                    continue;
                }
                let n = node_name!(call);
                let n_str = n.as_ref();
                let arg_count = call
                    .arguments()
                    .map(|a| a.arguments().iter().count())
                    .unwrap_or(0);
                let target_set: Option<&mut std::collections::HashSet<String>> = match n_str {
                    "private_constant" => Some(&mut private_constants),
                    "private" if arg_count > 0 => Some(&mut private_named),
                    "protected" if arg_count > 0 => Some(&mut protected_named),
                    _ => None,
                };
                let Some(target) = target_set else {
                    continue;
                };
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    // Skip `private def foo` form — DefNode arg means modifier-style, not name list
                    if arg_list.iter().any(|a| matches!(a, Node::DefNode { .. })) {
                        continue;
                    }
                    for a in &arg_list {
                        match a {
                            Node::SymbolNode { .. } => {
                                let sym = a.as_symbol_node().unwrap();
                                let bytes = sym.unescaped();
                                let b: &[u8] = bytes.as_ref();
                                let s = String::from_utf8_lossy(b).into_owned();
                                target.insert(s);
                            }
                            Node::StringNode { .. } => {
                                let st = a.as_string_node().unwrap();
                                let bytes = st.unescaped();
                                let b: &[u8] = bytes.as_ref();
                                let s = String::from_utf8_lossy(b).into_owned();
                                target.insert(s);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Second pass: walk children, track visibility, classify, emit offense if out-of-order.
        let mut visibility = Visibility::Public;
        let mut prev_index: i32 = -1;
        for child in children {
            // Visibility-only marker?
            if let Node::CallNode { .. } = child {
                let call = child.as_call_node().unwrap();
                if call.receiver().is_none() {
                    let n = node_name!(call);
                    let arg_count = call
                        .arguments()
                        .map(|a| a.arguments().iter().count())
                        .unwrap_or(0);
                    if arg_count == 0
                        && matches!(n.as_ref(), "private" | "protected" | "public")
                    {
                        visibility = match n.as_ref() {
                            "private" => Visibility::Private,
                            "protected" => Visibility::Protected,
                            _ => Visibility::Public,
                        };
                        continue;
                    }
                }
            }

            let Some(category) = self.classify(
                child,
                visibility,
                &private_constants,
                &private_named,
                &protected_named,
            ) else {
                continue;
            };
            let Some(idx) = self.expected_order.iter().position(|e| e == &category) else {
                continue;
            };
            let idx = idx as i32;
            if idx < prev_index {
                let prev = &self.expected_order[prev_index as usize];
                let message = format!(
                    "`{}` is supposed to appear before `{}`.",
                    category, prev
                );
                let loc = child.location();
                offenses.push(ctx.offense_with_range(
                    COP_NAME,
                    &message,
                    Severity::Convention,
                    loc.start_offset(),
                    loc.end_offset(),
                ));
            }
            prev_index = idx;
        }
    }

    fn check_body_node(&self, body_node: &Node, ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        // body may be a StatementsNode or a single statement.
        if let Some(stmts) = body_node.as_statements_node() {
            let children: Vec<Node> = stmts.body().iter().collect();
            self.check_body_children(&children, ctx, offenses);
        }
        // Single-statement bodies have nothing to reorder, so skip them.
    }
}

fn visibility_str(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    }
}

struct ClassStructureVisitor<'a, 'src> {
    cop: &'a ClassStructure,
    ctx: &'a CheckContext<'src>,
    offenses: Vec<Offense>,
}

impl<'a, 'src> Visit<'src> for ClassStructureVisitor<'a, 'src> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'src>) {
        if let Some(body) = node.body() {
            self.cop.check_body_node(&body, self.ctx, &mut self.offenses);
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'src>) {
        if let Some(body) = node.body() {
            self.cop.check_body_node(&body, self.ctx, &mut self.offenses);
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

impl Cop for ClassStructure {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = ClassStructureVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Layout/ClassStructure", |cfg| {
    let mut expected_order = Vec::new();
    let mut categories: HashMap<String, Vec<String>> = HashMap::new();

    if let Some(cc) = cfg.get_cop_config("Layout/ClassStructure") {
        if let Some(serde_yaml::Value::Sequence(seq)) = cc.raw.get("ExpectedOrder") {
            for v in seq {
                if let Some(s) = v.as_str() {
                    expected_order.push(s.to_string());
                }
            }
        }
        if let Some(serde_yaml::Value::Mapping(m)) = cc.raw.get("Categories") {
            for (k, v) in m {
                let Some(key) = k.as_str() else { continue };
                let Some(serde_yaml::Value::Sequence(seq)) = Some(v) else {
                    continue;
                };
                let names: Vec<String> = seq
                    .iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect();
                categories.insert(key.to_string(), names);
            }
        }
    }
    Some(Box::new(ClassStructure::new(expected_order, categories)))
});
