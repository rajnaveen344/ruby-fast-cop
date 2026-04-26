//! Gemspec/DependencyVersion - require/forbid version specs (or commit refs)
//! on `add_dependency` / `add_runtime_dependency` / `add_development_dependency`
//! calls inside `Gem::Specification.new do |spec| ... end`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/gemspec/dependency_version.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const REQUIRED_MSG: &str = "Dependency version specification is required.";
const FORBIDDEN_MSG: &str = "Dependency version specification is forbidden.";

const ADD_DEP_METHODS: &[&str] = &["add_dependency", "add_runtime_dependency", "add_development_dependency"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style { Required, Forbidden }

pub struct DependencyVersion {
    style: Style,
    allowed_gems: Vec<String>,
}

impl DependencyVersion {
    pub fn new(style: Style, allowed_gems: Vec<String>) -> Self {
        Self { style, allowed_gems }
    }
}

impl Default for DependencyVersion {
    fn default() -> Self { Self::new(Style::Required, Vec::new()) }
}

fn version_specification(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    while i < bytes.len() && matches!(bytes[i], b'~' | b'<' | b'>' | b'=') { i += 1; }
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    let start = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') { i += 1; }
    i > start
}

fn includes_version_specification(call: &ruby_prism::CallNode) -> bool {
    let Some(args) = call.arguments() else { return false; };
    for (i, arg) in args.arguments().iter().enumerate() {
        if i == 0 { continue; }
        if let Some(s) = arg.as_string_node() {
            if version_specification(&String::from_utf8_lossy(s.unescaped())) { return true; }
        }
    }
    false
}

fn includes_commit_reference(call: &ruby_prism::CallNode) -> bool {
    let Some(args) = call.arguments() else { return false; };
    for arg in args.arguments().iter() {
        let elements = if let Some(h) = arg.as_keyword_hash_node() {
            h.elements()
        } else if let Some(h) = arg.as_hash_node() {
            h.elements()
        } else {
            continue;
        };
        for el in elements.iter() {
            let Some(pair) = el.as_assoc_node() else { continue; };
            let key_name = if let Some(sym) = pair.key().as_symbol_node() {
                String::from_utf8_lossy(sym.unescaped()).to_string()
            } else { continue; };
            if !matches!(key_name.as_str(), "branch" | "ref" | "tag") { continue; }
            if pair.value().as_string_node().is_some() { return true; }
        }
    }
    false
}

fn first_string_arg(call: &ruby_prism::CallNode) -> Option<String> {
    let args = call.arguments()?;
    let first = args.arguments().iter().next()?;
    let s = first.as_string_node()?;
    Some(String::from_utf8_lossy(s.unescaped()).to_string())
}

fn is_gem_specification(node: &Node) -> bool {
    let c = match node.as_constant_path_node() { Some(c) => c, None => return false };
    let name = String::from_utf8_lossy(match c.name() { Some(n) => n.as_slice(), None => return false }).to_string();
    if name != "Specification" { return false; }
    match c.parent() {
        Some(Node::ConstantReadNode { .. }) => {
            let p = c.parent().unwrap();
            let pr = p.as_constant_read_node().unwrap();
            String::from_utf8_lossy(pr.name().as_slice()) == "Gem"
        }
        _ => false,
    }
}

struct BlockVarFinder { var_name: Option<String> }

impl Visit<'_> for BlockVarFinder {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.var_name.is_some() { return; }
        if node_name!(node).as_ref() == "new" {
            if let Some(recv) = node.receiver() {
                if is_gem_specification(&recv) {
                    if let Some(block) = node.block() {
                        if let Node::BlockNode { .. } = block {
                            let bn = block.as_block_node().unwrap();
                            if let Some(params) = bn.parameters() {
                                if let Some(bp) = params.as_block_parameters_node() {
                                    if let Some(p) = bp.parameters() {
                                        if let Some(first) = p.requireds().iter().next() {
                                            if let Some(rp) = first.as_required_parameter_node() {
                                                self.var_name = Some(
                                                    String::from_utf8_lossy(rp.name().as_slice()).to_string()
                                                );
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

struct CallVisitor<'a, 'b> {
    cop: &'a DependencyVersion,
    ctx: &'a CheckContext<'b>,
    block_var: &'a str,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for CallVisitor<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if ADD_DEP_METHODS.contains(&method.as_ref()) {
            if let Some(recv) = node.receiver() {
                if let Some(local) = recv.as_local_variable_read_node() {
                    let recv_name = String::from_utf8_lossy(local.name().as_slice()).to_string();
                    if recv_name == self.block_var {
                        self.cop.check_one(node, self.ctx, &mut self.offenses);
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl DependencyVersion {
    fn check_one(&self, call: &ruby_prism::CallNode, ctx: &CheckContext, offenses: &mut Vec<Offense>) {
        // Skip if first arg is a string literal that's in the AllowedGems list.
        // If first arg isn't a string literal (e.g. `'parser'.freeze`), still proceed.
        if let Some(name) = first_string_arg(call) {
            if self.allowed_gems.iter().any(|g| g == &name) { return; }
        }

        let has_version = includes_version_specification(call);
        let has_commit = includes_commit_reference(call);

        let offense = match self.style {
            Style::Required => !has_version && !has_commit,
            Style::Forbidden => has_version || has_commit,
        };
        if !offense { return; }
        let msg = match self.style {
            Style::Required => REQUIRED_MSG,
            Style::Forbidden => FORBIDDEN_MSG,
        };
        let loc = call.location();
        offenses.push(ctx.offense_with_range(self.name(), msg, self.severity(), loc.start_offset(), loc.end_offset()));
    }
}

impl Cop for DependencyVersion {
    fn name(&self) -> &'static str { "Gemspec/DependencyVersion" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let parsed = ruby_prism::parse(ctx.source.as_bytes());
        let tree = parsed.node();
        let mut finder = BlockVarFinder { var_name: None };
        finder.visit(&tree);
        let Some(var) = finder.var_name else { return vec![]; };
        let mut visitor = CallVisitor { cop: self, ctx, block_var: &var, offenses: Vec::new() };
        visitor.visit(&tree);
        let mut offenses = visitor.offenses;
        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct DependencyVersionCfg {
    enforced_style: String,
    allowed_gems: Vec<String>,
}

impl Default for DependencyVersionCfg {
    fn default() -> Self { Self { enforced_style: "required".to_string(), allowed_gems: Vec::new() } }
}

crate::register_cop!("Gemspec/DependencyVersion", |cfg| {
    let c: DependencyVersionCfg = cfg.typed("Gemspec/DependencyVersion");
    let style = match c.enforced_style.as_str() {
        "forbidden" => Style::Forbidden,
        _ => Style::Required,
    };
    Some(Box::new(DependencyVersion::new(style, c.allowed_gems)))
});
