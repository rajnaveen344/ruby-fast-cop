//! Lint/StructNewOverride - Don't override Struct built-in methods.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/struct_new_override.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

static STRUCT_METHODS: &[&str] = &[
    "!", "!=", "!~", "<=>", "==", "===", "[]", "[]=",
    "__id__", "__send__", "all?", "any?", "chain", "chunk", "chunk_while",
    "class", "clone", "collect", "collect_concat", "compact", "count", "cycle",
    "deconstruct", "deconstruct_keys", "define_singleton_method", "detect", "dig",
    "display", "drop", "drop_while", "dup", "each", "each_cons", "each_entry",
    "each_pair", "each_slice", "each_with_index", "each_with_object", "entries",
    "enum_for", "eql?", "equal?", "extend", "filter", "filter_map", "find",
    "find_all", "find_index", "first", "flat_map", "freeze", "frozen?", "grep",
    "grep_v", "group_by", "hash", "include?", "inject", "inspect",
    "instance_eval", "instance_exec", "instance_of?", "instance_variable_defined?",
    "instance_variable_get", "instance_variable_set", "instance_variables",
    "is_a?", "itself", "kind_of?", "lazy", "length", "map", "max", "max_by",
    "member?", "members", "method", "methods", "min", "min_by", "minmax",
    "minmax_by", "nil?", "none?", "object_id", "one?", "partition",
    "private_methods", "protected_methods", "public_method", "public_methods",
    "public_send", "reduce", "reject", "remove_instance_variable", "respond_to?",
    "reverse_each", "select", "send", "singleton_class", "singleton_method",
    "singleton_methods", "size", "slice_after", "slice_before", "slice_when",
    "sort", "sort_by", "sum", "take", "take_while", "tally", "tap", "then",
    "to_a", "to_enum", "to_h", "to_s", "to_set", "uniq", "values", "values_at",
    "yield_self", "zip",
];

fn struct_methods_set() -> HashSet<&'static str> {
    STRUCT_METHODS.iter().copied().collect()
}

#[derive(Default)]
pub struct StructNewOverride;

impl StructNewOverride {
    pub fn new() -> Self {
        Self
    }
}

struct StructNewOverrideVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    struct_methods: HashSet<&'static str>,
}

impl StructNewOverrideVisitor<'_> {
    fn is_struct_receiver(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let c = node.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(c.name().as_slice());
                name == "Struct"
            }
            Node::ConstantPathNode { .. } => {
                let cp = node.as_constant_path_node().unwrap();
                if cp.parent().is_some() {
                    return false;
                }
                let const_id = match cp.name() {
                    Some(id) => id,
                    None => return false,
                };
                let name = String::from_utf8_lossy(const_id.as_slice());
                name == "Struct"
            }
            _ => false,
        }
    }

    fn member_name(node: &Node) -> Option<String> {
        match node {
            Node::SymbolNode { .. } => {
                let s = node.as_symbol_node().unwrap();
                // SymbolNode: value content is in value_loc (the text without colon)
                if let Some(loc) = s.value_loc() {
                    Some(String::from_utf8_lossy(loc.as_slice()).into_owned())
                } else {
                    None
                }
            }
            Node::StringNode { .. } => {
                let s = node.as_string_node().unwrap();
                Some(String::from_utf8_lossy(s.unescaped()).into_owned())
            }
            _ => None,
        }
    }

    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if method != "new" {
            return;
        }

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        if !Self::is_struct_receiver(&receiver) {
            return;
        }

        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };

        let args_list: Vec<Node> = args.arguments().iter().collect();
        if args_list.is_empty() {
            return;
        }

        // Skip first arg if it's a string (class name like `Struct.new("MyStruct", ...)`)
        let start_idx = if matches!(args_list[0], Node::StringNode { .. }) { 1 } else { 0 };

        let src = self.ctx.source;
        for arg in &args_list[start_idx..] {
            // Skip keyword hash args
            if matches!(arg, Node::KeywordHashNode { .. }) {
                continue;
            }
            if let Some(name) = Self::member_name(arg) {
                if self.struct_methods.contains(name.as_str()) {
                    let loc = arg.location();
                    let repr = &src[loc.start_offset()..loc.end_offset()];
                    let msg = format!(
                        "`{}` member overrides `Struct#{}` and it may be unexpected.",
                        repr, name
                    );
                    let offense = self.ctx.offense_with_range(
                        "Lint/StructNewOverride",
                        &msg,
                        Severity::Warning,
                        loc.start_offset(),
                        loc.end_offset(),
                    );
                    self.offenses.push(offense);
                }
            }
        }
    }
}

impl Visit<'_> for StructNewOverrideVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for StructNewOverride {
    fn name(&self) -> &'static str {
        "Lint/StructNewOverride"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = StructNewOverrideVisitor {
            ctx,
            offenses: Vec::new(),
            struct_methods: struct_methods_set(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/StructNewOverride", |_cfg| {
    Some(Box::new(StructNewOverride::new()))
});
