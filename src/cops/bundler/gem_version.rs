//! Bundler/GemVersion - require/forbid version specs (or commit refs) on `gem` calls.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/bundler/gem_version.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
const REQUIRED_MSG: &str = "Gem version specification is required.";
const FORBIDDEN_MSG: &str = "Gem version specification is forbidden.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Required,
    Forbidden,
}

pub struct GemVersion {
    style: Style,
    allowed_gems: Vec<String>,
}

impl GemVersion {
    pub fn new(style: Style, allowed_gems: Vec<String>) -> Self {
        Self { style, allowed_gems }
    }
}

impl Default for GemVersion {
    fn default() -> Self {
        Self::new(Style::Required, Vec::new())
    }
}

fn version_specification(s: &str) -> bool {
    // /^\s*[~<>=]*\s*[0-9.]+/
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    while i < bytes.len() && matches!(bytes[i], b'~' | b'<' | b'>' | b'=') { i += 1; }
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    let start = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') { i += 1; }
    i > start
}

fn first_string_arg<'a>(call: &ruby_prism::CallNode<'a>) -> Option<String> {
    let args = call.arguments()?;
    let first = args.arguments().iter().next()?;
    let s = first.as_string_node()?;
    Some(String::from_utf8_lossy(s.unescaped()).to_string())
}

fn includes_version_specification(call: &ruby_prism::CallNode) -> bool {
    let Some(args) = call.arguments() else { return false; };
    // skip first positional (gem name)
    for (i, arg) in args.arguments().iter().enumerate() {
        if i == 0 { continue; }
        if let Some(s) = arg.as_string_node() {
            let text = String::from_utf8_lossy(s.unescaped());
            if version_specification(&text) { return true; }
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
            let key = pair.key();
            let key_name = if let Some(sym) = key.as_symbol_node() {
                String::from_utf8_lossy(sym.unescaped()).to_string()
            } else {
                continue;
            };
            if !matches!(key_name.as_str(), "branch" | "ref" | "tag") { continue; }
            // value must be a string
            if pair.value().as_string_node().is_some() {
                return true;
            }
        }
    }
    false
}

impl Cop for GemVersion {
    fn name(&self) -> &'static str { "Bundler/GemVersion" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "gem" { return vec![]; }
        if node.receiver().is_some() { return vec![]; }
        let Some(name) = first_string_arg(node) else { return vec![]; };
        if self.allowed_gems.iter().any(|g| g == &name) { return vec![]; }

        let has_version = includes_version_specification(node);
        let has_commit = includes_commit_reference(node);

        let offense = match self.style {
            Style::Required => !has_version && !has_commit,
            Style::Forbidden => has_version || has_commit,
        };
        if !offense { return vec![]; }

        let msg = match self.style {
            Style::Required => REQUIRED_MSG,
            Style::Forbidden => FORBIDDEN_MSG,
        };
        let loc = node.location();
        vec![ctx.offense_with_range(self.name(), msg, self.severity(), loc.start_offset(), loc.end_offset())]
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct GemVersionCfg {
    enforced_style: String,
    allowed_gems: Vec<String>,
}

impl Default for GemVersionCfg {
    fn default() -> Self { Self { enforced_style: "required".to_string(), allowed_gems: Vec::new() } }
}

crate::register_cop!("Bundler/GemVersion", |cfg| {
    let c: GemVersionCfg = cfg.typed("Bundler/GemVersion");
    let style = match c.enforced_style.as_str() {
        "forbidden" => Style::Forbidden,
        _ => Style::Required,
    };
    Some(Box::new(GemVersion::new(style, c.allowed_gems)))
});

