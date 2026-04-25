//! Gemspec/DevelopmentDependencies cop
//!
//! Enforce where development dependencies are specified: `Gemfile` (default),
//! `gems.rb`, or `gemspec`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Clone, Copy, PartialEq)]
enum Style { Gemfile, GemsRb, Gemspec }

pub struct DevelopmentDependencies {
    style: Style,
    allowed_gems: Vec<String>,
}

impl DevelopmentDependencies {
    pub fn new(style_s: &str, allowed_gems: Vec<String>) -> Self {
        let style = match style_s {
            "gems.rb" => Style::GemsRb,
            "gemspec" => Style::Gemspec,
            _ => Style::Gemfile,
        };
        Self { style, allowed_gems }
    }
}

impl Cop for DevelopmentDependencies {
    fn name(&self) -> &'static str { "Gemspec/DevelopmentDependencies" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let is_add_dev = method.as_ref() == "add_development_dependency";
        let is_gem = method.as_ref() == "gem";
        if !is_add_dev && !is_gem { return vec![]; }

        // Extract first arg as string literal
        let args = match node.arguments() { Some(a) => a, None => return vec![] };
        let first = match args.arguments().iter().next() { Some(f) => f, None => return vec![] };
        let gem_name = match first.as_string_node() {
            Some(s) => String::from_utf8_lossy(s.unescaped()).to_string(),
            None => return vec![],
        };
        if self.allowed_gems.iter().any(|g| g == &gem_name) { return vec![]; }

        let flag = match (self.style, is_add_dev, is_gem) {
            (Style::Gemfile | Style::GemsRb, true, _) => true,
            (Style::Gemspec, _, true) => true,
            _ => false,
        };
        if !flag { return vec![]; }

        let preferred = match self.style {
            Style::Gemfile => "Gemfile",
            Style::GemsRb => "gems.rb",
            Style::Gemspec => "gemspec",
        };
        let msg = format!("Specify development dependencies in {}.", preferred);
        let loc = node.location();
        vec![ctx.offense_with_range(self.name(), &msg, self.severity(), loc.start_offset(), loc.end_offset())]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
    allowed_gems: Option<Vec<String>>,
}

crate::register_cop!("Gemspec/DevelopmentDependencies", |cfg| {
    let c: Cfg = cfg.typed("Gemspec/DevelopmentDependencies");
    let style = c.enforced_style.unwrap_or_else(|| "Gemfile".to_string());
    let allowed = c.allowed_gems.unwrap_or_default();
    Some(Box::new(DevelopmentDependencies::new(&style, allowed)))
});
