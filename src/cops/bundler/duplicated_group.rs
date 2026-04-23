//! Bundler/DuplicatedGroup cop
//! Checks that there are no duplicate `group` blocks in a Gemfile.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/bundler/duplicated_group.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct DuplicatedGroup;

impl DuplicatedGroup {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicatedGroup {
    fn name(&self) -> &'static str { "Bundler/DuplicatedGroup" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only run on Gemfile-like files
        let basename = ctx.filename.rsplit('/').next().unwrap_or(ctx.filename);
        if basename != "Gemfile" && basename != "gems.rb" && !basename.ends_with(".gemfile") {
            return vec![];
        }

        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();

        let mut collector = GroupCallCollector {
            source: ctx.source,
            groups: Vec::new(),
            context_stack: Vec::new(),
        };
        collector.visit(&tree);

        // Group calls by context + attributes (like RuboCop's group_keys hash)
        let mut offenses = Vec::new();

        // For each group, find earlier groups with same key
        for i in 0..collector.groups.len() {
            let b = &collector.groups[i];
            for j in 0..i {
                let a = &collector.groups[j];

                // Must be in same context
                if a.context != b.context {
                    continue;
                }

                // Groups match if their attributes are equal (sorted)
                if a.attributes_key == b.attributes_key {
                    let msg = format!(
                        "Gem group `{}` already defined on line {} of the Gemfile.",
                        b.display_args,
                        a.line,
                    );
                    offenses.push(ctx.offense_with_range(
                        "Bundler/DuplicatedGroup", &msg, Severity::Convention,
                        b.node_start,
                        b.node_end,
                    ));
                    break; // Only report the first match
                }
            }
        }

        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

#[derive(Debug)]
struct GroupCall {
    line: usize,
    node_start: usize,
    node_end: usize,
    /// Sorted attributes key for equality comparison (normalized)
    attributes_key: String,
    /// Display string for the offense message (raw source of args)
    display_args: String,
    /// Context: the enclosing source/git/platforms/path call's key (or empty)
    context: String,
}

struct GroupCallCollector<'a> {
    source: &'a str,
    groups: Vec<GroupCall>,
    context_stack: Vec<String>,
}

impl<'a> GroupCallCollector<'a> {
    fn current_context(&self) -> String {
        self.context_stack.last().cloned().unwrap_or_default()
    }

    fn node_source(&self, start: usize, end: usize) -> &str {
        &self.source[start..end]
    }

    /// Normalize an argument's value for comparison:
    /// - Symbol :foo → "foo"
    /// - String 'foo' → "foo"
    /// - Other → raw source
    fn normalize_arg_value(&self, node: &Node) -> String {
        match node {
            Node::SymbolNode { .. } => {
                let s = node.as_symbol_node().unwrap();
                String::from_utf8_lossy(s.unescaped()).to_string()
            }
            Node::StringNode { .. } => {
                let s = node.as_string_node().unwrap();
                String::from_utf8_lossy(s.unescaped()).to_string()
            }
            Node::SplatNode { .. } => {
                let splat = node.as_splat_node().unwrap();
                let loc = node.location();
                if let Some(expr) = splat.expression() {
                    format!("*{}", self.node_source(
                        expr.location().start_offset(),
                        expr.location().end_offset(),
                    ))
                } else {
                    self.node_source(loc.start_offset(), loc.end_offset()).to_string()
                }
            }
            _ => {
                let loc = node.location();
                self.node_source(loc.start_offset(), loc.end_offset()).to_string()
            }
        }
    }

    /// Display representation of an argument (raw source)
    fn display_arg(&self, node: &Node) -> String {
        let loc = node.location();
        self.node_source(loc.start_offset(), loc.end_offset()).to_string()
    }

    fn extract_group_call(&self, node: &ruby_prism::CallNode) -> Option<GroupCall> {
        let loc = node.location();
        let line = 1 + self.source[..loc.start_offset()].bytes().filter(|&b| b == b'\n').count();

        let args = node.arguments()?;
        let arg_list: Vec<Node> = args.arguments().iter().collect();

        if arg_list.is_empty() {
            return None;
        }

        // Offense ends at end of arguments, or closing paren if present
        let args_end = node.closing_loc()
            .map(|cl| cl.end_offset())
            .unwrap_or_else(|| args.location().end_offset());

        // Build attributes key (for duplicate detection, normalized and sorted)
        // Mirrors RuboCop's group_attributes:
        // - For non-hash args: normalize value (symbol/string to plain string)
        // - For keyword hash: sort pairs by source
        let mut attribute_parts: Vec<String> = Vec::new();

        for arg in &arg_list {
            match arg {
                Node::KeywordHashNode { .. } => {
                    let hash = arg.as_keyword_hash_node().unwrap();
                    let mut pair_strs: Vec<String> = Vec::new();
                    for elem in hash.elements().iter() {
                        let loc = elem.location();
                        pair_strs.push(self.node_source(loc.start_offset(), loc.end_offset()).to_string());
                    }
                    pair_strs.sort();
                    attribute_parts.push(pair_strs.join(", "));
                }
                Node::HashNode { .. } => {
                    let hash = arg.as_hash_node().unwrap();
                    let mut pair_strs: Vec<String> = Vec::new();
                    for elem in hash.elements().iter() {
                        let loc = elem.location();
                        pair_strs.push(self.node_source(loc.start_offset(), loc.end_offset()).to_string());
                    }
                    pair_strs.sort();
                    attribute_parts.push(pair_strs.join(", "));
                }
                _ => {
                    attribute_parts.push(self.normalize_arg_value(arg));
                }
            }
        }

        // Sort the attribute parts for order-independent comparison
        // (e.g., group :test, :development == group :development, :test)
        attribute_parts.sort();
        let attributes_key = attribute_parts.join("|");

        // Display: raw source of each argument
        let display_parts: Vec<String> = arg_list.iter()
            .filter(|arg| !matches!(arg, Node::KeywordHashNode { .. } | Node::HashNode { .. }))
            .map(|arg| self.display_arg(arg))
            .chain(
                arg_list.iter()
                    .filter(|arg| matches!(arg, Node::KeywordHashNode { .. } | Node::HashNode { .. }))
                    .flat_map(|arg| {
                        if let Node::KeywordHashNode { .. } = arg {
                            let hash = arg.as_keyword_hash_node().unwrap();
                            hash.elements().iter().map(|e| {
                                let loc = e.location();
                                self.node_source(loc.start_offset(), loc.end_offset()).to_string()
                            }).collect::<Vec<_>>()
                        } else if let Node::HashNode { .. } = arg {
                            let hash = arg.as_hash_node().unwrap();
                            hash.elements().iter().map(|e| {
                                let loc = e.location();
                                self.node_source(loc.start_offset(), loc.end_offset()).to_string()
                            }).collect::<Vec<_>>()
                        } else {
                            vec![]
                        }
                    })
            )
            .collect();

        let display_args = display_parts.join(", ");

        if display_args.is_empty() {
            return None;
        }

        Some(GroupCall {
            line,
            node_start: loc.start_offset(),
            node_end: args_end,
            attributes_key,
            display_args,
            context: self.current_context(),
        })
    }
}

impl Visit<'_> for GroupCallCollector<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();

        match method.as_str() {
            "group" if node.receiver().is_none() => {
                if let Some(gc) = self.extract_group_call(node) {
                    self.groups.push(gc);
                }
                // Recurse into the block
                ruby_prism::visit_call_node(self, node);
            }
            "source" | "git" | "platforms" | "path" if node.receiver().is_none() => {
                // These create a new context scope
                let context_key = {
                    let arg_str = node.arguments()
                        .and_then(|args| args.arguments().iter().next())
                        .map(|arg| self.normalize_arg_value(&arg))
                        .unwrap_or_default();
                    format!("{}:{}", method, arg_str)
                };
                self.context_stack.push(context_key);
                ruby_prism::visit_call_node(self, node);
                self.context_stack.pop();
            }
            _ => {
                ruby_prism::visit_call_node(self, node);
            }
        }
    }
}

crate::register_cop!("Bundler/DuplicatedGroup", |_cfg| Some(Box::new(DuplicatedGroup::new())));
